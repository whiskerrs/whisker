# Whisker Chat

A bring-your-own-key chat application built with Whisker's public APIs. Connect
OpenAI, DeepSeek, or an OpenAI-compatible endpoint directly from the app. There
is no application server, build-time credential, or bundled API key.

## Features

- Provider presets, secure key entry, model discovery, and manual model IDs.
- Compact conversation history with search, a per-conversation action dialog, and Undo after moving a conversation to Trash.
- Streamed answers, stop, regenerate below the latest answer, and previous-answer navigation.
- Command + Enter (Web/macOS) or Ctrl + Enter (Web/Windows/Linux) to send; Enter inserts a newline.
- Markdown headings, lists, quotes, inline styles, links, tables, and highlighted code blocks.
- Saved drafts and partial-answer recovery after an interrupted session.
- An edge-to-edge conversation list with a floating header, composer, and Latest control.
- Light and dark appearances, with a device-local preference and live theme switching.
- Mobile stack navigation and a collapsible desktop/browser conversation sidebar.

Markdown uses the public rich Text API for inline formatting, links, and text
selection. Tables use ordinary View composition with horizontal scrolling.
Embedded HTML is never executed. Attachments, agent tools, cloud sync, explicit
copy buttons, and conversation-navigation keyboard shortcuts are not included.

## Run

From the repository root, select a target with its platform toolchain installed:

```sh
cargo run -p whisker-cli --bin whisker -- run ios --manifest-path examples/chat/Cargo.toml
cargo run -p whisker-cli --bin whisker -- run android --manifest-path examples/chat/Cargo.toml
cargo run -p whisker-cli --bin whisker -- run web --manifest-path examples/chat/Cargo.toml
cargo run -p whisker-cli --bin whisker -- run desktop --manifest-path examples/chat/Cargo.toml
```

When no API key is available, connection setup opens before the chat, including
after restarting Desktop or Web without a saved key. Enter your API base URL and key, discover models
or enter a model ID, then select **Start chatting**. To edit a configured connection,
open **Settings → API key & provider**. Saving those changes returns to Settings;
**Back to chat** returns to the conversation. Model discovery calls
`GET /models`; generation uses streamed Chat Completions. Availability and usage charges are controlled by your provider.
Browser connections require the provider to permit cross-origin requests.

On Web and Desktop at widths of at least 768 logical pixels, **Chats** opens an
animated sidebar; **Hide sidebar** gives the conversation more room. The header
button fades and collapses with the sidebar animation. Below that width, **Chats**
opens the same left-sliding history screen used on iOS and Android. Narrowing an
open sidebar closes it, preserving the selected conversation and draft. Use a conversation’s **…** menu to rename it or move it to Trash. **Settings → Color theme** switches appearance without resetting the
conversation or draft. Dark is the initial default; the selected appearance is
restored on launch. Settings uses a section sidebar on wide screens and stacks
the navigation as horizontal section controls above the form on narrow screens.
Web and Desktop start with the sidebar closed. On iOS and Android,
**Chats** opens a separate history screen that slides in from the left through
`whisker-router`.

## Data and credentials

Conversation data is local JSON and is not encrypted. Moving a conversation to Trash retains it on the device. **Undo** restores the
last trashed conversation while the history panel remains open. Native keys use endpoint-scoped
Keychain/Keystore storage when **Remember key securely** is selected. Desktop
retains keys in memory only. Web defaults to session-only storage and offers an
explicit, unchecked browser-storage consent option in setup and connection settings.
Opting in saves the API key in localStorage without app-level encryption. Scripts
running on the site, extensions with site access, and people using the same browser
profile may be able to read it. Uncheck the option and save to remove the stored key
while continuing to use it for the current session. Clearing browser site data also
removes it. Browser storage retains only the current endpoint's key; saving another
connection replaces or removes that entry. Changing the endpoint clears the key
field and browser-storage consent. A retained key is never reused for another
endpoint. Keys are excluded from conversation storage and provider error messages.

One answer can run at a time. Generation belongs to the app Owner above the
router, so covering or switching a screen preserves the active request. Stop
always targets that request, including when another conversation is selected.
Requests are never automatically retried. Failed or interrupted answers remain
visible but are excluded from subsequent assistant context.

Drafts save after a short idle interval and on blur. Text updates are batched
approximately every 32 ms; partial answers are checkpointed approximately once
per second. Abrupt termination can lose updates since the last checkpoint.
Restored conversations open at the beginning; **Latest** moves to the end.

## Code organization

| Directory | Responsibility |
| --- | --- |
| `src/design` | Color, typography, spacing, dimensions, and shared style tokens |
| `src/api` | HTTP requests, deadlines, SSE framing, and safe error classification |
| `src/state` | Conversations, app-owned generation, cancellation, and restoration |
| `src/storage` | Versioned persistence and platform-specific credential storage |
| `src/hooks` | Composable screen state and actions using signals, effects, and resources |
| `src/ui` | Router routes, screens, message rendering, and small controls |
| `tests/simulator_api.rs` | Optional local HTTP fixture for platform verification |

`use_connection_form` groups connection fields, discovery, and validation.
`use_chat` groups draft autosave, tail following, and conversation actions.
These are ordinary Rust functions called within a component Owner. Session and
turn signals belong to the app Owner; temporary form signals belong to their
screen. Generation captures its endpoint, key, and context before spawning.

## Verification without an API key

```sh
cargo test -p whisker-chat
whisker fmt $(rg --files examples/chat -g '*.rs')
whisker fmt --check $(rg --files examples/chat -g '*.rs')
```

For a deterministic local streaming API:

```sh
cargo test -p whisker-chat --test simulator_api -- --ignored --nocapture
```

It serves `http://127.0.0.1:8787/v1`, accepts the non-secret key `test-key`, and
lists `test-model` plus two alternative model IDs for checking list layouts.
`disconnect` simulates an unfinished stream; `rate-limit` returns HTTP 429. Other prompts produce a streamed English response. The fixture
permits browser CORS and never contacts an external API. For Android Emulator,
run `adb reverse tcp:8787 tcp:8787` before connecting to this loopback endpoint.

Loopback HTTP is accepted only with Rust debug assertions; remote endpoints
require HTTPS. For a non-hot-patch iOS build against the local fixture:

```sh
CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true cargo run -p whisker-cli --bin whisker -- run ios --manifest-path examples/chat/Cargo.toml --no-hot-patch
```
