# Whisker Chat

A native, bring-your-own-key chat application built with Whisker's public APIs.
The first runnable milestone targets iOS. The application calls OpenAI,
DeepSeek, or an OpenAI-compatible endpoint directly; no application server or
build-time API key is required.

## First milestone

- Connection settings, secure key entry, model discovery, and manual model IDs.
- One persistent conversation with a draft and independently updated turns.
- Streamed text, cancellation, manual retry, and partial-answer recovery.
- Keychain/Keystore storage on iOS/Android; explicit session-only storage elsewhere.
- A fixed header and composer, a virtualized message list, and optional tail following.

Restored conversations initially open at the beginning; use the latest-message
button to move to the end. New requests enable tail following.

Responses are currently rendered as plain text. Multiple conversations,
Markdown, answer alternatives, desktop shortcuts, responsive navigation, and
desktop credential-store integration belong to subsequent milestones. Other
platforms are not yet runtime-validated.

## Run on iOS

From the repository root, with Xcode and an iOS Simulator installed:

```sh
cargo run -p whisker-cli --bin whisker -- run ios --manifest-path examples/chat/Cargo.toml
```

Enter an API base URL, your own API key, and a model ID in the application.
OpenAI and DeepSeek presets fill the base URL. Model discovery uses `GET /models`
and does not generate a billable answer. Generation uses streamed Chat
Completions; model availability and usage charges are controlled by the provider.

API keys never enter the conversation store. Changing the base URL clears the
key field. Native keys are scoped to the normalized endpoint in the OS secure
store. Web and Desktop currently retain keys in memory only. Conversation data
is local and is not encrypted. Failed or interrupted answers remain visible but
are excluded from subsequent assistant context. Generation requests are never
automatically retried.

## Layout of the application

| Directory | Responsibility |
| --- | --- |
| `src/api` | HTTP requests, deadlines, SSE framing, and safe error classification |
| `src/state` | Conversation data, app-owned generation, cancellation, and restore |
| `src/storage` | Versioned persistence and platform-specific credential storage |
| `src/ui` | Screens, small controls, and shared styling |
| `tests/simulator_api.rs` | Optional local HTTP fixture for simulator verification |

The app Owner owns generation tasks. Screen disposal does not cancel an active
answer. Generation IDs reject late updates, while an abort handle drops the
request when the user stops. Text updates are batched approximately every 32 ms;
partial answers are checkpointed approximately once per second.

## Verification without an API key

```sh
cargo test -p whisker-chat
whisker fmt $(rg --files examples/chat -g '*.rs')
whisker fmt --check $(rg --files examples/chat -g '*.rs')
```

To run a deterministic local API for simulator testing:

```sh
cargo test -p whisker-chat --test simulator_api -- --ignored --nocapture
```

It serves `http://127.0.0.1:8787/v1`, accepts the non-secret key `test-key`, and
lists `test-model`. The `disconnect` and `rate-limit` prompts simulate incomplete
responses and HTTP 429. Other prompts produce a streamed Japanese response.
Loopback HTTP is accepted only when Rust debug assertions are enabled; remote
endpoints always require HTTPS. This fixture never contacts an external API.

For the local fixture with a non-hot-patch iOS build, enable debug assertions:

```sh
CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true cargo run -p whisker-cli --bin whisker -- run ios --manifest-path examples/chat/Cargo.toml --no-hot-patch
```
