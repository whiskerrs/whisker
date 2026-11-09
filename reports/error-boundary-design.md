# エラーバウンダリ設計案 (2026-11-08)

[NSH 比較レポート](nsh-ideas-for-whisker.md) の 1-3 を具体化したもの。
API と挙動に関わる判断は「8. 決めてほしいこと」にまとめた。Q1〜Q5 はすべて 2026-11-08 に決定済み。
確定後に英語にして `docs/error-boundary-design.md` へ移す想定。

## 1. 現状：panic は必ずアプリを落とす

- アプリのコードはすべて、`#[whisker::main]` が生成する `extern "C"` の入口から呼ばれる
  （`whisker_view_tick` / `whisker_view_dispatch_event` / `whisker_view_dispatch_pointer` / `whisker_view_dispatch_module_event` / `whisker_view_dispatch_resource_event`。[whisker-macros/src/lib.rs](../crates/whisker-macros/src/lib.rs:189)）。
- Rust 1.81 以降、`extern "C"` の境界を越えようとした unwind はその場で abort する。
  レポートで「推測」としていた「エフェクトやハンドラの panic でアプリが落ちる」は**確定**。
- ランタイム内部で、テスト以外の `catch_unwind` は `whisker-subsecond` の `call` だけ。
  これは `HotFnPanic` 以外を `resume_unwind` するので、結局 abort に届く。
- ただし unwind したときに壊れないための土台はすでにある。
  - `scheduler::flush` の `FlushGuard`：`flushing` フラグを戻す
  - `run_node_if_alive` の `RunGuard`：`owner_stack` と `current_tracker` を戻す
  - `Owner::with` の `Restore`：owner スタックを戻す
  - `with_runtime` の borrow ガード
  - `typed_subsystem_state_is_isolated_and_restored_after_panic` テスト

  足りないのは「どこで止めて、誰に届けるか」だけ。

## 2. 全体像

```
panic ─▶ 捕捉点で catch_unwind ─▶ (owner, ErrorInfo) を runtime の failures キューへ
                                          │
     安全点で配送 (execution_depth == 0 / flush ループの各周回 / drive_frame・dispatch_* の終わり)
                                          │
                 boundary_of(owner): owner の親をたどり、最寄りのバウンダリを探す
                    │                                  │
              見つかった                             なかった
     バウンダリの signal に error をセット        unhandled sink へ（その後は Q1 に従う）
     → バウンダリの effect が view の owner を
       dispose し、fallback を mount する
```

NSH と同じ形にする。NSH は `reactive.rs:66` の `boundaries: FxHashMap<Owner, Signal<Option<ComponentError>>>` と `boundary_of`（`reactive.rs:80`）で実現している。
Whisker の `Scope` はすでに `parent` を持っているので、親をたどる部分は新しく作る必要がない。

**unwind の途中では配送しない**。理由は 2 つ。

- 捕捉点の多くは、runtime の再入やほかの `RefCell` の中にある。
- 配送すると effect が走るので、unwind の途中で別のユーザーコードを動かすことになる。

配送する場所（安全点）は 3 つ。

- **`execution_depth` が 0 に戻ったとき**：`drain_disposals` を呼んでいるのと同じ場所。
- **`flush` ループの各周回**：空かどうかを判定する前に配送する。ループを抜けたあとで配送すると、バウンダリの effect が `pending` に積まれたまま、誰も起こさない。
- **`drive_frame` と各 `dispatch_*` の終わり**：初回のブートストラップで動く `mount_component` の body には `Execution` ガードがない。最後にここで回収しないと、そこで起きた失敗が配送されない。

`failures` キューは `ReactiveRuntime` に持たせ、thread-local にはしない。1 つのスレッドに複数の runtime context が入る構成が実際にあるため（`typed_subsystem_state_is_isolated_and_restored_after_panic`）。

