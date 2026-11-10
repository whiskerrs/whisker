// `whisker-image` ModuleDefinition (Windows): fetches with WinHTTP, decodes with WIC, draws with Direct2D.

#include <whisker/module.h>

#include <windows.h>
#include <wincodec.h>
#include <winhttp.h>
#include <wrl/client.h>

#include <algorithm>
#include <cctype>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <functional>
#include <list>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <string_view>
#include <thread>
#include <unordered_map>
#include <utility>
#include <vector>

#pragma comment(lib, "windowscodecs.lib")
#pragma comment(lib, "winhttp.lib")

namespace {

using Microsoft::WRL::ComPtr;

constexpr size_t kMaxDownload = 64 * 1024 * 1024;
constexpr size_t kCacheCapacity = 64;

struct Decoded {
  uint32_t width = 0;
  uint32_t height = 0;
  std::vector<uint8_t> pixels;  // Premultiplied BGRA, `width * 4` bytes per row.
};

using Load = std::pair<std::shared_ptr<const Decoded>, std::string>;

std::wstring Wide(std::string_view text) {
  if (text.empty()) return {};
  int length = MultiByteToWideChar(CP_UTF8, 0, text.data(), static_cast<int>(text.size()), nullptr, 0);
  std::wstring wide(static_cast<size_t>(length), L'\0');
  MultiByteToWideChar(CP_UTF8, 0, text.data(), static_cast<int>(text.size()), wide.data(), length);
  return wide;
}

std::string HresultMessage(const char* what, HRESULT hr) {
  char code[16];
  snprintf(code, sizeof(code), "0x%08lX", static_cast<unsigned long>(hr));
  return std::string(what) + " failed (" + code + ")";
}

std::vector<uint8_t> DecodeBase64(std::string_view text) {
  auto digit = [](char c) -> int {
    if (c >= 'A' && c <= 'Z') return c - 'A';
    if (c >= 'a' && c <= 'z') return c - 'a' + 26;
    if (c >= '0' && c <= '9') return c - '0' + 52;
    if (c == '+') return 62;
    if (c == '/') return 63;
    return -1;
  };
  std::vector<uint8_t> out;
  uint32_t buffer = 0;
  int bits = 0;
  for (char c : text) {
    int value = digit(c);
    if (value < 0) continue;
    buffer = (buffer << 6) | static_cast<uint32_t>(value);
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      out.push_back(static_cast<uint8_t>(buffer >> bits));
    }
  }
  return out;
}

// Parses a flat JSON object into its string members; `scalars` skips number, bool and null members.
bool ParseObject(std::string_view json, std::vector<std::pair<std::string, std::string>>& out,
                 bool scalars) {
  size_t i = 0;
  auto skip = [&] {
    while (i < json.size() && std::isspace(static_cast<unsigned char>(json[i]))) ++i;
  };
  auto string = [&](std::string& value) {
    skip();
    if (i >= json.size() || json[i] != '"') return false;
    ++i;
    while (i < json.size() && json[i] != '"') {
      char c = json[i++];
      if (c != '\\') {
        value.push_back(c);
        continue;
      }
      if (i >= json.size()) return false;
      char escaped = json[i++];
      switch (escaped) {
        case 'n': value.push_back('\n'); break;
        case 't': value.push_back('\t'); break;
        case 'r': value.push_back('\r'); break;
        case 'b': value.push_back('\b'); break;
        case 'f': value.push_back('\f'); break;
        case 'u': {
          if (i + 4 > json.size()) return false;
          unsigned code = std::stoul(std::string(json.substr(i, 4)), nullptr, 16);
          i += 4;
          if (code < 0x80) {
            value.push_back(static_cast<char>(code));
          } else if (code < 0x800) {
            value.push_back(static_cast<char>(0xC0 | (code >> 6)));
            value.push_back(static_cast<char>(0x80 | (code & 0x3F)));
          } else {
            value.push_back(static_cast<char>(0xE0 | (code >> 12)));
            value.push_back(static_cast<char>(0x80 | ((code >> 6) & 0x3F)));
            value.push_back(static_cast<char>(0x80 | (code & 0x3F)));
          }
          break;
        }
        default: value.push_back(escaped);
      }
    }
    if (i >= json.size()) return false;
    ++i;
    return true;
  };
  skip();
  if (i == json.size()) return true;
  if (json[i++] != '{') return false;
  skip();
  if (i < json.size() && json[i] == '}') return true;
  while (i < json.size()) {
    std::string name, value;
    if (!string(name)) return false;
    skip();
    if (i >= json.size() || json[i++] != ':') return false;
    skip();
    if (i < json.size() && json[i] == '"') {
      if (!string(value)) return false;
      out.emplace_back(std::move(name), std::move(value));
    } else {
      size_t start = i;
      while (i < json.size() && json[i] != ',' && json[i] != '}' &&
             !std::isspace(static_cast<unsigned char>(json[i]))) {
        ++i;
      }
      if (!scalars || i == start) return false;
    }
    skip();
    if (i < json.size() && json[i] == ',') {
      ++i;
      continue;
    }
    return i < json.size() && json[i] == '}';
  }
  return false;
}

