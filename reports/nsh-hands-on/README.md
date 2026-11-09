# NSH vs Whisker — 同じ Todo アプリを作って比較 (2026-11-07)

同じ仕様の Todo アプリ（入力して追加・完了切替・削除・All/Active/Done フィルタ・残り件数）を
NSH 0.4.0（Rust 版と TSX 版）と Whisker 0.15.5（Rust）で作り、iPhone 17 Pro シミュレータ（iOS 26.5）で動かした。
ホストは Apple Silicon Mac、Xcode 26.6。ソースは [apps/](apps/)、スクリーンショットは [shots/](shots/)。

| | NSH Rust | NSH TSX | Whisker |
|---|---|---|---|
| ソース行数 | 181 | 99 | 208 |
| 初回で動いたか | 動かず（crates.io の不具合を回避して起動） | 動いた（自分の JS バグを1件修正） | ほぼ動いた（Input が潰れていて高さ指定が必要だった） |
| 初回の起動まで | コールドは未計測（初回はクレート 1分26秒 + ランナーを数分ビルドした後に失敗）。パッチ後の再実行は 28 秒（キャッシュあり） | 12 秒（Rust ビルドなし） | 141 秒（コンパイル 25 秒 + xcodebuild 1分14秒。xcodebuild はプロジェクト初回のみの費用） |
| 編集が反映されるまで | パッケージ完了まで 4.7 秒（その後に再起動） | CLI の送信完了まで約 0.1 秒（端末側の適用時間は未計測） | 端末の適用応答まで 1.0〜1.2 秒（subsecond パッチ、1.3 MiB） |
| 編集後の状態 | すべて消える | 編集していない親の状態は残る | 編集していない親の状態は残る |
| debug 版 .app のサイズ | 42 MB | —（共通の dev host を使用） | 30 MB（うち WhiskerDriver.framework が 27 MB） |
| CLI | brew で 1.3 GB（ビルド済みホストを同梱） | 同左 | cargo install で 19 MB |

サイズはどちらも debug ビルドの参考値。release ビルドでは比べていない。

## 起きたこと

### NSH

- **Rust 1.96 では `nsh-core` 0.4.0 がビルドできない**（不安定機能 `isolate_most_least_significant_one` を使っている）。
  `rust-version` の指定がないので、cargo のエラーで初めて分かる。1.98.1 に固定して回避した。
- **crates.io の `nsh-platform-ios` 0.4.0 は単体ではビルドできない**。
  `#[path = "../../nsh-platform-macos/src/speech.rs"]` がモノレポの相対パスのまま公開されている（longbridge/nsh#940、main で修正済み・未リリース）。
  Rust アプリを iOS で動かす経路は、リリース版では壊れている。
  `speech.rs` を補ったローカルコピーを `.cargo/config.toml` の `[patch.crates-io]` で差し込んで回避した。
  TSX 版は CLI 同梱のビルド済み dev host を使うので、この不具合には当たらなかった。
- Rust 版のコードは**最初のビルドで型エラーなし**。Tailwind 名のメソッド（`px_4().rounded_full().gap_2()`）と `each` / `computed` で迷わなかった。
  ただし `line_through()` には条件を渡せないので、完了した行は色で表した。
- **Rust 版の dev ループは再起動**。見出しを変えるとパッケージ完了まで 4.7 秒で、その後に再起動する。フィルタも完了状態も初期値に戻る（[shots/nsh-after-edit-state-reset.png](shots/nsh-after-edit-state-reset.png)）。
- **TSX 版のホットアップデートは速い**。保存から CLI が送信を終えるまで約 0.1 秒（ビルドは 2〜3ms）。目で見ても待ちは感じなかった。
  子コンポーネントの `FilterTab` を編集しても、親が持つフィルタの状態は残った（[shots/nsh-tsx-hot-update-state-kept.png](shots/nsh-tsx-hot-update-state-kept.png)）。
- TSX で `set={filter.set}` とメソッドをそのまま渡すと `this` が外れ、タップ時に `Error: not a live signal` が出た。
  原因は私のコード。エラーには `App.tsx:42:32` のソース位置が付いていて、すぐ特定できた。
- `nsh check`（TypeScript 7 のネイティブ tsc）は 0.8 秒。`<Each key="id">` が型エラーになり、関数で渡す必要があった。