**`AssertUnwindSafe` を使ってよい理由**：panic したコードが持っていた状態は、バウンダリが dispose する部分木の中にある。部分木の外にある signal が更新の途中で止まっている可能性は残る。これは Solid や React でも同じで、許容する。

## 3. 捕捉点

ユーザーコードが動く場所は、次の 5 か所に絞れる。

| # | 場所 | そこで動くユーザーコード | panic したあと |
|---|---|---|---|
| C1 | `scheduler::run_node_if_alive` の `(*borrow)()` | effect、computed の再計算 | ノードの依存は空のまま。owner の dispose を待つ |
| C2 | `lifetime::Scoped::with` / `with_mut` | イベントリスナー（[renderer.rs:988](../crates/whisker-runtime/src/view/renderer.rs:988)）、`on_mount`（[component.rs:84](../crates/whisker-runtime/src/reactive/component.rs:84)）、タスクの poll（[tasks.rs:213](../crates/whisker-runtime/src/tasks.rs:213)）、module のコールバック、text query | `Scoped` を cancel する。タスクは `Poll::Ready(())` を返して終える |
| C3 | `mount_component` / `mount_component_remountable` / `remount_sites` の body | `#[component]` の本体 | 空の placeholder 要素を返し、親の描画は続ける |
| C4 | `Owner::dispose_now` の `cleanups` と `registrations` | `on_cleanup` | cleanup を**1つずつ**捕まえ、残りも実行する |
| C5 | `__driver_runtime::{tick, dispatch_*}` | 上の 4 か所が取りこぼしたもの全部（ランタイム内部の assert も含む） | 最後の防衛線。unhandled sink に送ってから `std::process::abort()` する。dev ビルドでも同じ |

補足：

- **C2 は 1 か所で済む**。`Registered` がすでに `owner` を持っているので、配送先は `boundary_of(self.inner.owner)` で決まる。
- **C4 は必須**。`Execution::drop` は `drain_disposals` を呼ぶ。そのため、別の panic で unwind している最中に cleanup が走ることがある。そこで cleanup がもう一度 panic すると、二重 panic で abort する。
- **C3 の placeholder** は phantom 要素にする。body が途中まで作った要素は owner に登録済みなので、バウンダリが dispose すれば消える。
- **C5 まで届いた panic は abort する**。そこまで来ると、フレームが途中まで適用された状態や、sink への書き込みが途中の状態がありうる。runtime がどんな状態か分からないので、続行はしない。
  - 実際には、`extern "C"` の外へ再送出しても abort になる。C5 は、その前に sink へ届けて、rustc の "cannot unwind" より分かりやすい記録を残すためにある。
  - dev ビルドでは、sink がオーバーレイ用の記録を残す。次のパッチで全体を再ブートストラップし、そこで復帰する。
- `drain_runtime_dispatches` の `callback()`（他スレッドから post されたもの）は owner を持たない。各 callback を個別に捕まえ、unhandled 扱いにする。

### 3-1. 捕捉しない panic

- **`HotFnPanic`**：subsecond が「上位のコードが変わった」ことを、外側の `subsecond::call` まで巻き戻して伝えるための panic（[whisker-subsecond/src/lib.rs:259](../crates/whisker-subsecond/src/lib.rs:259)）。
  - すべての捕捉点で payload を `downcast_ref::<HotFnPanic>()` で調べ、該当したら `resume_unwind` する。
  - これを忘れると、上位のコードを変えたときのホットリロードが黙って効かなくなる。
- **ランタイム内部の不変条件の assert**（`Owner::new` の "cannot create a child of a disposed owner" など。Q3 で決定）
  - これらの `assert!` を `std::panic::panic_any(RuntimeInvariant { .. })` に置き換える。
  - 捕捉点は、`HotFnPanic` と同じく payload の型で見分けて `resume_unwind` する。
  - `panic_will_be_caught` は、この payload に対して偽を返す。

## 4. ErrorInfo とパニック位置の取得

panic の payload にはファイルや行番号が入っていない。そこで次の流れにする。