// Parses the flat `{"name": "value"}` object the Rust API serialises headers as.
bool ParseHeaders(std::string_view json, std::vector<std::pair<std::string, std::string>>& out) {
  return ParseObject(json, out, false);
}

bool Download(const std::string& url, const std::string& headers, std::vector<uint8_t>& body,
              std::string& error) {
  std::vector<std::pair<std::string, std::string>> parsed;
  if (!ParseHeaders(headers, parsed)) {
    error = "invalid image request headers";
    return false;
  }
  std::wstring wide_url = Wide(url);
  URL_COMPONENTS parts{sizeof(parts)};
  wchar_t host[256]{};
  parts.lpszHostName = host;
  parts.dwHostNameLength = ARRAYSIZE(host);
  parts.dwUrlPathLength = static_cast<DWORD>(-1);
  parts.dwExtraInfoLength = static_cast<DWORD>(-1);
  if (!WinHttpCrackUrl(wide_url.c_str(), 0, 0, &parts)) {
    error = HresultMessage("WinHttpCrackUrl", HRESULT_FROM_WIN32(GetLastError()));
    return false;
  }
  std::wstring path(parts.lpszUrlPath, parts.dwUrlPathLength);
  if (parts.lpszExtraInfo) path.append(parts.lpszExtraInfo, parts.dwExtraInfoLength);
  if (path.empty()) path = L"/";

  struct Handle {
    HINTERNET value = nullptr;
    ~Handle() {
      if (value) WinHttpCloseHandle(value);
    }
  };
  Handle session{WinHttpOpen(L"Whisker", WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_NO_PROXY_NAME,
                             WINHTTP_NO_PROXY_BYPASS, 0)};
  Handle connection{session.value ? WinHttpConnect(session.value, host, parts.nPort, 0) : nullptr};
  DWORD flags = parts.nScheme == INTERNET_SCHEME_HTTPS ? WINHTTP_FLAG_SECURE : 0;
  Handle request{connection.value ? WinHttpOpenRequest(connection.value, L"GET", path.c_str(), nullptr,
                                                       WINHTTP_NO_REFERER,
                                                       WINHTTP_DEFAULT_ACCEPT_TYPES, flags)
                                  : nullptr};
  if (!request.value) {
    error = HresultMessage("WinHttpOpenRequest", HRESULT_FROM_WIN32(GetLastError()));
    return false;
  }
  std::wstring extra;
  for (auto const& [name, value] : parsed) extra += Wide(name) + L": " + Wide(value) + L"\r\n";
  if (!WinHttpSendRequest(request.value, extra.empty() ? WINHTTP_NO_ADDITIONAL_HEADERS : extra.c_str(),
                          static_cast<DWORD>(extra.size()), WINHTTP_NO_REQUEST_DATA, 0, 0, 0) ||
      !WinHttpReceiveResponse(request.value, nullptr)) {
    error = HresultMessage("HTTP request", HRESULT_FROM_WIN32(GetLastError()));
    return false;
  }
  DWORD status = 0;
  DWORD size = sizeof(status);
  WinHttpQueryHeaders(request.value, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                      WINHTTP_HEADER_NAME_BY_INDEX, &status, &size, WINHTTP_NO_HEADER_INDEX);
  if (status < 200 || status >= 300) {
    error = url + ": status code " + std::to_string(status);
    return false;
  }
  for (;;) {
    DWORD available = 0;
    if (!WinHttpQueryDataAvailable(request.value, &available)) {
      error = HresultMessage("WinHttpQueryDataAvailable", HRESULT_FROM_WIN32(GetLastError()));
      return false;
    }
    if (available == 0) return true;
    size_t offset = body.size();
    size_t take = std::min<size_t>(available, kMaxDownload - offset);
    if (take == 0) return true;
    body.resize(offset + take);
    DWORD read = 0;
    if (!WinHttpReadData(request.value, body.data() + offset, static_cast<DWORD>(take), &read)) {
      error = HresultMessage("WinHttpReadData", HRESULT_FROM_WIN32(GetLastError()));
      return false;
    }
    body.resize(offset + read);
  }
}

