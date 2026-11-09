# NSH から Whisker に取り入れられるアイデア (2026-11-07)

NSH（[longbridge/nsh](https://github.com/longbridge/nsh)、commit `4ca2672`、Rust 約 90 万行）のコードを、次の4つの領域に分けて調べた。

- リアクティブ / ホットリロード
- スタイル / 部品
- エンジン / プロトコル / 性能
- ツール / テスト / 配布

各領域で Whisker の該当コードと突き合わせ、主要な主張は自分でも grep とソースで確かめた。
実際にアプリを作って比べた結果は [nsh-hands-on/README.md](nsh-hands-on/README.md) にある。

NSH 側のファイル参照は、NSH リポジトリのルートからの相対パス（`N/…`）で書く。

## 優先度順の早見表

| # | アイデア | 領域 | 価値 | コスト |
|---|---|---|---|---|
| 1 | `Input` に既定の1行ぶんの高さを持たせる | 部品 | 高 | S |
| 2 | `ForEach` の行に item/index をシグナルで渡し、必要な行だけ動かす | リアクティブ | 高（不具合修正を含む） | S |
| 3 | 公開前チェック：クレート外を指すパスの lint と、iOS/Android 向けの単体ビルド | 配布 | 高 | S〜M |
| 4 | MSRV の参照元を1か所にまとめる | 配布 | 中 | S |
| 5 | コミットごとの時間計測と `--trace` | 性能 | 高 | S〜M |
| 6 | `Css` の短縮形（`border_width` / `padding_x` / `size` / `inset`） | スタイル | 中 | S |
| 7 | 色・長さの setter に `var()` を渡せるようにし、トークンで theme を組む。システムのダークモードを追う | スタイル | 高 | S〜M |
| 8 | ヘッドレスでアプリを動かすテスト基盤 `whisker-testing` + `whisker test` + テンプレートにテスト同梱 | テスト | 高 | M〜L |
| 9 | エラーバウンダリと、ホットパッチ後の panic の隔離 | リアクティブ | 高 | M |
| 10 | `global::<T>()`：フルリマウントでも消えないアプリ状態 | ホットリロード | 高 | S〜M |
| 11 | 押下・無効などの状態スタイルをネイティブで適用し、その上にスタイルなし部品群を作る | 部品 | 高 | L |

この先は領域別に、NSH のやり方、Whisker の現状、得られるもの、コストの順で書く。

---

## 0. 調査中に見つかった Whisker の不具合（アイデア以前に直すもの）

- **`ForEach` が古いデータを表示し続け、行がリークする**（[control_flow.rs:103-142](../crates/whisker/src/control_flow.rs:103)）。いずれもコードで確認した。
  - 同じキーのまま中身が変わっても、新しい `item` は捨てられ、表示が更新されない。
  - キーが重複すると、2つ目の行が `new_entries.insert` で最初のエントリを上書きする。最初の行の owner は dispose されず、要素も fragment に付いたまま残る。結果として**リークし、画面に孤立した行が残る**。
  - 1件追加するだけでも、残る行をすべて一度外して付け直す。フォーカス中の入力欄を含む行があれば、キーボードが閉じる。
- **`Input` が高さ 0 に潰れる**（[measurement.rs:7-9](../packages/whisker-input/src/measurement.rs:7)）。`auto_size` が false だと計測自体をしない。hands-on で実際に踏んだ。
- **MSRV の記述が食い違っている**。
  - `Cargo.toml` は `rust-version = "1.88"`。
  - [doctor.rs:234-239](../crates/whisker-cli/src/doctor.rs:234) と [CONTRIBUTING.md:15](../CONTRIBUTING.md) は 1.85 のまま。
- **ドキュメントの使用例がコンパイルできない**。[whisker-safe-area](../packages/whisker-safe-area/src/lib.rs:17) と whisker-keyboard の使用例は `format!` で作った CSS 文字列を `style:` に渡しているが、`Style` はこれをコンパイル時に拒否する。
- **存在しない文書への参照がある**。[whisker-cli/src/lib.rs:90](../crates/whisker-cli/src/lib.rs:90) が `docs/module-author-guide.md` を参照しているが、そのファイルはない。
- **公開済み tarball のテストが壊れている**。[whisker-cng/tests/project_composition.rs:14-19](../crates/whisker-cng/tests/project_composition.rs:14) が `include_str!("../../whisker-plugin/…")` を使い、その tests を crate に同梱している。tarball から `cargo test` を流すと失敗する（利用者のビルドには影響しない）。
- **Android の snap が二段階に動く（推測）**。[WhiskerContainerView.kt:477-546](../platforms/android/module/src/main/kotlin/rs/whisker/runtime/WhiskerContainerView.kt:477) は、32ms ごとのポーリングで 2 フレーム静止を待ってから、現在位置に最も近いターゲットへ `smoothScrollTo` する。fling の速度は無視される。iOS は投射先から決めるので、両 OS で挙動がずれる。
- **`whisker.rs/llms.txt` が 404**。

## 1. リアクティブ / コンポーネント / リスト

### 1-1. `ForEach` を NSH の `each` に揃える（S）
- **NSH**: 行関数は `Fn(ReadSignal<T>, ReadSignal<usize>)`（`N/crates/nsh-core/src/element/flow.rs:314`）。
  - 同じキーの行には `row.item.set(item); row.index.set(index)` で新しい値を流す。
  - 重複したキーは別の行として扱う。
  - 並べ替えでは、位置が正しくない行だけを挿入し直す。理由はコメントにある（"Moving a native view … ends its touches and its focus"、`core/components.rs:~2975`）。
- **Whisker**: 上記 0 の不具合のとおり。`List` はすでに `ReadSignal<T>` を渡しており、`ForEach` とだけ形が違う。
  virtualizer には「位置が合っていれば動かさない」差分がすでにある（[virtualizer.rs:1600-1608](../crates/whisker-runtime/src/view/virtualizer.rs:1600)）。これを移植すれば済む。
- **得られるもの**:
  - 項目を編集するとその場で反映される。
  - 1件追加すれば、ネイティブの挿入は1回で済む。
  - フォーカスが外れなくなる。

### 1-2. 等しい値の書き込みでは通知しない（opt-in なら S、既定にするなら M）
- **NSH**: `signal<T: PartialEq>` は、等しい値を書き込んでも誰にも通知しない。`signal_with(.., Never)` で無効化できる（`N/crates/nsh-core/src/element/reactive.rs:499`）。
- **Whisker**: `set` は常に通知する（[signal.rs:155-162](../crates/whisker-runtime/src/reactive/signal.rs:155)）。そのため `Show` は `last_cond` で自前に防いでいる。
- **進め方**: まず `set_if_changed` を足し、既定にするかは移行コストを見て決める。

### 1-3. エラーバウンダリ（M）
- **NSH**: `catch_error(view, |error, reset| fallback)`（`flow.rs:208`）。computation とクリーンアップをすべて `catch_unwind` の中で実行し、失敗は owner をたどって最寄りのバウンダリに届ける。
- **Whisker**: テスト以外の `catch_unwind` は、subsecond 自身のものと crashlytics モジュールの報告用だけ。エフェクトや `on_tap` の panic は host のコールバックまで巻き戻り、`extern "C"` 境界でアプリが落ちる（推測）。
- **得られるもの**: ホットパッチ後のコンポーネントが `unwrap` で panic しても、アプリは生き残り、エラーを表示し、次の保存で復帰する。本番では、フィードのカード1枚が落ちてもアプリ全体は落ちない。

### 1-4. その他（S〜M）
- **`ListRef::scroll_to` が計測後も目標を追う（S〜M）**:
  - NSH の `follow_pin`（`core/list.rs:758`）は、行が計測されるたびに目標を補正する。
  - Whisker は推定値（44px）で一度だけ解決し、その後は追わない。行の高さが可変だと、ジャンプ先がずれる。
  - `initial_scroll` で使っている保留の仕組みを流用できる。
- **フォーカス中の行をウィンドウ外でも保持（M）**:
  - NSH の `Core::sync_kept` は、フォーカスやオーバーレイを持つ行を、スクロールで外れても保持する。
  - Whisker の virtualizer はフォーカスを見ていないので、`List` 内の入力欄をスクロールで外すと、入力中の内容が消える。
- **表示中の項目の変化を通知するコールバック（S）**: NSH の `on_viewable_items_change` に相当するもの。既読処理、インプレッション計測、自動再生に使える。Whisker には、呼ばれたときに現在値を返す `snapshot().visible_keys` しかない。
- **シグナルアクセスに `#[track_caller]`（S）**: 破棄済みシグナルで panic したときに、呼び出し元が分かる。メモリに残している「ハンドラ内の破棄済みシグナル」系の不具合の調査に効く。

## 2. ホットリロード

### 2-1. `global::<T>()`：リマウントでも消えない状態（S〜M）
- **NSH**: 型ごとにサーフェスあたり1つのシグナルを、detached owner の下に作る（`N/crates/nsh-core/src/element/ambient.rs:453`）。TSX 側でも、トップレベルの `signal()` は、初期化式のシグネチャが変わらない限りモジュールを再実行しても残る（`N/crates/nsh-js/src/hot.rs:680`）。
- **Whisker**: 公開 API に相当するものはない。[hot-reload-internals.md](../docs/hot-reload-internals.md) のとおり、ヘルパーや型、ルートを編集すると全状態が消える。
  `runtime_local` はリマウントを越えて残るので、土台はある（推測）。
- **注意**: `TypeId` は型のレイアウトを含まない。パッチで `AppState` にフィールドが増えると、`downcast` が通って未定義動作になる。props-layout gate と同じく、レイアウトのハッシュでスロットを分ける必要がある。
- **得られるもの**: ヘルパー関数を直しても、ルーターのスタック、ログイン状態、フォームの下書きが消えない。

### 2-2. ヘルパーの編集で、同じファイルのコンポーネントだけ作り直す（M）
- **NSH**: コンポーネントのシグネチャは「自分のコード + モジュールの他のコード」。ヘルパーを編集すると、そのモジュールのコンポーネントだけが再描画される（`N/docs/dev/tooling.md:210-224`）。
- **Whisker**: `#[component]` は自分の本体だけをハッシュする。ヘルパーの編集では `remounted == 0` になり、ルートからのフルリマウントに落ちる。
- **進め方**: ファイル内の非コンポーネント部分のハッシュを、各コンポーネントのハッシュに混ぜる。proc-macro からファイルの内容を取れるかは要確認。

### 2-3. 端末上のエラーオーバーレイ（M）
- **NSH**: ビルド失敗、評価エラー、ハンドラの例外を、コード片付きの赤いオーバーレイで出す。タップか次の正常なリロードで消える（`N/website/docs/dev-server-and-debugging.md:203-230`）。
- **Whisker**: panic は端末のターミナルへのログ転送だけ（[log_capture.rs](../crates/whisker-dev-runtime/src/log_capture.rs)）。パッチのビルドが失敗したり panic したりすると、端末側は固まるか真っ白になる。

## 3. スタイル / テーマ / 部品

### 3-1. `Input` の内在的な高さ（S、最優先）
- **NSH**: `measure_input` で常に計測する。測る文字列は、値 → プレースホルダー → 空行の順に選ぶ（`N/crates/nsh-core/src/core/mod.rs:3300`）。
- **Whisker の現状**: `auto_size` でなければ、`measurement.rs:7-9` が `None` を返し、高さは 0 になる。chat サンプルは固定の高さを指定して回避している。
- **提案**: `auto_size` でなくても、`lines` 行ぶん（既定 1 行）をテキストスタイルから測る。明示した高さは今までどおり優先する。

### 3-2. トークンは既存の custom property で実現する（S〜M）
- **既にあるもの**: Whisker には継承される custom property（`Css::custom_property` / `property_variable`、[css.rs:174-220](../crates/whisker-css/src/css.rs:174)）が**すでにある**。
- **足りないもの**:
  - `background_color(v: Color)` などの素の setter が変数を受け取らない（[background.rs:19](../crates/whisker-css/src/prop/background.rs:19)）。
  - そのため、theme を変えると `computed` で全要素の `Css` を組み直すことになる（chat の `appearance.rs`）。
- **提案**:
  - setter を `impl Into<ValueOrVariable<T>>` に広げる（素の値は既存の `From` で変換される）。
  - トークンは `CustomPropertyName` 定数の構造体としてアプリ側で定義する。
  - システムのダークモードをシグナルとして公開する。Rust / Swift / Kotlin を探しても、color scheme を扱うコードは見つからなかった。
- **得られるもの**: テーマの切り替えが、ルートの更新1回で済む。NSH の「setter ごとにクロージャ」をまねるより、Whisker の CSS 継承に合っている。

### 3-3. 短縮形（S）
- **NSH**: `style_helpers!` 表（`N/crates/nsh-core/src/element/style.rs:1074`）で `px`→PaddingHorizontal、`size`→Width+Height、`inset`→四辺を定義している。
- **Whisker**: `border_width` / `border_color` / `padding_x` / `size` / `inset` を、**CSS の名前で**足す。chat サンプルが `Border::new().width(px(1)).color(..).style(Solid)` を何度も繰り返している箇所が短くなる。

### 3-4. 押下・無効・フォーカスのスタイルをネイティブで（L）と、スタイルなし部品群（M ずつ）
- **NSH**: `.active(|s| ..)` は view の状態に紐づくルールに変換され、押しても Rust は動かない（`N/crates/nsh-core/src/element/builder.rs:1855`）。`disabled` は press リスナーも外す。
  その上に、見た目を持たない `Button` / `Switch` / `Tabs` / `Menu` / `Select` / `Dialog` がある（`N/crates/nsh-base`）。Button は 75 行。
- **Whisker**: 押したときの反応がない。無効状態は `on_tap` のガードと opacity を手で書いている（chat の `button.rs`）。
- **進め方**:
  1. `Css::pressed(Css)` のような状態レイヤーを入れる。
  2. host が Rust を経由せずにレイヤーを切り替えられるか確かめる（未検証）。
  3. Button → Switch → Tabs の順に作る。

### 3-5. その他（S〜M）
- **safe area とキーボードの値を custom property として host が書き込む（M）**: `padding_bottom(var(--keyboard-inset-height))` のように書ける。キーボードが動くフレームごとに Rust で `Css` を作り直す必要がなくなる。`calc()` の中で `var()` が使えるかは要確認。
- **`Input` の色を型付きの `Css` に（S）**: `caret_color` / `placeholder_color` / `selection_color` は、今は CSS の色文字列を持つ props になっている。`StyleProperty::CaretColor` はすでにある。
- **`Css::when(cond, |c| ..)` と、Android の ripple / elevation（S / M）**。

## 4. エンジン / プロトコル / 性能

### 4-1. コミットごとの時間計測とトレース（S〜M、優先度高）
- **NSH**: `CommitTiming { js, layout, ops, items }`（`N/crates/nsh-engine/src/perf.rs:61`）。サーフェスごとに `PerfMonitor` を置き、計測していないときのコストは `Option` の判定1回だけ。
  `nsh test --trace` は mount の回数を出し、決定的なので CI のゲートに使える。hot path を変更するときは、変更前後のトレース値を添えることが規約になっている（`N/docs/dev/performance.md`）。
- **Whisker**: runtime / engine / driver のどこにも計測コードがなく、`[[bench]]` もない。
- **進め方**: まず `drive_frame` の中で、reactive flush / Taffy / 計測の往復 / host apply の時間と回数を取り、CLI フラグで表示する。単一スレッドなので、時計が1つで済み、NSH より素直に作れる。

### 4-2. テキスト計測を幅の範囲で再利用する（S〜M）
- **NSH**: 貪欲に改行するテキストエンジンなら、計測時の最大行幅から、計測した幅までのどの幅でも改行位置は同じになる。なので、その範囲の幅には typeset せずに答える（`N/crates/nsh-core/src/text.rs:728`、`core/mod.rs:8845`）。
- **Whisker**: キャッシュキーは制約の完全一致（[measurement.rs:668-678](../crates/whisker-engine/src/measurement.rs:668)。iOS のキャッシュも同じ）。Taffy は1つのテキストに min-content / max-content / 確定幅と何度も問い合わせるので、そのたびに FFI を往復する。
- **注意**: Android の `BREAK_STRATEGY_HIGH_QUALITY` は貪欲な改行ではない。この最適化は host ごと・改行戦略ごとのフラグにし、`max_lines` や省略記号があるときは無効にする。

### 4-3. snap 先を共有の Rust で決める（S）
- **NSH**: React Native と同じ規則の `snap_target(offset, projected, velocity, ..)` を Rust に置き、単体テストしている（`N/crates/nsh-core/src/mount.rs:277`）。
- **Whisker**: 同じ式を iOS と Android で別々に書いていて、挙動も違う（0 を参照）。
- **進め方**: 純粋関数として protocol に置く。Android はポーリングをやめ、fling の終わりを検知するフックに替える。

### 4-4. relayout 境界（M）
- **NSH**: 固定サイズのノードを独立にレイアウトし、変化したセル1つあたりのコストを 140µs から 36µs に下げた（`N/crates/nsh-core/src/core/layout_tree.rs:444`、`N/docs/dev/performance.md:157`）。
- **Whisker**: `mark_dirty` で祖先がすべて dirty になり、毎回ルートからレイアウトする（[whisker-layout/src/lib.rs:443-461](../crates/whisker-layout/src/lib.rs:443)）。
- **注意**: 対象にできる条件が細かい。差分テスト（境界ありのレイアウトとフルレイアウトを比較）を必須にする。

### 4-5. overscan 行のマウントを時間予算で複数フレームに分ける（M）
- **NSH**: 見えている行は即座にバインドし、残りは 1 フレームあたり 2ms / 150 op まで、後続のフレームで進める（`N/crates/nsh-engine/src/lib.rs:1279`）。
- **Whisker**: 範囲全体を1フレームでマウントする。速い fling や、画像の多い行で効く。

### 4-6. 既存アプリへの埋め込み（M〜L）
- **NSH**: ビューごとの登録表、起動パラメータ（`set_params`）、ホストとのメッセージ（エンジン不在中はバッファ）、Promise を返す `host.call`、URL をホスト側で先に処理する仕組み（`N/crates/nsh-engine/src/host_channel.rs`）がある。
- **Whisker**:
  - `whisker_view_create` は引数なしの1シンボルだけ。
  - モジュールの登録表はプロセス全体で1つ。
- **進め方**: 起動パラメータとホストとのメッセージは M。ビューごとのモジュールインスタンスは L で、ライフサイクル、コード生成、ホットリロードに影響する。

## 5. ツール / テスト / 配布

### 5-1. 公開前チェック（S〜M、優先度高）
NSH はまさにこの穴で事故を起こしている。
- issue #940 は、新しいクレートを切って修正した。
- 同じ種類の `#[path = "../../nsh-platform-macos/src/audio.rs"]` が、別の PR #955 で**すでに再発している**（`N/crates/nsh-platform-ios/src/lib.rs:45`、確認済み）。
- 原因は、公開時の検証が Linux で走り、iOS 専用のクレートは Linux では空になることにある（未マージの PR #1114 が指摘している）。

Whisker も、公開は ubuntu で `cargo publish` するだけで、同じ死角を持っている。
- (a) 公開対象のクレートで、クレート外を指す `#[path]` / `include_*!` を xtask の lint で検出する。tests も crate に同梱されるので、対象に含める。
- (b) `cargo package --workspace --no-verify` で展開したものを `[patch.crates-io]` で差したアプリを作り、`aarch64-apple-ios-sim` と Android 向けにビルドする。既存の mobile-link-test のランナーで走る。

### 5-2. MSRV の参照元を1か所に（S）
- doctor は `env!("CARGO_PKG_RUST_VERSION")` を使う。
- CI は `cargo metadata` から MSRV を読む。
- CONTRIBUTING は `Cargo.toml` を指すだけにする。
- 任意で、MSRV のチェックを iOS / Android ターゲットでも走らせる。

### 5-3. ヘッドレステスト基盤（M〜L）と `whisker test`（S）
- **NSH**:
  - `Headless::start(..)` → `app.wait_for("Count: 0")` → `app.press("Tap")` と書ける。手動の時計とネットワークのスタブもある（`N/crates/nsh-testing/src/lib.rs`）。
  - 失敗すると、画面上のテキストを報告する。
  - テンプレートは最初からテストを含み、`nsh test --junit` が使える。
  - 同じセレクタで、実機を座標なしで操作することもできる（`echo 'await app.press("Add")' | nsh test --target ios`）。
- **Whisker**: 一部はもうある。
  - `drive_frame` はホストから時刻を受け取る。
  - `RecordingRenderer` がある。
  - 足りないのは、アプリ作者向けの API と、「アイドルになったか」の答え（`needs_frame` で作れる）。
- **得られるもの**: giga や Bluesky のアプリを、実機なしで決定的にテストできる。エージェントの UI 検証も、座標探しから解放される。

### 5-4. エージェントとコントリビューター向けの知識の置き場（S）
- **NSH**:
  - `AGENTS.md` に「X を変えるなら Y を読む」の表がある。
  - `docs/dev/pitfalls.md`（680 行）には、検証済みの制約を領域ごとにまとめてある。
  - 利用者向けには `npx skills add longbridge/nsh` と `llms.txt` がある。skill とドキュメントの内容がずれたら、テストで落ちる。
- **Whisker**:
  - CLAUDE.md はコメント規約だけ。
  - 苦労して得た制約（`Show` の再アンカー、ホットパッチの条件表、SPM pin の一括更新など）は、私的なメモリファイル約 130 件にしかなく、他のコントリビューターにも CI にも見えない。
- **提案**: 公開してよいものを `docs/pitfalls.md` に移し、CLAUDE.md から参照する。移す過程で、Lynx 時代の古い記述も整理できる。

### 5-5. その他（S〜M）
- **CI のスケジュール調整**: draft はスキップ、PR 番号で concurrency を組む、merge queue に対応。
- **`NO_COLOR` 対応**: doctor が TTY を見ずに ANSI を出している。
- **docs のリンク切れチェック**。
- **`cargo-semver-checks`**: release-prepare に入れ、互換性ポリシーを短い1ページにまとめる。

## 6. まねしないもの

- **エンジン用の別スレッド**: NSH はこのために次の対応を抱えている。Whisker の「スレッドではなく責務で分ける」設計と、同期的な計測呼び出しの前提を壊すことになる。
  - テキストエンジンからメインスレッドのオブジェクトに触れない
  - リサイズ時に最大 16ms ブロックする
  - スクロール報告に適用済みコマンドの番号を付ける

  計測、再利用、境界、予算化（4-1〜4-5）は、別スレッドがなくても効く。
- **Tailwind のクラス文字列と Tailwind 尺度のメソッド名（`px_4()`）**:
  - Whisker の `px(4)` は 4px の長さなので、名前がぶつかる。
  - 型付きで文字列を受け付けない方針（compile_fail テスト）とも合わない。
  - 短縮形は CSS の名前で足す（3-3）。
- **署名付き OTA パッケージ（`.nshpkg`）**: JS バンドルを解釈実行するから必要になる仕組み。Whisker はネイティブバイナリなので、実行時に差し替える対象がない。ストアの規約とも衝突しやすい（推測）。
- **キーを越えてセルを使い回し、行の状態をアプリの外に置かせる方式**: Whisker の「キーごとの owner + 見た目だけをプールする」方が安全。
- **公開の `estimated_item_size` を主な契約にすること**: Whisker は意図的に持っていない。推定値で困る箇所（scroll_to）だけを 1-4 で直せばよい。
- **ナビゲーションを UIKit / Android のスタックに任せること**: Rust が持つルーター（離脱アニメーション中は画面を固定する）を崩す。
- **`rust-version` を宣言しないこと、Linux だけで公開時の検証をすること**: NSH 0.4.0 の事故の直接の原因。

## 7. 進め方の案

1. **すぐやる（S、各数時間〜1日）**: `Input` の高さ、`ForEach` の修正、MSRV の一本化、ドキュメント例とリンクの修正、短縮形、`#[track_caller]`、公開パスの lint。
2. **次にやる（M）**: `drive_frame` の計測と `--trace`、setter の `var()` 対応 + ダークモード、エラーバウンダリ、`global::<T>()`（レイアウトハッシュ付き）、パッケージを展開してのモバイルビルド、snap 先の共有化、テキスト計測の範囲再利用。
3. **大きな投資（M〜L）**: ヘッドレステスト基盤 + `whisker test`、状態スタイル + スタイルなし部品群、relayout 境界、overscan の予算化、埋め込み API。