### Whisker

- **`whisker_input::Input` に高さを指定しないと 0 に潰れる**。パディングだけの細い帯になり、プレースホルダーも出ず、タップしてもフォーカスしない。
  エラーも警告も出ない。`height(px(44))` を足すと直った。
  原因は設計にある。`measurement.rs` は `auto_size: true` のときだけ内容を測り、それ以外は測らない。
  クレートのドキュメントの例は `height: px(44)` を指定しているが、高さの指定が必須だとはどこにも書いていない。
  NSH の `input()` は内在的な高さを持つので、何も指定しなくても使えた。
  Whisker 側では、少なくとも高さ 0 での無言の潰れを警告にすべき。
- コンパイルエラーは1件だけ（`Css::border_width` がなく、`Border::new().width(..)` を使う）。
- `whisker-input` / `whisker-safe-area` は 0.14.3 のまま、本体は 0.15.5。依存関係としては互換だが、バージョンが揃っていないので一瞬迷う。
- **ホットリロードは Rust のまま状態を保てる**。
  子コンポーネントの `TodoRow` を編集したとき、保存から反映まで 1.03 秒。Active フィルタと完了状態はそのまま残った（[shots/whisker-hot-reload-state-kept.png](shots/whisker-hot-reload-state-kept.png)）。
  状態を持つ `todo_app` 自身を編集した場合は、その状態がリセットされる（仕様どおり）。
- 日本語 IME で変換中に Add を押した場合、Whisker は入力欄を空にした。NSH は確定した文字列が欄に残った。
  シミュレータのローマ字入力を経由したので、入力内容自体も両者で少し違う。小さな差として記録しておく。

## 書き味

同じ行コンポーネントの抜粋：

```rust
// NSH (Rust): ビルダーメソッド
view().w(px(22.0)).h(px(22.0)).rounded_full().border_2()
    .border_color(rgb(ACCENT))
    .bg(move || if item.get().done { rgb(ACCENT) } else { rgb(CARD) })
```

```rust
// Whisker: render! + Css ビルダー、リアクティブなスタイルは computed で包む
View(style: computed(move || Css::new().width(px(22)).height(px(22)).border_radius(px(11))
    .border(Border::new().width(px(2)).style(BorderStyle::Solid).color(Color::hex(ACCENT)))
    .background_color(Color::hex(if item.get().done { ACCENT } else { CARD }))))
```

```tsx
// NSH (TSX): Tailwind のクラス文字列
<View class={`size-[22px] rounded-full border-2 border-[#7C5CFF] ${props.item().done ? "bg-[#7C5CFF]" : "bg-[#1A1A22]"}`} />
```

- NSH は**プロパティ単位でリアクティブ**にできる（`.bg(move || ..)`）。Whisker はスタイル全体を `computed` で包むので、書く量が多い。
- Whisker の `render!` は木構造が見たまま読め、子コンポーネントを `#[component]` で分けるのが自然。その分けたコンポーネントの単位が、そのままホットリロードの単位になる。
- 同じ見た目にするのに、TSX は Rust 版のどちらよりも約半分の行数で済んだ。

## まとめ

- **NSH の強み**:
  - TSX のホットアップデート（約 0.1 秒、状態を保つ）と、ビルド済み host による Rust ビルドなしの速い初回起動
  - Tailwind 流のスタイル
  - 内在サイズを持つ部品
- **NSH の弱み**:
  - Rust で書く場合、dev ループが再起動で状態を失う
  - 0.4.0 時点では crates.io 経由の iOS ビルドが壊れている
  - CLI が 1.3 GB と重い
- **Whisker の強み**:
  - Rust のまま約 1 秒・状態を保ったホットリロード（NSH Rust 版との差がいちばん大きい点）
  - CLI が軽い
- **Whisker の弱み**:
  - 初回ビルドが重い（xcodebuild だけで 1分14秒）
  - `Input` が潰れる罠
  - スタイルの記述量が多い

Whisker への持ち帰り候補：

1. `auto_size` でない `Input` にも1行ぶんの内在高さを持たせる（少なくとも高さ 0 のときに警告を出す）
2. スタイルにプロパティ単位のリアクティブ指定を足す
3. モジュールのバージョンを本体と揃える