bool ReadFile(const std::string& path, std::vector<uint8_t>& body, std::string& error) {
  std::ifstream file(Wide(path), std::ios::binary);
  if (!file) {
    error = "cannot open " + path;
    return false;
  }
  body.assign(std::istreambuf_iterator<char>(file), {});
  return true;
}

std::shared_ptr<const Decoded> DecodeImage(const std::vector<uint8_t>& bytes, std::string& error) {
  struct Com {
    HRESULT hr = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    ~Com() {
      if (SUCCEEDED(hr)) CoUninitialize();
    }
  } com;
  ComPtr<IWICImagingFactory> factory;
  ComPtr<IWICStream> stream;
  ComPtr<IWICBitmapDecoder> decoder;
  ComPtr<IWICBitmapFrameDecode> frame;
  ComPtr<IWICFormatConverter> converter;
  HRESULT hr = CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER,
                                IID_PPV_ARGS(&factory));
  if (SUCCEEDED(hr)) hr = factory->CreateStream(&stream);
  if (SUCCEEDED(hr)) {
    hr = stream->InitializeFromMemory(const_cast<BYTE*>(bytes.data()), static_cast<DWORD>(bytes.size()));
  }
  if (SUCCEEDED(hr)) {
    hr = factory->CreateDecoderFromStream(stream.Get(), nullptr, WICDecodeMetadataCacheOnDemand,
                                          &decoder);
  }
  if (SUCCEEDED(hr)) hr = decoder->GetFrame(0, &frame);
  if (SUCCEEDED(hr)) hr = factory->CreateFormatConverter(&converter);
  if (SUCCEEDED(hr)) {
    hr = converter->Initialize(frame.Get(), GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone,
                               nullptr, 0.0, WICBitmapPaletteTypeCustom);
  }
  auto decoded = std::make_shared<Decoded>();
  if (SUCCEEDED(hr)) hr = converter->GetSize(&decoded->width, &decoded->height);
  if (SUCCEEDED(hr) && (decoded->width == 0 || decoded->height == 0)) hr = E_FAIL;
  if (SUCCEEDED(hr)) {
    decoded->pixels.resize(size_t{decoded->width} * decoded->height * 4);
    hr = converter->CopyPixels(nullptr, decoded->width * 4, static_cast<UINT>(decoded->pixels.size()),
                               decoded->pixels.data());
  }
  if (FAILED(hr)) {
    error = HresultMessage("image decode", hr);
    return nullptr;
  }
  return decoded;
}

class DecodeCache {
 public:
  std::shared_ptr<const Decoded> Get(const std::string& key) {
    std::lock_guard lock(mutex_);
    auto it = entries_.find(key);
    if (it == entries_.end()) return nullptr;
    order_.splice(order_.begin(), order_, it->second.second);
    return it->second.first;
  }

  void Put(const std::string& key, std::shared_ptr<const Decoded> image) {
    std::lock_guard lock(mutex_);
    if (auto it = entries_.find(key); it != entries_.end()) {
      order_.erase(it->second.second);
      entries_.erase(it);
    }
    order_.push_front(key);
    entries_.emplace(key, std::make_pair(std::move(image), order_.begin()));
    if (entries_.size() > kCacheCapacity) {
      entries_.erase(order_.back());
      order_.pop_back();
    }
  }

 private:
  std::mutex mutex_;
  std::list<std::string> order_;
  std::unordered_map<std::string, std::pair<std::shared_ptr<const Decoded>, std::list<std::string>::iterator>>
      entries_;
};

DecodeCache& Cache() {
  static DecodeCache cache;
  return cache;
}

