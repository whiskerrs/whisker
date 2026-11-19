#include "graphics.h"

#pragma comment(lib, "d2d1.lib")
#pragma comment(lib, "d3d11.lib")
#pragma comment(lib, "dxgi.lib")

namespace whisker {

namespace comp = winrt::Microsoft::UI::Composition;

comp::CompositionGraphicsDevice CreateGraphicsDevice(comp::Compositor const& compositor,
                                                     winrt::com_ptr<ID3D11Device>* d3d_out) {
  winrt::com_ptr<ID3D11Device> d3d;
  for (auto driver : {D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP}) {
    if (SUCCEEDED(D3D11CreateDevice(nullptr, driver, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                                    nullptr, 0, D3D11_SDK_VERSION, d3d.put(), nullptr, nullptr))) {
      break;
    }
  }
  winrt::com_ptr<ID2D1Factory1> factory;
  winrt::check_hresult(D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, factory.put()));
  winrt::com_ptr<ID2D1Device> d2d;
  winrt::check_hresult(factory->CreateDevice(d3d.as<IDXGIDevice>().get(), d2d.put()));
  winrt::com_ptr<::IUnknown> device;
  winrt::check_hresult(
      compositor.as<ICompositorInterop>()->CreateGraphicsDevice(d2d.get(), device.put()));
  if (d3d_out) *d3d_out = d3d;
  return device.as<comp::CompositionGraphicsDevice>();
}

}  // namespace whisker
