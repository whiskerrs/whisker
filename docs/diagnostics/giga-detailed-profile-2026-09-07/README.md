# GIGA Android：修正後debugビルドの詳細プロファイル

2026-09-07。対象は `bb5656bd` のruntime最適化を入れたGIGA。simpleperfによるCPUサンプリングと、RustにATrace区間を追加したPerfetto計測を別々に実施した。

## 結論

残っている負荷は主に3種類ある。

1. **一覧スクロールとリーダーから戻る際の、繰り返しレイアウト計算。** 遅いtickでは、文字計測を挟んで4回のレイアウト計算が走る。スクロール中のCPUサンプルの44.0%で `LayoutTree::compute` を観測した。
2. **スタイル反映時の描画状態のコピーと破棄。** 子孫のスタイル解決を省いても、`SurfaceEngine` 全体のコピーは残っている。Perfettoでは1回のコピーが操作別平均3.9〜7.9ms。画面遷移のCPUサンプルではコピーに17〜19%、旧状態の破棄に8〜10%が含まれる。
3. **Android側の画面破棄。** Group→Homeの最も遅いtickでは、113〜176msのうち108〜169msをAndroidへの反映が占める。simpleperfでは `HostScene.validateStagedFrame`、`deleteNode`、子孫判定の親チェーン走査が確認できた。

Rustの更新時間だけでなく、同じ時刻のFrameTimelineでも `App Deadline Missed` を確認した。主にUIスレッドが実行中で、スケジューラー待ちだけでは説明できない。

## 環境と計測方法

- Androidエミュレーター `emulator-5554`、`sdk_gphone16k_arm64`、API 37、1080×2400。実機ではない。
- GIGA 3.0.0 / versionCode 26、Whisker 0.13.7＋既存runtime最適化。Rustはdebugビルド。
- runtime / engine / driverの公開済みcrateを一時ディレクトリに複製し、runtimeの2ファイルだけ既存修正版に差し替えた。その上に計測区間を追加した。Kotlinの製品コードは変更していない。
- APK内の `libgiga.so` とシンボル解決用の非stripライブラリの `.text` SHA-256が一致：`e4c05105ca958d613030105f08eca9b0e5d45f72e8383efad682202aacf879ee`。
- Perfetto：5操作×2回。ATrace、sched_switch / sched_waking、FrameTimelineを記録。各操作7秒、スクロール10秒。読み込み後の更新まで含めた。
- simpleperf：5操作×1回。`cpu-clock`、400Hz、call graph。Perfettoは停止した状態で実行。全記録でlost=0。
- プロファイラーありの15計測を通じてアプリのPIDは27717。操作前後でPIDの維持と描画フレーム発生を検査し、保存画像で遷移先と読み込み完了を確認した。
- Home履歴のキングダム→Group→第885話の「続きを読む」→Back→一覧を6回スワイプ→Back。アプリデータの削除・ログアウトは行っていない。
- 計測中はビルドや解析を同時実行していない。GIGAのCargo.toml / Cargo.lockは復元し、最後に計測コードのない既存最適化APKへ戻した。

**以下はプロファイラー動作中の観測値であり、通常実行の性能値ではない。** 前回のプロファイラーなしp95とは比較しない。ホスト負荷、JIT、キャッシュ、非同期データ到着のタイミングでも変動する。特に初回の画面遷移と読み込み完了は別のイベントである。

## プロファイラーを止めた対照測定

同じ計測用APKを再起動し、ATraceが無効・simpleperfも停止した状態で5操作を測定した。PIDは29297。gfxinfo p95はGroup表示81ms、Reader表示81ms、Reader→Group250ms、スクロール450ms、Group→Home150msだった。

この対照でも遅延が残り、記録中より悪い値も出た。プロセス再起動・JIT・非同期ロード・ホスト環境の変動が混ざるため、この比較からプロファイラーの追加負荷を何%と算出することはできない。前回の数値との回帰判定にも使わない。**絶対時間の一般化ではなく、遅い更新中の処理内訳と、2回のトレース・CPUサンプリングで一致する負荷経路を今回の結論とする。**

## 遅い更新と、表示フレームの対応

各操作の最も長い `driver.tick` を選び、その区間を包含するGIGAのFrameTimelineと対応させた。2回のPerfetto計測を併記する。tickはAndroidからRustへ入って戻るまでで、同期的なAndroidコールバックも含む。アプリのフレーム全体とは異なる。