Load LoadImage(const std::string& src, const std::string& headers) {
  std::string key = src + "\n" + headers;
  if (auto cached = Cache().Get(key)) return {cached, {}};
  std::vector<uint8_t> bytes;
  std::string error;
  bool ok;
  if (src.rfind("http://", 0) == 0 || src.rfind("https://", 0) == 0) {
    ok = Download(src, headers, bytes, error);
  } else if (src.rfind("file://", 0) == 0) {
    ok = ReadFile(src.substr(7), bytes, error);
  } else if (src.rfind("data:image/", 0) == 0) {
    auto comma = src.find(',');
    ok = comma != std::string::npos && src.compare(comma - 7, 7, ";base64") == 0;
    if (ok) {
      bytes = DecodeBase64(std::string_view(src).substr(comma + 1));
    } else {
      error = comma == std::string::npos ? "image data URL has no payload"
                                         : "only base64 image data URLs are supported";
    }
  } else {
    ok = ReadFile(src, bytes, error);
  }
  if (!ok) return {nullptr, error};
  auto decoded = DecodeImage(bytes, error);
  if (decoded) Cache().Put(key, decoded);
  return {decoded, error};
}

// Runs work on the thread that created it; the views' host services are UI-thread only.
class UiThread {
 public:
  static UiThread& Current() {
    thread_local UiThread instance;
    return instance;
  }

  // Callable from any thread for the lifetime of the process.
  void Post(std::function<void()> work) {
    auto boxed = new std::function<void()>(std::move(work));
    if (!PostMessageW(window_, kRun, 0, reinterpret_cast<LPARAM>(boxed))) delete boxed;
  }

 private:
  static constexpr UINT kRun = WM_APP + 1;

  UiThread() {
    WNDCLASSW window_class{};
    window_class.lpfnWndProc = [](HWND window, UINT message, WPARAM w, LPARAM l) -> LRESULT {
      if (message != kRun) return DefWindowProcW(window, message, w, l);
      std::unique_ptr<std::function<void()>> work(reinterpret_cast<std::function<void()>*>(l));
      (*work)();
      return 0;
    };
    window_class.hInstance = GetModuleHandleW(nullptr);
    window_class.lpszClassName = L"WhiskerImageDispatch";
    RegisterClassW(&window_class);
    window_ = CreateWindowExW(0, window_class.lpszClassName, L"", 0, 0, 0, 0, 0, HWND_MESSAGE, nullptr,
                              window_class.hInstance, nullptr);
  }

  HWND window_ = nullptr;
};

enum class Mode { AspectFill, AspectFit, ScaleToFill, Center };

whisker::Value Detail(double width, double height, std::string error) {
  return whisker::Value::Map{{"width", width}, {"height", height}, {"error", std::move(error)}};
}

class ImageView : public whisker::ModuleView {
 public:
  using ModuleView::ModuleView;

  ~ImageView() override {
    if (pending_) pending_->view = nullptr;
  }

  void SetSrc(std::string_view src) {
    src_ = src;
    Reload();
  }

  void SetHeaders(std::string_view headers) {
    headers_ = headers;
    if (!src_.empty()) Reload();
  }

  // Managed-cache requests load as ordinary images: this Host keeps no cache files or decode bounds.
  void SetCache(std::string_view cache) {
    if (cache == cache_) return;
    cache_ = cache;
    Reload();
  }

  void SetMode(std::string_view mode) {
    if (mode == "aspectFill") mode_ = Mode::AspectFill;
    if (mode == "aspectFit") mode_ = Mode::AspectFit;
    if (mode == "scaleToFill") mode_ = Mode::ScaleToFill;
    if (mode == "center") mode_ = Mode::Center;
  }