1. ランタイムの起動時に、既存のフックを `take_hook` / `set_hook` で**連鎖させて**、自前のフックを入れる。
2. フックは `info.location()`、メッセージ、（dev ビルドのみ）バックトレースを thread-local に退避する。
3. 捕捉点は catch したときに、その thread-local を取り出して `ErrorInfo` を組み立てる。

```rust
pub struct ErrorInfo {
    pub message: String,
    pub location: Option<Location>,      // file / line / column
    pub component: Option<&'static str>, // panic したコンポーネントの名前
    pub phase: ErrorPhase,               // Render | Effect | Event | Mount | Task | Cleanup
    pub backtrace: Option<String>,       // dev ビルドのみ
}
```

`component` を埋めるために、`#[component]` マクロから関数名を `mount_component` に渡す。今は `fn_ptr` しか渡していないので、ここに引数が 1 つ増える。

## 5. API 案

`show` と同じく `#[component]` で書き、`render!` からそのまま使えるようにする。

```rust
render! {
    ErrorBoundary(
        fallback: |error: ErrorInfo, reset: Reset| render! {
            View {
                Text(value: format!("読み込みに失敗しました: {}", error.message))
                Button(on_tap: move |_| reset.reset()) { Text(value: "再試行") }
            }
        },
    ) {
        Feed()
    }
}
```

- **`fallback` には新しい型が要る**。既存の `Fallback`（[into_view.rs:235](../crates/whisker-runtime/src/view/into_view.rs:235)）は `Fn() -> Element` で引数を取れないので、`ErrorFallback(Rc<dyn Fn(ErrorInfo, Reset) -> Element>)` を新しく作る。
- **`Reset`** は `Copy` なハンドル。`reset()` を呼ぶと error を `None` に戻し、fallback を dispose して children を mount し直す。
- **バウンダリの effect には、直前の状態を覚えるガードを付ける**（`Show` の `last_cond` と同じ）。`set` は値が同じでも必ず通知するので、ガードがないと、children を表示中に `reset()` しただけで作り直してしまう。
- **バウンダリが見張るのは children の側だけ**。fallback を表示している間は、そのバウンダリを登録しない（NSH の `flow.rs:123-130` と同じ）。fallback の中で起きた panic は、1 つ外側のバウンダリに届く。
- **リストの行ごとに置ける**。`ForEach` の行も、virtualizer の `MountedEntry` も、行ごとに独自の owner を持っている。行の中に `ErrorBoundary` を置けば、壊れるのはその 1 行（たとえばフィードのカード 1 枚）だけで済む。

### 5-1. module 作者向けの API

| API | 用途 |
|---|---|
| `whisker::error::panic_will_be_caught(payload) -> bool` | panic フックの中から呼び、この panic が**バウンダリに届くか**を知る。判定は次の 3 段階 |
| `whisker::error::on_error(f: impl Fn(&ErrorInfo, Handled))` | 捕捉されたエラーも含めて、すべてのエラーを観測する |

`panic_will_be_caught` の判定：

1. 捕捉点は入るときに、自分の owner を thread-local のスタックに積む。
2. フックの中では、そのスタックの一番上の owner について `boundary_of(owner).is_some()` を返す。dev ビルドではルートに暗黙のバウンダリがあるので、常に真になる。
3. payload が `HotFnPanic` なら偽を返す。

「捕捉点の中にいるか」だけで判定してはいけない。Q1 を (a) にした場合、バウンダリのない panic は捕捉点を通ったあとで abort する。そのとき crashlytics には non-fatal が 1 件記録され、そのあとに名前のない abort が続くことになり、今より悪くなる。

crashlytics にはこの両方の API が必要になる（6-2）。

## 6. ほかの仕組みとの関係

### 6-1. ホットリロード

- `tick` の中で `hot_reload.apply` が成功したら、dev ビルドではエラー状態のバウンダリをすべて `reset()` する。「エラー表示 → 直して保存 → 元に戻る」が成り立つ。
- `remount_sites` で作り直した body が panic したら、C3 で捕まえ、その site の親 owner から見て最寄りのバウンダリに届ける。

