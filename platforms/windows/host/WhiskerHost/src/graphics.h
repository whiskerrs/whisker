#pragma once

#include <windows.h>
#undef GetCurrentTime

#include <d2d1_1.h>
#include <winrt/Microsoft.UI.Composition.h>

namespace whisker {

// WinAppSDK ships these interop interfaces only as MIDL output of its winmd.
struct __declspec(uuid("2D6355C2-AD57-4EAE-92E4-4C3EFF65D578")) ICompositionDrawingSurfaceInterop
    : ::IUnknown {
  virtual HRESULT __stdcall BeginDraw(const RECT*, REFIID, void**, POINT*) = 0;
  virtual HRESULT __stdcall EndDraw() = 0;
  virtual HRESULT __stdcall Resize(SIZE) = 0;
  virtual HRESULT __stdcall Scroll(const RECT*, const RECT*, int, int) = 0;
  virtual HRESULT __stdcall ResumeDraw() = 0;
  virtual HRESULT __stdcall SuspendDraw() = 0;
};

struct __declspec(uuid("FAB19398-6D19-4D8A-B752-8F096C396069")) ICompositorInterop : ::IUnknown {
  virtual HRESULT __stdcall CreateGraphicsDevice(::IUnknown*, ::IUnknown**) = 0;
};

winrt::Microsoft::UI::Composition::CompositionGraphicsDevice CreateGraphicsDevice(
    winrt::Microsoft::UI::Composition::Compositor const& compositor);

// Calls `draw` with a context whose user space is logical pixels at `scale`.
template <typename Draw>
void DrawSurface(winrt::Microsoft::UI::Composition::CompositionDrawingSurface const& surface,
                 float scale, Draw&& draw) {
  auto surface_interop = surface.as<ICompositionDrawingSurfaceInterop>();
  winrt::com_ptr<ID2D1DeviceContext> context;
  POINT offset{};
  if (FAILED(surface_interop->BeginDraw(nullptr, __uuidof(ID2D1DeviceContext), context.put_void(),
                                        &offset))) {
    return;
  }
  context->SetDpi(96.0f * scale, 96.0f * scale);
  context->SetTransform(D2D1::Matrix3x2F::Translation(offset.x / scale, offset.y / scale));
  context->Clear(D2D1::ColorF(0, 0, 0, 0));
  draw(context.get());
  surface_interop->EndDraw();
}

}  // namespace whisker