  void Draw(ID2D1DeviceContext* context, float width, float height) override {
    if (!image_) return;
    ComPtr<ID2D1Bitmap> bitmap;
    auto properties = D2D1::BitmapProperties(
        D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED));
    if (FAILED(context->CreateBitmap(D2D1::SizeU(image_->width, image_->height), image_->pixels.data(),
                                     image_->width * 4, properties, &bitmap))) {
      return;
    }
    float source_width = static_cast<float>(image_->width);
    float source_height = static_cast<float>(image_->height);
    float draw_width = width, draw_height = height;
    if (mode_ == Mode::AspectFill || mode_ == Mode::AspectFit) {
      float sx = width / source_width, sy = height / source_height;
      float scale = mode_ == Mode::AspectFill ? std::max(sx, sy) : std::min(sx, sy);
      draw_width = source_width * scale;
      draw_height = source_height * scale;
    } else if (mode_ == Mode::Center) {
      float dpi_x, dpi_y;
      context->GetDpi(&dpi_x, &dpi_y);
      draw_width = source_width * 96.0f / dpi_x;
      draw_height = source_height * 96.0f / dpi_y;
    }
    float x = (width - draw_width) / 2, y = (height - draw_height) / 2;
    context->PushAxisAlignedClip(D2D1::RectF(0, 0, width, height), D2D1_ANTIALIAS_MODE_ALIASED);
    context->DrawBitmap(bitmap.Get(), D2D1::RectF(x, y, x + draw_width, y + draw_height), 1.0f,
                        D2D1_INTERPOLATION_MODE_LINEAR, nullptr, nullptr);
    context->PopAxisAlignedClip();
  }

 private:
  // Cleared when the view is destroyed or reloads, so a late load result is dropped.
  struct Pending {
    ImageView* view;
  };

  void Reload() {
    image_.reset();
    if (pending_) pending_->view = nullptr;
    pending_.reset();
    Invalidate();
    std::string src = src_, headers = headers_;
    if (!cache_.empty()) {
      std::vector<std::pair<std::string, std::string>> request;
      if (!ParseObject(cache_, request, true)) {
        Emit("error", Detail(0, 0, "invalid image cache options"));
        return;
      }
      src.clear();
      for (auto const& [name, value] : request) {
        if (name == "_src") src = value;
        if (name == "_headers") headers = value;
      }
    }
    if (src.empty()) return;
    auto pending = std::make_shared<Pending>(Pending{this});
    pending_ = pending;
    auto& ui = UiThread::Current();
    std::thread([pending, src = std::move(src), headers = std::move(headers), &ui] {
      auto load = LoadImage(src, headers);
      ui.Post([pending, load = std::move(load)]() mutable {
        if (pending->view) pending->view->Finish(std::move(load));
      });
    }).detach();
  }

  void Finish(Load load) {
    pending_.reset();
    if (load.first) {
      image_ = std::move(load.first);
      Invalidate();
      Emit("load", Detail(image_->width, image_->height, ""));
    } else {
      Emit("error", Detail(0, 0, std::move(load.second)));
    }
  }

 private:
  std::string src_;
  std::string headers_;
  std::string cache_;
  Mode mode_ = Mode::AspectFill;
  std::shared_ptr<const Decoded> image_;
  std::shared_ptr<Pending> pending_;
};

class ImageModule : public whisker::Module {
 public:
  whisker::ModuleDefinition Definition() override {
    return whisker::ModuleDefinition()
        .Name("Image")
        .AsyncFunction("prefetch",
                       [](const whisker::Arguments& args, whisker::Promise promise) {
                         auto urls = args.empty() ? nullptr : args[0].AsArray();
                         auto headers = args.size() > 1 ? args[1].AsString() : std::nullopt;
                         if (!urls || !headers) {
                           promise.Reject("Image.prefetch requires an URL array and headers string");
                           return;
                         }
                         std::vector<std::string> sources;
                         for (auto const& url : *urls) {
                           if (auto text = url.AsString()) sources.emplace_back(*text);
                         }
                         std::thread([sources = std::move(sources), headers = std::string(*headers),
                                      promise] {
                           for (auto const& src : sources) LoadImage(src, headers);
                           promise.Resolve();
                         }).detach();
                       })
        .AsyncFunction("configureCache",
                       [](const whisker::Arguments&, whisker::Promise promise) {
                         promise.Reject("the Windows Host does not support the managed image cache");
                       })
        .AsyncFunction("removeCached",
                       [](const whisker::Arguments&, whisker::Promise promise) {
                         promise.Reject("the Windows Host does not support the managed image cache");
                       })
        .View<ImageView>("whisker-image:Image", [](auto& view) {
          view.Prop("src", [](ImageView& image, const whisker::Value& value) {
            image.SetSrc(value.AsString().value_or(""));
          });
          view.Prop("mode", [](ImageView& image, const whisker::Value& value) {
            image.SetMode(value.AsString().value_or("aspectFill"));
          });
          view.Prop("headers", [](ImageView& image, const whisker::Value& value) {
            image.SetHeaders(value.AsString().value_or(""));
          });
          view.Prop("cache", [](ImageView& image, const whisker::Value& value) {
            image.SetCache(value.AsString().value_or(""));
          });
          view.Events({"load", "error"});
        });
  }
};

}  // namespace

WHISKER_MODULE(ImageModule)
