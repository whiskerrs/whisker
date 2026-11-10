#include "resources.h"

#include <wincrypt.h>

#include <algorithm>
#include <filesystem>
#include <fstream>
#include <string_view>

#include <winrt/Microsoft.UI.Dispatching.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Storage.Streams.h>
#include <winrt/Windows.Web.Http.h>

#include "text.h"

#pragma comment(lib, "crypt32.lib")
#pragma comment(lib, "windowscodecs.lib")

namespace whisker {

namespace {

std::string_view View(const WhiskerStringRef& value) {
  return {value.ptr ? value.ptr : "", value.len};
}

std::optional<std::vector<uint8_t>> Base64(std::string_view text) {
  DWORD size = 0;
  if (!CryptStringToBinaryA(text.data(), static_cast<DWORD>(text.size()), CRYPT_STRING_BASE64,
                            nullptr, &size, nullptr, nullptr)) {
    return std::nullopt;
  }
  std::vector<uint8_t> bytes(size);
  if (!CryptStringToBinaryA(text.data(), static_cast<DWORD>(text.size()), CRYPT_STRING_BASE64,
                            bytes.data(), &size, nullptr, nullptr)) {
    return std::nullopt;
  }
  bytes.resize(size);
  return bytes;
}

std::optional<std::vector<uint8_t>> DataUrl(std::string_view url) {
  auto comma = url.find(',');
  if (comma == std::string_view::npos) return std::nullopt;
  auto header = url.substr(5, comma - 5);
  if (header.size() < 7 || header.substr(header.size() - 7) != ";base64") return std::nullopt;
  return Base64(url.substr(comma + 1));
}

// Bundled assets resolve against the executable's directory, as iOS resolves them against the bundle.
std::filesystem::path AssetRoot() {
  wchar_t module[MAX_PATH];
  GetModuleFileNameW(nullptr, module, MAX_PATH);
  return std::filesystem::path(module).parent_path();
}

}  // namespace

ResourceService::ResourceService() : self_(std::make_shared<ResourceService*>(this)) {
  winrt::check_hresult(CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER,
                                        IID_PPV_ARGS(wic_.put())));
}

ResourceService::~ResourceService() { *self_ = nullptr; }

bool ResourceService::Apply(const WhiskerMobileResourceCommand& command) {
  if (command.resource == 0 || command.generation == 0) return false;
  if (command.command == WHISKER_RESOURCE_COMMAND_RELEASE) {
    return Release(command.resource, command.generation);
  }
  if (command.command != WHISKER_RESOURCE_COMMAND_LOAD ||
      command.kind != WHISKER_RESOURCE_RASTER_IMAGE) {
    return false;
  }
  return Load(command.resource, command.generation, command);
}

bool ResourceService::Load(uint64_t id, uint64_t generation,
                           const WhiskerMobileResourceCommand& command) {
  if (generation <= latest_[id]) return false;
  latest_[id] = generation;
  current_[id] = generation;
  Emit(id, generation, ResourceState::Loading);
  auto identifier = View(command.identifier);
  switch (command.source) {
    case WHISKER_RESOURCE_SOURCE_BYTES: {
      if (identifier.substr(0, 6) != "image/" || !command.data.ptr || command.data.len == 0) {
        Fail(id, generation, WHISKER_RESOURCE_FAILURE_UNSUPPORTED);
        return true;
      }
      Decode(id, generation, {command.data.ptr, command.data.ptr + command.data.len});
      return true;
    }
    case WHISKER_RESOURCE_SOURCE_URL: {
      if (identifier.substr(0, 5) == "data:") {
        auto bytes = DataUrl(identifier);
        if (!bytes) {
          Fail(id, generation, WHISKER_RESOURCE_FAILURE_DECODE);
        } else {
          Decode(id, generation, *bytes);
        }
        return true;
      }
      if (identifier.substr(0, 7) != "http://" && identifier.substr(0, 8) != "https://") {
        Fail(id, generation, WHISKER_RESOURCE_FAILURE_UNSUPPORTED);
        return true;
      }
      auto queue = winrt::Microsoft::UI::Dispatching::DispatcherQueue::GetForCurrentThread();
      if (!queue) {
        Fail(id, generation, WHISKER_RESOURCE_FAILURE_UNSUPPORTED);
        return true;
      }
      winrt::Windows::Web::Http::HttpClient client;
      auto request = client.GetBufferAsync(winrt::Windows::Foundation::Uri(Widen(command.identifier)));
      request.Completed([self = self_, queue, id, generation](auto const& operation, auto status) {
        std::optional<std::vector<uint8_t>> bytes;
        if (status == winrt::Windows::Foundation::AsyncStatus::Completed) {
          auto buffer = operation.GetResults();
          bytes.emplace(buffer.data(), buffer.data() + buffer.Length());
        }
        queue.TryEnqueue([self, id, generation, bytes = std::move(bytes)] {
          auto service = *self;
          if (!service || service->current_[id] != generation) return;
          if (!bytes || bytes->empty()) {
            service->Fail(id, generation, WHISKER_RESOURCE_FAILURE_NETWORK);
          } else {
            service->Decode(id, generation, *bytes);
          }
        });
      });
      return true;
    }
    case WHISKER_RESOURCE_SOURCE_BUNDLED_ASSET: {
      auto path = AssetRoot() / std::filesystem::path(Widen(command.identifier));
      std::ifstream file(path, std::ios::binary);
      if (identifier.empty() || !file) {
        Fail(id, generation, WHISKER_RESOURCE_FAILURE_NOT_FOUND);
        return true;
      }
      Decode(id, generation, {std::istreambuf_iterator<char>(file), {}});
      return true;
    }
    default:
      Fail(id, generation, WHISKER_RESOURCE_FAILURE_UNSUPPORTED);
      return true;
  }
}