| 操作 | Rust tick（1回目 / 2回目） | tick中のCPU Running | 対応するフレーム時間 |
| --- | ---: | ---: | ---: |
| Groupを開く・一覧反映 | 659.6 / 501.0ms | 649.3 / 498.3ms | 712.6 / 526.7ms |
| Readerを開く | 126.3 / 134.0ms | 126.1 / 133.5ms | 137.7 / 145.0ms |
| Reader→Group | 125.6 / 127.8ms | 121.7 / 124.6ms | 144.3 / 140.6ms |
| チャプター一覧をスクロール | 161.8 / 209.0ms | 161.8 / 208.7ms | 184.5 / 232.2ms |
| Group→Home | 176.4 / 113.3ms | 175.0 / 113.1ms | 209.3 / 133.9ms |

これら10フレームすべてに `App Deadline Missed` が付いている。区間集計は入れ子を含むため、次の表の列を単純に合計してはいけない。

| 最も長いtick内の処理 | Groupを開く | Readerを開く | Reader→Group | スクロール | Group→Home |
| --- | ---: | ---: | ---: | ---: | ---: |
| Taffy / LayoutTreeの計算 | 163.9 / 159.2ms | 5.3 / 6.4ms | 70.7 / 82.9ms | 105.5 / 112.7ms | 0.8 / 1.4ms |
| そのtickのレイアウト計算回数 | 6 / 6回 | 1 / 1回 | 4 / 4回 | 4 / 4回 | 1 / 1回 |
| Host文字計測バッチ | 94.0 / 74.5ms | 0回 | 5.0 / 2.9ms | 5.9 / 14.1ms | 0回 |
| スタイル反映 | 70.9 / 53.5ms | 22.3 / 29.4ms | 9.2 / 11.4ms | 0.0 / 18.1ms | 約0ms |
| Android側への同期反映 | 97.6 / 40.9ms | 62.2 / 50.9ms | 9.3 / 3.6ms | 8.9 / 22.8ms | 169.1 / 107.5ms |

### スクロール

6スワイプの区間全体では、Rust tickが48回だった1回目にレイアウト計算が120回、文字計測バッチが84回走った。2回目もレイアウト101回・文字計測69回。文字計測そのものより、その前後で繰り返すレイアウト計算が大きい。

1回目のTaffy / LayoutTree計算合計は1891ms、Host計測160ms、スタイル用コピー315ms、Android反映241ms。Listエントリーの生成は54回で212ms。これらは操作全体の合計で、1フレームの値ではない。

遅いtickの中には行生成が0回でも4回のレイアウト計算が走るものがある。そのため「行を作る処理だけ」を速くしても、残るスクロール遅延は解消しない。

simpleperfでは `LayoutTree::compute` を44.0%、Taffyの `determine_hypothetical_cross_size` を25.7%、`MeasurementCoordinator::apply_batch` を6.9%のUIスレッドサンプルで観測した。これらは呼び出し先を含み、重なる。

`MeasurementCoordinator::apply_batch` は応答バッチごとに自身をcloneし、応答を適用してから置き換えている（`crates/whisker-engine/src/measurement.rs:398`）。レイアウトの複数パスに加え、このトランザクション用コピーも確認対象になる。

### Groupの一覧読み込み完了

1回目は操作から約3.18秒後のtickが最長だった。最長tickではListエントリーを30回生成し、文字計測バッチ5回とレイアウト6回を同期処理した。2回目も同じ回数だった。前回の3秒程度の観測窓だけでは、非同期データ到着後のこの更新を取り逃がす可能性がある。

1回目のtick全体660msのうち、runtime.layout区間が339ms。内側のレイアウト計算164msとHost計測94msに加え、計測結果の反映や通知などがある。List行生成区間は合計63msで、スタイル反映等と一部重複する。

ここで計数しているのは画面全体の `new_mounted_entry` 呼び出しであり、すべてを特定のListのチャプター行と断定するものではない。「全886件の行を一度に生成した」という証拠はない。

### Readerの表示と戻り

Reader表示の最長tickではListエントリー生成21回、Android反映51〜62msがある。操作全体では、読み込み中も含めたスタイル反映で状態コピーが繰り返される。コピーは平均6.6〜6.9ms / 回。

Reader→Groupの最長tickはレイアウト計算4回・文字計測3回。操作全体でもListエントリー生成は各計測1回なので、この記録では大量の行再生成が支配的ではなかった。

### Group→HomeのAndroid側削除

