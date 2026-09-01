# Documentation ownership and maintenance

Whisker has three documentation layers. They serve different readers and must
not become independent descriptions of the same API.

## Where information belongs

| Layer | Audience | Owns | Does not own |
|---|---|---|---|
| `docs/` in this repository | Whisker contributors and Host/module maintainers | Architecture, invariants, crate boundaries, build and runtime internals, design rationale | Tutorials or exhaustive public API reference |
| `../website/src/content/docs/` | Application and module authors | Getting started, concepts, guides, supported platforms, task-oriented examples | Internal implementation details that users cannot rely on |
| Rustdoc in each crate | Rust callers of that crate | Exact public items, contracts, errors, safety, short compiling examples | Long tutorials or cross-platform product documentation |
| `docs/rfcs/` | Design history and accepted decisions | Why a change was proposed and what was accepted at that time | A guarantee that the described implementation is still current |

The implementation and its tests are the source of truth. Rustdoc is the
closest prose representation of the public Rust API; the website links concepts
to those exact names instead of duplicating full signatures. Current-design
documents explain seams and invariants that cannot be understood from one API
item. RFCs remain historical records and may therefore mention removed systems
such as Lynx.

## Required updates with a change

- A public Rust signature or behavior change updates its Rustdoc and any website
  example that uses it.
- A crate responsibility, Host boundary, ABI, lifecycle, or build-flow change
  updates the relevant current-design document under `docs/`.
- A user-visible workflow or platform-support change updates the website and the
  root README.
- An architectural decision that needs durable rationale gets an RFC. Once the
  implementation lands, the current-design docs must also be updated; changing
  only the RFC is insufficient.

## Rustdoc standard

Every publishable library crate has crate-level `//!` documentation that states:

1. the crate's responsibility;
2. who should depend on it directly;
3. its primary entry points;
4. important ownership, threading, or safety constraints.

Public application APIs document behavior and include small examples where that
clarifies usage. Internal plumbing may remain `#[doc(hidden)]`, but public docs
must not link to private items because those links break on docs.rs.

Validate the complete workspace with:

```sh
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --lib
cargo test --workspace --doc
```

The first command catches broken, ambiguous, and private intra-doc links. The
second compiles runnable examples. Use `ignore` only when an example requires a
Host or generated project; `ignore` blocks must still be valid Rust syntax.

The application-facing `whisker`, `whisker-css`, and `whisker-animation`
crates prevent regressions through `#![warn(missing_docs)]` plus the
warnings-as-errors Rustdoc build. `whisker-runtime` still exposes lower-level
extension plumbing that predates this policy; document and narrow that surface
incrementally before enabling the same lint there.

## Examples and terminology

- Use the public PascalCase element names (`View`, `Text`, `ScrollView`, `List`,
  `Fragment`).
- Use structured `css!` values; raw style strings are not supported.
- Describe `render!`, `css!`, and `routes!` as authoring adapters over public
  builders. The builder API is the semantic contract.
- Call Android, iOS, Web, and Desktop implementations **Hosts**. Android/iOS use
  the FFI Driver; Web/Desktop compose the Rust runtime directly.
- Do not describe Lynx as a current dependency. It may be named in RFC history
  or compatibility rationale, with past-tense wording.
- Copy non-trivial code from a checked example or test whenever possible. If a
  standalone snippet cannot be compiled in CI, identify the source file beside
  it so future updates have an implementation reference.

## Review checklist

- Does each statement live in the layer whose readers need it?
- Do commands match `whisker --help` and generated project behavior?
- Do examples use current element, style, module, and event APIs?
- Are platform differences stated explicitly rather than implied by a mobile-only
  example?
- Does Rustdoc build with warnings denied, and do website links resolve?
- Was historical migration language kept out of current-design and user docs?
