# Whisker RFCs

RFCs describe substantial changes to Whisker before they become the
implemented architecture. They are reviewed as pull requests and retain the
reasoning behind an accepted decision.

The rest of [`docs/`](../README.md) describes the current implementation. An
accepted RFC is therefore not evidence that a feature is already available.
When an RFC is implemented, update the current-design documents in the same
change and mark the RFC `Implemented`.

## Status

An RFC has one of these states:

- **Draft** — open for design review; no compatibility commitment.
- **Accepted** — the design is approved, but may not be implemented yet.
- **Implemented** — the current implementation and documentation conform to
  the RFC.
- **Rejected** — considered and deliberately not adopted.
- **Superseded** — replaced by another RFC, which the header must link.

The normal progression is:

```text
Draft -> Accepted -> Implemented
   \--------> Rejected
Implemented -> Superseded
```

## Process

1. Use a GitHub Discussion to develop the problem statement and collect broad
   feedback when useful.
2. Open an RFC pull request containing a numbered Markdown file. The pull
   request is the place for line-by-line design review.
3. Record the discussion and tracking issue in the RFC header.
4. Merge as `Draft` while material questions remain, or as `Accepted` when the
   maintainers have made the decision.
5. Track implementation in issues and ordinary code pull requests.

RFC numbers identify decisions, not releases. Interface and protocol versions
are specified independently inside each RFC.

## Index

| RFC | Title | Status |
|---|---|---|
| [0001](0001-runtime-modules-and-build-plugins.md) | Runtime Modules and Build-time Plugins | Draft |
| [0002](0002-renderer-interface-and-frame-protocol.md) | Renderer Interface and Frame Protocol | Draft |
| [0003](0003-typed-inline-style-layout-and-paint.md) | Typed Inline Style, Layout, and Paint | Draft |
| [0004](0004-native-modules-and-host-elements.md) | Native Modules and Host Elements | Draft |
