# GPUI の Desktop text input 実装調査

調査日: 2026-09-02

対象は `zed-industries/zed` の `main`、commit
[`97b1e64a177a2fe3c2803e323087b5c2fa6fff1e`](https://github.com/zed-industries/zed/tree/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e)
である。以下で「GPUI」は特記しない限りこの revision を指す。

## 結論

GPUI は Desktop の editable text をネイティブ `NSTextField` / Win32 Edit /
GTK entry として配置していない。テキスト、選択、caret、marked text はアプリ側の
状態として保持し、GPUI の text system と GPU scene で描画する。一方、OS の入力
システムとは、フォーカス中の描画要素が毎フレーム登録する `InputHandler` を境界に
接続する。

```text
OS text system
  macOS NSTextInputClient / Windows IMM32 / Wayland text-input-v3 / X11 XIM
        ↕ UTF-16 range、commit/preedit、caret bounds
PlatformInputHandler
        ↕
InputHandler / EntityInputHandler
        ↕
field/editor が所有する text・selection・marked range・layout snapshot
        ↓
GPUI text shaping + GPU drawing
```

この分割は Whisker Desktop に適している。推奨は、モバイルと同じ native control
を Desktop に埋め込むことではなく、`whisker-input` の公開 API は維持しつつ、
Desktop Host に「editable-text session」という専用境界を追加すること。ただし
Whisker が現在使う winit 0.30.13 の `Ime::{Preedit, Commit}` だけでは GPUI と同等の
replacement range、文書問い合わせ、point-to-index、macOS Accessibility Keyboard
連携を表現できない。winit 経路は段階的な最小実装には使えるが、完全な入力契約の
最終境界にはしない方がよい。

## 1. GPUI の共通入力境界

### `InputHandler`

中心は [`InputHandler`](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui/src/platform.rs#L1693-L1880)
trait である。コメント上も AppKit の `NSTextInputClient` を基礎にした API と明記され、
次の双方向操作を持つ。

| 分類 | メソッド | 意味 |
|---|---|---|
| 状態問い合わせ | `selected_text_range` | 選択範囲と向きを返す |
| 状態問い合わせ | `marked_text_range` | IME composition 範囲を返す |
| 文書問い合わせ | `text_for_range` | 指定範囲の文字列と調整済み範囲を返す |
| OS → editor | `replace_text_in_range` | commit/通常入力を置換として適用する |
| OS → editor | `replace_and_mark_text_in_range` | preedit を置換し marked 状態にする |
| OS → editor | `unmark_text` | composition を終了する |
| geometry | `bounds_for_range` | candidate window 用の画面上の範囲を返す |
| geometry | `character_index_for_point` | 画面座標を文字位置へ逆変換する |
| selection | `set_selected_text_range` | OS 側が動かした選択を反映する |
| policy | `accepts_text_input` | 現在入力可能か返す |
| policy | `prefers_ime_for_printable_keys` | keybinding より IME を先にするか返す |
| assistance | `text_input_configuration` | autocorrect 等の設定を返す |

OS 境界の offset は明示的に UTF-16 code unit である。
[`UTF16Selection`](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui/src/platform.rs#L1680-L1691)
は range と selection direction を分けて保持する。Rust の UTF-8 byte offset を直接
OSへ渡していない点が重要である。

`PlatformInputHandler` は `Box<dyn InputHandler>` と `AsyncWindowContext` を保持し、
platform callback から entity/window state を安全に更新する type erasure 層である。
委譲処理と candidate bounds の計算は
[`platform.rs`](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui/src/platform.rs#L1440-L1677)
にある。composition 中は marked range 内の現在の visual line の先頭、通常時は
selection head の bounds を候補ウィンドウ位置に使う。

### View/entity との接続

通常の view はより型付きの
[`EntityInputHandler`](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui/src/input.rs#L7-L124)
を実装する。`ElementInputHandler<V>` が entity 更新を `InputHandler` に変換する
canonical adapter である
([`input.rs`](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui/src/input.rs#L126-L286))。

入力ハンドラは常設の hidden text field ではない。要素の paint 中に
[`Window::handle_input`](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui/src/window.rs#L4848-L4874)
を呼び、対応する `FocusHandle` が focused の場合だけ次フレームの handler として
登録される。したがって focus、表示中の layout geometry、OS input session が同じ
rendered frame に結びつく。

## 2. 入力要素・Editor が持つ状態

### 最小 input example

公式の
[`crates/gpui/examples/input.rs`](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui/examples/input.rs#L35-L414)
は単一行 input の最小モデルであり、次を view state として持つ。

- `content`
- byte range の `selected_range` と `selection_reversed`
- `marked_range`
- geometry/hit-test 用の `last_layout` と `last_bounds`
- pointer drag selection 用の `is_selecting`
- `focus_handle`

OS境界では UTF-8 byte offset と UTF-16 offset を相互変換し、左右移動や削除では
Unicode grapheme boundary を使う。同 example の
[`EntityInputHandler` 実装](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui/examples/input.rs#L278-L403)
では、preedit を `marked_range` に置き、commit 時は marked range（なければ selection）
を置換する。

描画も明瞭である。text は `shape_line`、selection と caret は `PaintQuad`、marked text
は underline 付き `TextRun` として構築し、`paint` で scene に描く
([`TextElement::prepaint/paint`](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui/examples/input.rs#L405-L548))。
つまり native control の外観を透明化して重ねる方式ではなく、表示は GPUI 独自描画
である。

### Zed Editor

実製品の Editor も同じ trait を実装するが、状態管理はより強い。

- selection は editor の multi-buffer selection model が所有する。
- composition は `HighlightKey::InputComposition` の anchor ranges として保持される。
- preedit/commit は実バッファへの transaction として適用し、同じ
  `ime_transaction` に group する。
- multi-cursor では OS が見ている一つの replacement range を各 cursor に展開する。
- preedit 中は auto-close / auto-surround を一時的に止める。

根拠は
[`Editor` の `EntityInputHandler` 実装](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/editor/src/input.rs#L2764-L3099)
である。marked text を別の OS-owned shadow buffer に閉じ込めず editor state として
扱うため、描画、undo grouping、selection、複数 cursor と同じモデル上で整合させられる。

Editor element も paint 時に focused editor を `ElementInputHandler` として登録する
([`editor/src/element.rs`](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/editor/src/element.rs#L9514-L9530))。

## 3. OS ごとの接続

### macOS: `NSTextInputClient` を custom `NSView` に直接実装

GPUI の native view class は `NSTextInputClient` protocol を追加し、`hasMarkedText`、
`markedRange`、`selectedRange`、`firstRectForCharacterRange`、`insertText`、
`setMarkedText`、`unmarkText`、`attributedSubstringForProposedRange`、
`characterIndexForPoint` を登録する
([`gpui_macos/src/window.rs`](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui_macos/src/window.rs#L239-L303))。

各 Objective-C callback は `PlatformInputHandler` に委譲する。character range の
bounds は GPUI 座標から macOS screen 座標へ変換され、`insertText` は commit、
`setMarkedText` は preedit と local selection を渡す
([`window.rs`](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui_macos/src/window.rs#L3014-L3230))。

Apple の公式仕様も、独自 text view は `NSView` を subclass して
`NSTextInputClient` を実装できるとしており、この protocol を text input system と
正しく連携するための契約としている
([Apple: `NSTextInputClient`](https://developer.apple.com/documentation/appkit/nstextinputclient))。
Apple の Text Editing guide は first responder の client が input context に接続され、
`insertText` / `setMarkedText` / `doCommandBySelector` を受け、scroll 等で座標が変われば
`invalidateCharacterCoordinates` を通知する流れを説明している
([Apple: Text Editing](https://developer.apple.com/library/archive/documentation/TextFonts/Conceptual/CocoaTextArchitecture/TextEditing/TextEditing.html))。
GPUI の `update_ime_position` も `NSTextInputContext.invalidateCharacterCoordinates` を
呼ぶ
([source](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui_macos/src/window.rs#L1931-L1948))。

keybinding と IME の競合も platform adapter で調停する。composition 中、非 printing
key、または日本語等の composition-based input source で handler が希望する場合は
`NSTextInputContext.handleEvent` を先に呼び、処理できない key は
`doCommandBySelector` から GPUI key dispatch に戻す
([`handle_key_event`](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui_macos/src/window.rs#L2328-L2510))。

### Windows: TSF ではなく IMM32

現在の GPUI Windows backend は Text Services Framework の `ITextStoreACP` 等ではなく、
window procedure で `WM_CHAR`、`WM_IME_STARTCOMPOSITION`、
`WM_IME_COMPOSITION` を処理する IMM32 実装である
([message dispatch](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui_windows/src/events.rs#L143-L160))。

- 通常文字は `WM_CHAR` から `replace_text_in_range(None, text)` へ送る
  ([source](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui_windows/src/events.rs#L473-L481))。
- `WM_IME_COMPOSITION` の `GCS_RESULTSTR` を commit、`GCS_COMPSTR` を marked text、
  `GCS_CURSORPOS` を composition 内 caret として処理する
  ([source](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui_windows/src/events.rs#L725-L778))。
- `ImmGetCompositionStringW` で UTF-16 data を読む
  ([source](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui_windows/src/events.rs#L1658-L1709))。
- `ImmSetCompositionWindow` と `ImmSetCandidateWindow` へ caret bounds を渡す
  ([source](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui_windows/src/events.rs#L646-L693))。
- focused handler の `accepts_text_input` に応じて `ImmAssociateContextEx` で IME context
  を有効化/解除する
  ([source](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui_windows/src/events.rs#L695-L723))。

Microsoft の仕様でも、self-drawn composition を扱うアプリは
`WM_IME_COMPOSITION` の flags を調べ `ImmGetCompositionString` で状態を取得する
([`WM_IME_COMPOSITION`](https://learn.microsoft.com/en-us/windows/win32/intl/wm-ime-composition),
[`ImmGetCompositionStringW`](https://learn.microsoft.com/en-us/windows/win32/api/imm/nf-imm-immgetcompositionstringw))。
候補/comp window の位置 API も GPUI の利用法と一致する
([`ImmSetCandidateWindow`](https://learn.microsoft.com/en-us/windows/win32/api/imm/nf-imm-immsetcandidatewindow),
[`ImmSetCompositionWindow`](https://learn.microsoft.com/en-us/windows/win32/api/imm/nf-imm-immsetcompositionwindow))。

### Wayland: `zwp_text_input_v3`

Wayland backend は `zwp_text_input_manager_v3` から seat ごとの
`zwp_text_input_v3` を取得する
([source](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui_linux/src/linux/wayland/client.rs#L1677-L1706))。
focus/acceptance に応じて enable/disable し、cursor rectangle を protocol に設定する
([source](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui_linux/src/linux/wayland/client.rs#L535-L590))。

イベントは `PreeditString` を一旦保持し、同じ transaction の `Done` で
`SetMarkedText`、`CommitString` で commit を handler に送る
([source](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui_linux/src/linux/wayland/client.rs#L1980-L2057),
[`ImeInput` adapter](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui_linux/src/linux/wayland/window.rs#L1413-L1451))。

注意点として、protocol が持つ `set_surrounding_text` と
`delete_surrounding_text` は現在の GPUI code path では使われず、event match の残りは
無視される。したがって Wayland backend は共通 `InputHandler` の全文書問い合わせ能力
を使い切っていない。protocol 自体の要求・イベント一覧は Wayland Protocols の
一次仕様にある
([`text-input-unstable-v3.xml`](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/afb614d5fcbd02d261a6ae91920aa91cf3915a8a/unstable/text-input/text-input-unstable-v3.xml))。

### X11: XIM callback style

X11 backend は Zed の `xim-rs` を通じて XIM input context を作り、
`PREEDIT_CALLBACKS`、client/focus window を設定する
([`xim_handler.rs`](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui_linux/src/linux/x11/xim_handler.rs#L31-L137))。
XIM の commit callback は `replace_text_in_range`、preedit draw callback は
`replace_and_mark_text_in_range` に変換される
([client callback routing](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui_linux/src/linux/x11/client.rs#L1427-L1509),
[window adapter](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui_linux/src/linux/x11/window.rs#L1212-L1265))。

candidate spot は input handler の selection bounds から `SpotLocation` として XIC に
設定する。XIM feedback と callback の caret/change offsets は現状捨て、preedit 全体を
再設定している。X.Org の Xlib specification でも `XIMPreeditCallbacks` は
preedit start/done/draw/caret callbacks を client が提供する style と定義されている
([Xlib specification, chapter 13](https://www.x.org/releases/current/doc/libX11/libX11/libX11.html))。

## 4. focus、clipboard、accessibility

### focus

`FocusHandle` と render dispatch tree が論理 focus を所有し、入力 element は
`track_focus` する。`handle_input` が focused handle のみを platform に公開するため、
複数 field が同時に OS input target になることはない
([`FocusHandle`](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui/src/window.rs#L528-L613),
[`handle_input`](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui/src/window.rs#L4848-L4874))。

### clipboard

Clipboard は editable-text trait の隠れた責務ではなく `Platform` service である。
`read_from_clipboard` / `write_to_clipboard` と Linux primary selection が共通 trait に
定義される
([`Platform` clipboard API](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui/src/platform.rs#L300-L328))。
input example は copy/cut/paste action からこの service を使う
([source](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui/examples/input.rs#L137-L180))。
`InputHandler::paste` には platform 主導 paste 用の default があり、plain text を現在の
selection に挿入する。

### accessibility

GPUI 自体は AccessKit tree と action routing を提供し、macOS、Windows、Linux の
platform adapter に渡す
([GPUI accessibility guide](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui/src/_accessibility.rs),
[`Window` initialization](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui/src/window.rs#L1447-L1504))。
guide は custom text editor を `Role::TextInput` と synthetic `TextRun`、
`TextSelection` で表現する例も示す
([source](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui/src/_accessibility.rs#L227-L269))。

ただし text input と accessibility は同じ trait ではない。公式の最小 input example
は text-input role/tree を実装しておらず、`InputHandler` を実装しただけで accessible
text field が自動生成されるわけではない。Whisker でも IME 対応完了と accessibility
対応完了を同一視してはいけない。

## 5. GPUI の build/composition 形状

GPUI の共通 trait/state は `gpui`、OS接続は `gpui_macos`、`gpui_windows`、
`gpui_linux` に分離される。`gpui_platform::current_platform` が target `cfg` に応じて
一つを構築する
([`gpui_platform.rs`](https://github.com/zed-industries/zed/blob/97b1e64a177a2fe3c2803e323087b5c2fa6fff1e/crates/gpui_platform/src/gpui_platform.rs#L44-L70))。
すべて最終的には単一 Rust executable に link され、OS object と共通 view state の間に
JSON/FFI bridge はない。

これは Whisker の `whisker-desktop` と薄い `whisker-macos` / `whisker-windows` /
`whisker-linux` という構成に近い。GPUIから借りるべきなのは個別の editor 実装ではなく、
「共通の豊かな editable-text contract」と「OS別 adapter」を明確に分離する形である。

## 6. winit 0.30.13 との差

Whisker Desktop は winit 0.30.13 を使用している。一方、現在の
[`DesktopApplication::window_event`](../platforms/desktop/src/app.rs)
は resize、pointer、wheel、touch 等だけを処理し、`WindowEvent::KeyboardInput`、
`WindowEvent::Ime`、`Focused` はまだ処理していない。

winit の公開 API には以下がある。

- `WindowEvent::Ime(Ime)` を明示的に有効化する `Window::set_ime_allowed`。
- `Ime::Preedit(String, Option<(byte_start, byte_end)>)` と `Ime::Commit(String)`。
- candidate box のための `Window::set_ime_cursor_area`。

仕様は
[`Ime`](https://github.com/rust-windowing/winit/blob/v0.30.13/src/event.rs#L774-L807) と
[`Window::set_ime_cursor_area` / `set_ime_allowed`](https://github.com/rust-windowing/winit/blob/v0.30.13/src/window.rs#L1211-L1286)
にある。これだけなら GPUI の Linux/Windows に近い基本的な preedit/commit 表示は
共通コードで開始できる。

しかし winit event は文書 range の query/replace protocol ではない。特に macOS の
winit 0.30.13 `NSTextInputClient` 実装は次の制限をコード上で明記している。

- `selectedRange` は常に `NSNotFound`。
- `setMarkedText` と `insertText` の `replacementRange` を無視する。
- `attributedSubstringForProposedRange` は `None`。
- `characterIndexForPoint` は `0`。
- `firstRectForCharacterRange` はアプリが設定した一つの IME area を返す。

根拠は winit の
[`WinitView: NSTextInputClient`](https://github.com/rust-windowing/winit/blob/v0.30.13/src/platform_impl/macos/view.rs#L243-L410)
である。したがって winit の `Ime` event だけでは、GPUI が可能にしている macOS の
replacement range、周辺文字列問い合わせ、point-to-character、system-driven selection
を Whisker state に反映できない。

## 7. Whisker Desktop への推奨境界

### 推奨: native control ではなく Host-owned editable-text session

`packages/whisker-input` の app-facing schema (`value`/`text`, `on_input`, `on_change`,
`on_focus`, `on_blur`, `on_submit`, `focus`/`blur`/`clear`/`setValue`) は維持する。
Desktop 実装は次の責務で分けるのがよい。

1. `whisker-input` Desktop module
   - schema property/command/event ID を binding する。
   - node ごとの `EditableTextState` を作る。
2. `whisker-desktop` 共通 editable-text layer
   - text、selection direction、marked range、scroll offset、focus、revision を保持する。
   - UTF-8/UTF-16/grapheme の変換を一箇所に集約する。
   - glyphon/cosmic-text の layout snapshot から caret/range bounds と hit-test を返す。
   - selection、caret、composition underline、placeholder、secure mask を scene として描く。
3. OS adapter
   - macOS: GPUI と同種の `NSTextInputClient` bridge。
   - Windows: まず IMM32。将来 TSF が必要なら同じ共通 contract の別 adapter とする。
   - Wayland: text-input-v3。`set_surrounding_text` と
     `delete_surrounding_text` も共通 contract に接続する。
   - X11: XIM callback/spot-location adapter。

現在の [`DesktopNativeElement`](../platforms/desktop/src/element.rs) は property、command、
raster、measurement、scroll の境界であり、window event、focus、IME session、layout
range query を受け取れない。入力を単なる `DesktopNativeElement::rasterize` 実装に
押し込めるのではなく、scene/window loop と協調する専用 capability を追加すべきである。
例えば `DesktopNativeElement` から opt-in の `editable_text()` handle を取得し、
`DesktopApplication` が focused node の handle だけを OS adapter に公開する形なら、
既存 module composition と両立する。

### 最小 trait の候補

GPUIをそのままコピーする必要はないが、少なくとも次の意味を Host 内部契約に持たせる。

```rust,ignore
trait DesktopEditableText {
    fn selected_range_utf16(&self) -> DirectedRange;
    fn marked_range_utf16(&self) -> Option<Range<usize>>;
    fn text_utf16(&self, range: Range<usize>) -> Option<AdjustedText>;

    fn commit(&mut self, replacement: Option<Range<usize>>, text: &str);
    fn set_preedit(
        &mut self,
        replacement: Option<Range<usize>>,
        text: &str,
        selection_in_preedit: Option<Range<usize>>,
    );
    fn unmark(&mut self);
    fn set_selection_utf16(&mut self, range: DirectedRange);

    fn bounds_for_range(&self, range: Range<usize>) -> Option<LogicalRect>;
    fn index_utf16_for_point(&self, point: LogicalPoint) -> Option<usize>;
    fn configuration(&self) -> TextInputConfiguration;
}
```

この trait は public component API ではなく Host 内部の OS/editor seam とする。
Whisker の既存 RFC が要求している `host_state_revision` と stale controlled-value の
拒否もこの state machine に統合する。composition 中の外部 `value` 更新を単純代入すると
marked range と caret を壊すためである。

### 実装順序

1. winit の `Ime::Preedit/Commit`、`set_ime_allowed`、`set_ime_cursor_area` で
   GPU-drawn single-line input の縦断 prototype を作る。
2. focus、selection、grapheme navigation、clipboard、secure、multiline、scroll を
   Host-owned state として追加する。
3. composition と controlled-value revision の conformance tests を追加する。
4. macOS の replacement/document-query 要件を満たす native bridge を追加する。
5. Wayland surrounding-text/delete、Windows IMM32、X11 XIM の差を OS adapter tests で
   固定する。
6. AccessKit に `TextInput`/`TextRun`/selection/actions を別 capability として公開する。

最初から native child control を合成しない判断には利点がある。Whisker の Taffy layout、
clip/transform/opacity、GPU paint order と自然に一致し、mobile の wrapper/native-view
制約を Desktop へ持ち込まずに済む。一方で native control が無料で提供する selection、
caret、IME、clipboard、accessibility、password semantics をすべて Host が実装する責任も
引き受ける。GPUI の設計はこの選択が実用可能であることを示すが、最小 input example
だけを移植すれば完成するわけではない。

## 8. 判断表

| 方針 | 長所 | 制約 | 推奨 |
|---|---|---|---|
| Desktop native text control を子Viewとして合成 | OS標準機能が多い | GPU scene のclip/transform/z-order、3 OS の native embedding、見た目の一致が難しい | mobile には継続、Desktop の第一候補にはしない |
| winit `Ime` event + GPU描画 | 現行 Host に最短、3 OS 共通の入口 | replacement/document query/selection/a11y が不足 | prototype と段階1に採用 |
| GPUI型の rich contract + OS adapter + GPU描画 | state/layout/revisionをWhiskerで統一、将来の完全対応が可能 | 実装量とOS別検証が必要 | Desktop の目標設計 |

最終提案は「winit で小さく開始し、公開 component と Host 内部 contract は最初から
GPUI 相当の情報量を持たせる」である。これなら最初の Desktop 対応を阻害せず、winit
の抽象化で失われる情報を後から OS adapter で補っても app-facing API を変更せずに済む。