### 6-2. crashlytics

[crashlytics の `record_panics`](../packages/whisker-firebase/crashlytics/src/lib.rs:117) が入れるフックは、クラッシュレポートを書いたあとプロセスを終える。
panic フックは `catch_unwind` より**前**に走るので、このままではバウンダリがあっても先にアプリが落ちる。

修正は次の 2 つ。

- フックの中で `panic_will_be_caught()` が真のときは、クラッシュではなく non-fatal として記録し、プロセスを終えない。
- `on_error` で捕捉されたエラーも non-fatal として送る。

フックを入れる順番には依存しない。フックはどちらも連鎖し、判定はフックを呼んだ時点の thread-local を見るだけだから。

### 6-3. computed のトランポリン

[computed.rs:61-68](../crates/whisker-runtime/src/reactive/computed.rs:61) は、クロージャを take → 実行 → put-back する。
実行中に panic すると put-back が飛ばされ、その computed は二度と再計算されなくなる。

- バウンダリが owner ごと dispose するなら問題にならない。
- しかしバウンダリがなく、release ビルドで生かし続ける設定（8 章の Q1）にした場合は、壊れたまま残る。

そこで put-back を drop ガードにする。

### 6-4. 捕捉できない環境

| 環境 | 理由 | 挙動 |
|---|---|---|
| Web（wasm32） | 標準のツールチェーンでは panic = abort になる | フックが sink に送ったあと、abort する |
| `profile.*.panic = "abort"` | `catch_unwind` が何もしない | 同上 |

`whisker doctor` で、アプリの profile に `panic = "abort"` があれば警告を出す。

## 7. 実装の段階分け

1. **土台**：failures キュー、安全点での配送、C1〜C5、`HotFnPanic` の素通し、パニックフック、unhandled sink。
   - この段階では、バウンダリがなくても今と同じく落ちる（Q1 の既定値に従う）。
2. **`ErrorBoundary` コンポーネントと `Reset`、ホットリロード時の自動 reset**
3. **crashlytics の対応**（`panic_will_be_caught` と `on_error`）
4. **dev のエラーオーバーレイ**（レポートの 2-3）。unhandled sink の消費者として別途作る。この設計の範囲外。

### テスト（`reactive/tests.rs` と `tests/render_macro.rs`）

- effect が panic すると fallback が表示され、兄弟の要素は残る
- イベントハンドラ、`on_mount`、タスクの panic が、それぞれ最寄りのバウンダリに届く
- dispose 中に cleanup が panic しても abort せず、残りの cleanup も実行される
- `HotFnPanic` は捕捉されずに素通りする
- `reset()` で children が mount し直される
- fallback の中の panic は、外側のバウンダリに届く
- バウンダリがないときは unhandled sink に届く
- computed が panic したあとも、次の書き込みで再計算される
- C3：body が panic しても親の描画が最後まで進み、placeholder が置き換わる
- ブートストラップ中の mount で panic し、そのあと flush が起きなくても、バウンダリに届く
- `HotFnPanic` に対して、`panic_will_be_caught` が偽を返す
- バウンダリのない panic に対して、`panic_will_be_caught` が偽を返す（release ビルド）

## 8. 決めてほしいこと

