# Windows Host

WinUI 3 Host that drives a Whisker app through the mobile C ABI
(`crates/whisker-driver-sys/bridge/include/whisker_mobile.h`).

- `WhiskerHost/` — static library; `whisker::WhiskerView` presents Rust frames
  as Composition visuals and measures/draws text with DirectWrite, and
  `whisker_windows_run` opens a window around it. Packed as the
  `Whisker.WindowsHost` NuGet package (`nuget/`) that the `whisker-windows`
  crate restores and links.
- `WhiskerHostConformance/` — C entry points that present into a detached
  Composition scene and capture it for `conformance/`, the shared Host
  conformance runner (`cargo xtask host-conformance windows`).
- `WhiskerApp/` — unpackaged development app that links `WhiskerHost` and a
  Rust staticlib (`/p:WhiskerRustLib=...`, default: `whisker_news.lib`).

```powershell
cargo rustc --manifest-path examples/news/Cargo.toml --lib --crate-type staticlib --release --target x86_64-pc-windows-msvc
cd platforms/windows/host
nuget restore packages.config -PackagesDirectory packages
msbuild WhiskerHost.sln /p:Configuration=Release /p:Platform=x64
```

`cargo xtask windows-host-pack <version>` packs `target\nuget\Whisker.WindowsHost.<version>.nupkg`.
Point `WHISKER_WINDOWS_HOST_FEED` at that directory to build apps against an unpublished package.

The app needs the Windows App Runtime 2.6 installed (the MSIX files under
`packages/Microsoft.WindowsAppSDK.Runtime.*/tools/MSIX`).