bool ResourceService::Release(uint64_t id, uint64_t generation) {
  auto state = states_.find({id, generation});
  if (state == states_.end()) return false;
  if (current_[id] == generation) {
    current_.erase(id);
    rasters_.erase(id);
  }
  Emit(id, generation, ResourceState::Released);
  return true;
}

void ResourceService::Decode(uint64_t id, uint64_t generation, const std::vector<uint8_t>& bytes) {
  winrt::com_ptr<IWICStream> stream;
  winrt::com_ptr<IWICBitmapDecoder> decoder;
  winrt::com_ptr<IWICBitmapFrameDecode> frame;
  winrt::com_ptr<IWICFormatConverter> converter;
  UINT width = 0;
  UINT height = 0;
  bool decoded =
      SUCCEEDED(wic_->CreateStream(stream.put())) &&
      SUCCEEDED(stream->InitializeFromMemory(const_cast<uint8_t*>(bytes.data()),
                                             static_cast<DWORD>(bytes.size()))) &&
      SUCCEEDED(wic_->CreateDecoderFromStream(stream.get(), nullptr,
                                              WICDecodeMetadataCacheOnLoad, decoder.put())) &&
      SUCCEEDED(decoder->GetFrame(0, frame.put())) &&
      SUCCEEDED(wic_->CreateFormatConverter(converter.put())) &&
      SUCCEEDED(converter->Initialize(frame.get(), GUID_WICPixelFormat32bppPBGRA,
                                      WICBitmapDitherTypeNone, nullptr, 0,
                                      WICBitmapPaletteTypeMedianCut)) &&
      SUCCEEDED(converter->GetSize(&width, &height)) && width > 0 && height > 0;
  winrt::com_ptr<IWICBitmap> bitmap;
  if (decoded) {
    decoded = SUCCEEDED(wic_->CreateBitmapFromSource(converter.get(), WICBitmapCacheOnLoad,
                                                     bitmap.put()));
  }
  if (!decoded) {
    Fail(id, generation, WHISKER_RESOURCE_FAILURE_DECODE);
    return;
  }
  rasters_[id] = {bitmap.as<IWICBitmapSource>(), width, height};
  Emit(id, generation, ResourceState::Ready);
}

bool ResourceService::Register(uint64_t id, uint32_t width, uint32_t height, const uint8_t* rgba) {
  if (id == 0 || width == 0 || height == 0 || !rgba) return false;
  std::vector<uint8_t> bgra(size_t{width} * height * 4);
  for (size_t i = 0; i < bgra.size(); i += 4) {
    float alpha = rgba[i + 3] / 255.0f;
    bgra[i] = static_cast<uint8_t>(std::lround(rgba[i + 2] * alpha));
    bgra[i + 1] = static_cast<uint8_t>(std::lround(rgba[i + 1] * alpha));
    bgra[i + 2] = static_cast<uint8_t>(std::lround(rgba[i] * alpha));
    bgra[i + 3] = rgba[i + 3];
  }
  winrt::com_ptr<IWICBitmap> bitmap;
  if (FAILED(wic_->CreateBitmapFromMemory(width, height, GUID_WICPixelFormat32bppPBGRA, width * 4,
                                          static_cast<UINT>(bgra.size()), bgra.data(),
                                          bitmap.put()))) {
    return false;
  }
  rasters_[id] = {bitmap.as<IWICBitmapSource>(), width, height};
  return true;
}

std::optional<Raster> ResourceService::Find(uint64_t id) const {
  auto it = rasters_.find(id);
  if (it == rasters_.end()) return std::nullopt;
  return it->second;
}

std::optional<ResourceState> ResourceService::State(uint64_t id, uint64_t generation) const {
  auto it = states_.find({id, generation});
  if (it == states_.end()) return std::nullopt;
  return it->second;
}

void ResourceService::Fail(uint64_t id, uint64_t generation, uint32_t code) {
  if (current_[id] != generation) return;
  Emit(id, generation, ResourceState::Failed, code);
}

void ResourceService::Emit(uint64_t id, uint64_t generation, ResourceState state,
                           uint32_t failure) {
  states_[{id, generation}] = state;
  if (!sink_ || state == ResourceState::Loading || state == ResourceState::Released) return;
  WhiskerMobileResourceEvent event{};
  event.status = state == ResourceState::Ready ? WHISKER_RESOURCE_EVENT_READY
                                               : WHISKER_RESOURCE_EVENT_FAILED;
  event.failure_code = failure;
  event.resource = id;
  event.generation = generation;
  if (state == ResourceState::Ready) {
    auto const& raster = rasters_.at(id);
    event.width = static_cast<float>(raster.width);
    event.height = static_cast<float>(raster.height);
    event.scale = 1;
    event.dimensions_mask = WHISKER_RESOURCE_DIMENSIONS_PRESENT;
  }
  sink_(event);
}

}  // namespace whisker