| # | 問い | 選択肢 | 推奨 |
|---|---|---|---|
| Q1 | release ビルドで、どのバウンダリにも捕まらなかった panic をどうするか | **決定（2026-11-08）：(a) 今と同じく abort**（クラッシュレポーターに残る）。不採用：(b) sink に送って生かし続ける | **(a)**。生き残るかどうかは、バウンダリを置いた場所でアプリ作者が選ぶ。dev ビルドではルートに暗黙のバウンダリを置いて、オーバーレイを出す（C5 まで届いたものは除く） |
| Q2 | イベントハンドラの panic もバウンダリに届けるか | **決定（2026-11-08）：推奨どおり**。(a) 届ける（NSH と同じ） (b) 届けない（React と同じ。unhandled 扱い） | **(a)**。Whisker では届けないと abort になるので、React と前提が違う |
| Q3 | ランタイム内部の不変条件の assert を、ユーザーの panic と同じように扱うか | **決定（2026-11-08）：推奨どおり**。(a) 同じに扱う (b) 常に再送出して落とす | **(b)**。捕まえると、フレームワークのバグが隠れて状態の破損が広がる。ただし dev ビルドではオーバーレイに出す |
| Q4 | module 向け API の名前と形 | **決定（2026-11-08）：推奨どおり**。`panic_will_be_caught()` + `on_error()`、または `ErrorHook` trait にまとめる | **関数 2 つ**。crashlytics 以外にほぼ使い道がないので、小さく保つ |
| Q5 | `ErrorBoundary` の名前と置き場所 | **決定（2026-11-08）：推奨どおり**。`ErrorBoundary`（React / Solid / Leptos と同じ）／`CatchError`（NSH と同じ） | **`ErrorBoundary`**。`whisker::prelude` に入れる |

## 9. 実装との差分（feat/error-boundary、2026-11-08）

実装は設計どおりだが、次の点が違う。

| 項目 | 設計 | 実装 | 理由 |
|---|---|---|---|
| バウンダリのない panic | C5 で sink に送ってから abort | 捕捉点で `resume_unwind` し、今までと同じ経路で落ちる（モバイルでは `extern "C"` で abort） | Q1 の決定と同じ結果になり、既存のテストや desktop の挙動も変わらない。C5 と sink は dev オーバーレイと一緒に作る |
| `on_error` | module 向け API の 1 つ | **未実装** | crashlytics はフックの中で `panic_will_be_caught` を見るだけで non-fatal に振り分けられ、`on_error` を使わない（要確認） |
| `HotFnPanic` の素通し | 全捕捉点で再送出 | 対応していない | 同梱の subsecond fork は `HotFnPanic` を一度も生成しない（`try_call` は常に `Ok`）。runtime から subsecond への依存も増やさずに済む |
| `ErrorInfo.component` | コンポーネント名を入れる | 入れていない | `mount_component_remountable` の引数とマクロ出力が変わるので、別途 |
| 不変条件の panic | `panic_any(RuntimeInvariant)` | メッセージの接頭辞 `whisker-runtime invariant violated: ` で見分ける | `panic_any` だと既定のフックが `Box<dyn Any>` としか表示せず、メッセージが失われる |
| dev のルートバウンダリとオーバーレイ | dev では暗黙のルートバウンダリ | 未実装（7 章の 4） | 範囲外のまま |
| cleanup の panic の届け先 | 失敗したスコープの親 | dispose を始めたスコープの親 | dispose は親スコープから先に取り除くので、子の cleanup が走る時点では直接の親がもういない |
| 配送の安全点 | `execution_depth == 0`、flush ループ、`drive_frame` / `dispatch_*` の終わり | flush ループだけ。あわせて `has_pending_work` に未配送の失敗を含めた | 入口の最後で配送すると、レンダラーが外れたあとで fallback を mount してしまう。最後の flush より後に捕まえた失敗（`flush_mounts` の中など）は `wake_runtime` で次のフレームを起こして配送する |
| 公開 API | `ErrorBoundary`、`ErrorInfo`、`Reset`、`panic_will_be_caught`、`on_error` | `ErrorBoundary` / `ErrorFallback` / `ErrorInfo` / `Reset` を prelude に、`whisker::error::{ErrorInfo, ErrorLocation, ErrorPhase, panic_will_be_caught}` を公開。`catch` / `set_error_boundary` / `reset_on_hot_patch` は `#[doc(hidden)]` | 後者 3 つは `whisker` crate の `ErrorBoundary` から runtime を呼ぶために必要（要確認） |