1回目の最長tickは176msで、そのうち169msが `MobileFrameSink` からAndroidに渡す同期コールバックだった。2回目も113ms中108ms。パケットのRust→C表現への変換は1回目0.027msで、主な負荷ではなかった。

simpleperfのUIスレッドサンプルでは以下を観測した（inclusive）：

- `HostScene.commit`：24.2%
- `HostScene.validateStagedFrame`：12.4%
- `HostScene.deleteNode`：7.2%
- `HostScene.isStagedDescendant`：6.7%
- `HostScene.isDescendant`：6.2%

`platforms/android/runtime/src/main/kotlin/rs/whisker/runtime/scene/HostScene.kt:192` の削除検証は既存ノード全体を走査して親チェーンをたどる。`:564` の実際の削除も全ノードから子孫を探す。多数の削除命令に対し、この走査を繰り返す構造になっている。子リストを使った部分木の列挙、削除命令のまとめ方の見直しが具体的な改善候補。

## simpleperf：操作全体のUIスレッドCPUサンプル

| 操作 | UIサンプル数 | SurfaceEngineのコピー | 旧SurfaceEngineの破棄 | Android HostScene.commit |
| --- | ---: | ---: | ---: | ---: |
| Groupを開く | 1158 | 19.0% | 8.8% | 4.1% |
| Readerを開く | 260 | 19.2% | 8.5% | 12.7% |
| Reader→Group | 197 | 17.3% | 8.6% | 9.6% |
| スクロール | 1878 | 8.5% | 4.2% | 4.8% |
| Group→Home | 194 | 17.5% | 9.8% | 24.2% |

サンプリングは各呼び出しの厳密な時間計測ではなく、関数がスタック上に存在したCPUサンプルの比率。特に短い操作は標本が少なく、深いdebugスタックの展開やインライン情報にも限界がある。Perfettoとは別実行で、読み込み待ちの長さも異なるため、両者の操作全体の合計時間を直接比較しない。

## 次の修正方針

1. **スクロールの計測・再レイアウトを減らす。** 新規行の幅制約と文字計測がどう変化して複数パスになるかを追い、キャッシュを再利用できる条件と不要なinvalidateを特定する。4回を固定1回に制限するだけでは正しさを壊すため行わない。
2. **描画状態の全体コピーと破棄を減らす。** 既存の原子的な更新とエラー時rollbackを保ちながら、描画だけの更新経路や変更対象の差分に限定したcommitを設計する。計測コーディネーターのバッチ用コピーも含める。
3. **Androidの削除処理を部分木単位にする。** 検証と実適用の両方で、削除ごとの全ノード走査を避ける。Group→Homeの停止に直接対応する。

今回は計測と原因の絞り込みまでで、この3項目の製品修正は行っていない。

## 成果物と再現方法

- `summary.json`：10本のPerfetto集計、最長tick内訳、対応するFrameTimeline、5本のCPUサマリー。
- `scripts/`：計測用crateの作成、ビルド、操作別capture、シンボル解決、SQL集計、HTML作成。
- ローカル作業ディレクトリ：`/tmp/giga-detailed-profile/`。各 `perfetto-*` に `timeline.pftrace`、各 `simpleperf-*` に `cpu.data`。同じディレクトリに非stripライブラリとシンボル対応表がある。
- 永続保存したHTMLとrawデータ：`/Users/itome/.codex/visualizations/2026/09/05/01a070fd-cf10-76a2-9a0a-57231fbd2e81/android-detailed-profile/`。`report.html`、`traces/`、`cpu/`、シンボル対応表を保存。
- HTMLレポートは操作別の区間表、遅いtickの内訳、CPUサンプリングのフレームグラフを含む。関数名での強調、クリックによる拡大が可能。データはHTML内に埋め込み、外部へ送信しない。

再計測は該当画面を表示して `python3 /tmp/giga-detailed-profile/capture.py perfetto scroll perfetto-scroll-new` のように実行する。`simpleperf` も同じ引数形式。座標は今回のエミュレーター専用で、開始画面と読み込み完了の確認が必要。計測用APKを再インストールして使う。通常の既存最適化APKにはATrace区間を入れていない。

計測開始前に、独立した計測用crateからATraceを呼ぶ構成にした。Whisker CLIの最終リンクが `libandroid.so` を取り込まなかったため、一時ヘルパーは `dlopen` / `dlsym` でトレース関数を取得する。製品crateの `forbid(unsafe_code)` は維持した。この接続を直す前の起動失敗は計測結果に含まない。
