# Managed image caching

`whisker-image::Image` accepts optional reactive `ImageCacheOptions`. Ordinary
images retain their platform loader policy. Opt-in images use an exact URL key,
a temporary original file named by SHA-256, and a shared decoded-image LRU.
HTTP headers are forwarded on a miss but never participate in the key.

The public decode-bound and control API lives in [Rustdoc](../packages/whisker-image/src/lib.rs).
Bounds determine the retained pixel dimensions independently of layout and fit.

## Platforms

| Host | Implementation | Decoded cost |
|---|---|---|
| Android | `ManagedImageCache.kt`; `BitmapFactory` sample-size decode, then scale | bitmap allocation |
| iOS, macOS | `ManagedImageCache.swift`; ImageIO thumbnail decode | row stride × height |
| Linux | `whisker_image::loader` (`loader` feature); decode, then resize | RGBA length |
| Windows | none; requests load as ordinary images | — |
| Web | none; the browser cache applies | — |

Android and Apple decoders downsample while decoding. The Rust loader used by
the Linux Host decodes the full source and resizes before retaining the result.

## Memory

Each URL has separate decoded variants for its requested bounds. All variants
share a 64 MiB default budget based on the decoded cost above. Hits refresh
recency. Insertion and budget reduction evict least recently used entries.
Oversized images are not retained; zero disables retention. Visible image
references, GPU surfaces and transient decode allocations are not part of the
budget. Decoders may need full-source memory temporarily.

## Files

Originals live in an application temporary directory and have no expiration:
the app cache directory on Android, the app temporary directory on Apple
platforms, and the OS temporary directory namespaced by the executable path on
Linux. A file survives memory eviction and restarts until the OS removes it.
Downloads larger than 64 MiB and HTTP/decode failures are not committed; a
corrupt saved file is removed and retried once. Await
`remove_cached_image(url)` to delete the original and all memory variants.
Visible images remain visible after removal.

Each platform serializes loads, budget changes and removals on one cache worker
off the UI thread, so they apply in request order. On Linux, a queued load whose
view has since changed source or unmounted is skipped when it reaches the worker.

## Host request encoding

The internal `cache` string attribute contains the options plus `_src` and
`_headers`. A single attribute carries one coherent request independently of
Host property application order. Ordinary source/header attributes are empty
for managed images, preventing a second unmanaged load or cache.

## Unsupported Hosts

Web accepts the component but delegates to the browser cache. Windows reads
`_src` and `_headers` from the request and loads them through its ordinary
WinHTTP/WIC path and decode cache. Neither persists files, enforces decode
bounds or applies the byte budget. Configuration and removal methods return
errors on both, and Web custom request headers retain the existing
browser-host restriction.
