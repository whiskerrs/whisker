# slime (chantoinc/slime) のエンジン機能インベントリ

調査対象: `/Users/itome/Projects/github.com/chantoinc/slime` (HEAD `7383a5e`, 2026-10-29, 52 commits)。パスはすべてこのリポジトリからの相対パス。

根拠の表記:
- **【コード】** ソースを読んで確認した
- **【実測】** この調査でファイルやデータを数えた (`git ls-files` + `wc -l`、`du`、バイナリのヘッダーの解析、既存の `asset-pipeline info` の出力)
- **【文書】** README / CLAUDE.md / docs の記載のみで、コードや動作では未確認。性能値はすべてこの区分で、この調査では再計測していない

`docs/ENGINE_HISTORY.md` の M0〜M4 と渋谷の記録は、前身プロジェクト openworld での開発記録 (同文書冒頭)。数値の多くは現在の佐世保ワールドの値ではない。

## 各エンジン領域で何が実装されているか

### Takeaway
ランタイムは「wgpu + 自前」のブラウザ向け WebGPU エンジンで、描画とストリーミングが突出して作り込まれている (GPU 駆動カリング、ディファード、TAA、GTAO、SSR、大気 LUT、PCSS など)。一方、物理、ナビゲーション、ネットワーク、セーブは無いか最小限で、音声と HUD はエンジンの外 (ページの JS / HTML) にある。

### Cited Findings

#### レンダリング
- 依存は wgpu 30.0.1 のみ。バックエンドは webgpu / metal / vulkan / dx12 で、GLES / WebGL は明示的に除外 — 【コード】`Cargo.toml`、`crates/renderer/Cargo.toml`、`crates/renderer/src/lib.rs` の `backends()`
- ネイティブでも WebGPU 既定 limits で動かす。オプション feature は `TIMESTAMP_QUERY`、`TEXTURE_COMPRESSION_BC`、`TEXTURE_COMPRESSION_ASTC` だけで、ある場合のみ有効化 — 【コード】`crates/renderer/src/lib.rs:140,294-296`
- フレームの順序: cull1 (compute) → G バッファ phase 1 → Hi-Z → cull2 → G バッファ phase 2 → shadow → AO → lighting → TAA → post — 【コード】`crates/renderer/src/profiler.rs` の `PASS_NAMES`、`crates/renderer/src/lib.rs:946-1094`
- GPU 駆動: 2 フェーズのオクルージョンカリング (視錐台 + Hi-Z) と間接描画。multi-draw indirect は不使用 — 【コード】`crates/renderer/src/cull.rs`、`hiz.rs`、`shaders/cull.wgsl`、`props_cull.wgsl`、方針は【文書】`CLAUDE.md`
- メッシュの建物はセル単位の視錐台カリングで描き、建物ごとの GPU カリングはしない — 【文書】`ENGINE_HISTORY.md` 渋谷の節、実装 `crates/renderer/src/city_mesh.rs`。佐世保では `buildings` の箱が 0 個 (【実測】) なので、Hi-Z の経路を主に通るのは配置アセットと HLOD と推定
- ディファード: G バッファは色+粗さ (rgba8 sRGB)、八面体法線+金属度+発光 (rgba16float)、速度 (rg16float) — 【文書】`docs/ENGINE_HISTORY.md` / 実装ファイルは【コード】`crates/renderer/src/gbuffer.rs`
- 影: カスケード 3 枚、2048²、範囲 20 / 80 / 300m — 【コード】`crates/renderer/src/shadows.rs:8-12`。PCSS と contact shadow — 【文書】`ENGINE_HISTORY.md`、実装先 `shaders/lighting.wgsl`
- ライティング: GGX PBR とアニメ調の切り替え、点光源は最大 16 個 — 【コード】`crates/renderer/src/lighting.rs:34`。セル調と輪郭線はキャラ限定 — 【文書】`docs/GAME_DESIGN.md`
- 画面空間系: TAA + アップスケール (`taa.rs`)、GTAO (`ao.rs`)、SSR (`lighting.wgsl` 内)、画面空間 GI (`gi.rs`)、ボリュメトリック光 (`volumetric.rs`)、自動露出・ブルーム・トーンマップ・モーションブラー (`post.rs`, `exposure.wgsl`) — 【コード】ファイルの存在、内容は【文書】`ENGINE_HISTORY.md`、`GAME_DESIGN.md`
- 空: Hillaire 2020 方式の大気 LUT、雲は厚みのない 2D 層 — 【文書】`ENGINE_HISTORY.md`、実装 `crates/renderer/src/atmosphere.rs`
- 前方描画の半透明: スライムの体は G バッファに入れず、背景を写し取って屈折させる。頂点シェーダーで 2 万三角形の球を変形 — 【文書】`GAME_DESIGN.md`、実装 `crates/renderer/src/slime_body.rs` (1,660 行)、`shaders/slime_body.wgsl` (1,217 行)
- 手続き的マテリアル: 建物の外壁、看板、草地の地面はシェーダーで描く — 【文書】`ENGINE_HISTORY.md`、`GAME_DESIGN.md`、実装 `shaders/city_buildings.wgsl`、`terrain.wgsl`
- 画質制御: 動的解像度 0.5〜1.0 (0.1 刻み)、初期画素数の上限 220 万、GPU 目標 13ms — 【コード】`crates/game/src/resolution.rs`。段階 High / Medium / Low は下げるだけで戻さない — 【コード】`crates/game/src/quality.rs`
- 描画パイプライン生成の呼び出しは 33 か所 — 【実測】`crates/renderer/src` の `create_render_pipeline` / `create_compute_pipeline` の grep

#### アニメーション
- 骨格アニメ: TRS の補間、クリップのサンプリング、スキニング行列、ルートモーションを自前実装 — 【コード】`crates/engine/src/skeleton.rs` (230 行)
- GPU スキニング、関節は最大 64 — 【コード】`crates/renderer/src/dynamic.rs:13`
- ブレンド: 待機・歩き・走りの速度合成、技の重ね合わせ、上半身と下半身のマスク、被弾の加算レイヤー — 【コード】`crates/game/src/player.rs`、`creature.rs`
- 焼き込みアニメの群衆: 30fps で焼いたモーションを 1 回の描画でインスタンス描画 — 【文書】`ENGINE_HISTORY.md`、実装 `crates/renderer/src/crowd.rs`
- 二次運動: 髪と帯はボーンを足さず、ばねの重りのずれ × 頂点ごとの揺れやすさで動かす — 【文書】`GAME_DESIGN.md`、実装 `game::Sway`、`dynamic.wgsl`
- IK とモーフターゲットは無し (`\bIK\b`, `morph_target`, `blend shape` は `crates` と `tools/asset-pipeline/src` で grep ヒットなし。`morph` のヒットは地形の LOD 関連のファイルのみで、キャラのものではない) — 【コード】

#### 物理・当たり判定
- 物理エンジンなし。`rapier` / `physx` などへの依存はゼロ — 【コード】`Cargo.toml` 群と grep
- 判定の形は水平面の「回転つき箱」と「円」の 2 種類で、円柱の押し出しと線分レイキャストだけ — 【コード】`crates/game/src/world.rs:11-24,109,176`
- 重力とジャンプは手書きの速度積分 — 【コード】`crates/game/src/player.rs` (`GRAVITY`, `JUMP_SPEED`)
- 地形の高さは CPU 側のタイルの写しから引く — 【コード】`crates/engine/src/terrain/stream.rs:238`。未読込の場所には当たり判定がない — 【コード】`world.rs` 冒頭のコメント
- 戦闘判定: 球と相手の縦線分の距離 — 【文書】`GAME_DESIGN.md`、実装 `crates/gameplay/src/combat.rs:299`。弾は移動線分で判定 — 【文書】同、実装 `projectile.rs`
- 車はアーケード式 — 【文書】`ENGINE_HISTORY.md`、実装 `crates/game/src/vehicle.rs` (142 行)

#### 音声
- Rust 側に音声コードなし。すべてページの Web Audio — 【コード】`apps/web/index.html:262-292`
- 構成: マスター + コンプレッサー + 畳み込みリバーブ (インパルス応答は乱数で生成)。SFX は mp3 をデコードして再生、BGM と環境音はオシレーターで生成 — 【コード】同上
- ゲームは効果音のきっかけを番号つきで HUD の JSON に入れ、ページが鳴らす — 【文書】`GAME_DESIGN.md`、【コード】`crates/game/src/slime.rs` の JSON 生成 (`sounds`)
- 3D 定位: `createPanner` は grep でヒットせず、位置音響なし — 【コード】`apps/web/index.html`

#### 入力
- キーボードとマウスは web-sys で直接 — 【コード】`apps/web/src/lib.rs:657-685`
- 指とゲームパッドはページの JS で読み (`navigator.getGamepads`)、wasm の `touch_stick` / `touch_look` / `touch_button` へ渡す — 【コード】`apps/web/index.html:464`、`apps/web/src/lib.rs:549-567`
- デスクトップは winit — 【コード】`apps/desktop/Cargo.toml`
- HUD のキー案内は最後に使った入力に追従。全画面と横向きロックあり — 【文書】`README.md`、【コード】`apps/web/index.html:557`

#### カメラ
- 基本はヨーとピッチのカメラ、fov 60°、near 0.5 — 【コード】`crates/engine/src/camera.rs`
- 肩越しの追従カメラ、ボス戦時の引きと向き替え、揺れ — 【コード】`crates/game/src/lib.rs:1308-1388`、`feedback.rs:192`。必殺技の演出カメラ — 【文書】`GAME_DESIGN.md`
- めり込み防止 — 【文書】`ENGINE_HISTORY.md` (カメラは箱とだけ当たる)

#### パーティクル・VFX
- CPU シミュレーション + インスタンス描画で、GPU パーティクルではない — 【コード】`crates/game/src/effects.rs` (1,734 行)、`boss_fx.rs` (685 行)、`crates/renderer/src/slime_body.rs` の `set_water_effects` 経由
- 上限: 水滴 384、輪 48、スプライト 1,280、三日月 48、技の予告 8、軌跡 64 サンプル、剣 16、霧 700、ボス破片 520 — 【コード】`crates/renderer/src/slime_body.rs:169-187`、`crates/game/src/effects.rs:173`、`boss_fx.rs:71`
- テクスチャのフリップブックは試したが不採用で、魔法陣やきらめきはシェーダー描画 — 【文書】`GAME_DESIGN.md`、`CLAUDE.md`
- ヒットストップ、スロー、揺れ、閃光 — 【コード】`crates/game/src/feedback.rs`

#### UI / HUD
- HUD はエンジンで描かず、ページの HTML/CSS。ゲームが毎フレーム `window.__hud` に JSON を書き、ページが描く — 【コード】`apps/web/index.html:55,582`、【文書】`GAME_DESIGN.md`
- ダメージ数字やロックオン枠も、ワールド座標を画面に写した値をページが描く — 【文書】`GAME_DESIGN.md`
- つまりレンダラーにテキスト描画や UI システムはない — 【コード】`crates/renderer/src` のファイル一覧にそれらしいモジュールなし (推定を含む)
- 開発用の計測表示 (F3) — 【コード】`crates/game/src/stats.rs` (418 行)

#### アセットパイプラインと形式
- 独自バイナリ形式: `.owa` (LOD 連鎖 + テクスチャ + インポスター)、`.owk` (スキン付きキャラ + 骨格 + クリップ)、`.owi` (後読みテクスチャ) — 【コード】`crates/engine/src/asset.rs`。`.owf` は `facade.owf` 1 種 3 個 — 【実測】(外壁用と思われるが中身は未確認)
- glTF はランタイムで読まず、オフラインで変換 — 【コード】`tools/asset-pipeline/Cargo.toml`、`CLAUDE.md`
- 変換処理: glTF 読み込み → 実寸化 → meshoptimizer で LOD → xatlas で UV 再展開 → BVH 上のレイで焼き直し → インポスター焼き (wgpu) → BC7 / ASTC 圧縮 — 【文書】`ENGINE_HISTORY.md`、実装 `tools/asset-pipeline/src/{mesh,rebake,impostor,compress}.rs`
- テクスチャは RGBA8 / BC7 / ASTC 4x4 を事前生成して配り、端末で選ぶ (Basis Universal は不採用) — 【文書】`ENGINE_HISTORY.md`、【コード】`crates/renderer/src/lib.rs:441-447`
- 四足獣の自動リグ: 形から 19 関節を推定し、動きは手続き生成 — 【文書】`GAME_DESIGN.md`、実装 `tools/asset-pipeline/src/quadruped.rs` (788 行)
- 実在都市の取り込み: PLATEAU CityGML + OSM → 地形、建物、道路、都市設備、看板、交通網 — 【コード】`tools/city-import/src/*`
- AI 生成: Meshy (3D)、ElevenLabs (効果音) — 【コード】`tools/meshy/generate.py`、`tools/sfx/generate.py`
- シェーダーのホットリロードやインクリメンタルなアセット DB はドキュメントに記載なし

#### ストリーミング・LOD
- 地形: 8,192m 四方、四分木 7 レベル、5,461 タイル (131² x 16bit = 34KB/枚)、GPU アトラス 576 枠、LRU 追い出し、低レベルは固定保持 — 【コード】`crates/engine/src/terrain/format.rs`、`stream.rs`、`crates/renderer/src/terrain.rs:12-14`、【実測】レベルごとのファイル数 1 / 4 / 16 / 64 / 256 / 1,024 / 4,096
- 都市: 256m セル (32x32)、GPU に固定 96 枠、1 フレーム 4 セルまでアップロード — 【コード】`crates/engine/src/cells.rs:30-32`、`crates/game/src/lib.rs:55-59`
- セルは 2 段階 (簡略版 `city/lod1/` を先に読み、近くなら詳細版へ差し替え)。読込範囲 900m、詳細 160m — 【コード】`cells.rs` 冒頭のコメント、値は【文書】`ENGINE_HISTORY.md`
- 移動先の先読み — 【コード】`crates/game/src/lib.rs` の `PREFETCH_SECONDS`
- 配置アセットの LOD: 画面誤差 1px 基準 (Medium 1.5、Low 2.0)、遠景はインポスター、影は 2〜4 段粗い LOD — 【コード】`crates/game/src/quality.rs`、【文書】`ENGINE_HISTORY.md`
- HLOD: 常駐の箱 — 【コード】`cells.rs` 冒頭のコメント
- 段階読込: 起動時は軽い主人公と剣と HLOD のみ (gz 約 2MB)、残りは遅延読込し、霧の縁で隠す `reveal` — 【文書】`ENGINE_HISTORY.md`、実装 `crates/game/src/reveal.rs`、`apps/web/src/lib.rs` の `load_late`
- データは gzip で配り、`DecompressionStream` で展開 — 【文書】`ENGINE_HISTORY.md`、【コード】`deploy/build.sh`
- メッシュレットは検討して見送り — 【文書】`ENGINE_HISTORY.md`「M3 で見送ったもの」

#### シーン・エンティティモデル
- ECS なし。汎用シーングラフもなし。`hecs` / `bevy` / `specs` / `legion` への言及なし — 【コード】grep
- 種類ごとの具象構造体と `Vec`: `SlimeGame` が `monsters`、`projectiles`、`strikes` などを持つ — 【コード】`crates/game/src/slime.rs:154-201`
- 戦闘は `Actor` の配列で解決 — 【コード】`crates/gameplay/src/combat.rs:299`
- レンダラーは `ObjectId` を発行する保持型 API (`add_skinned_object`, `set_object`, `set_joints`) — 【コード】`crates/renderer/src/lib.rs:545-567`
- ワールド定義は 1 行 1 項目の `world.txt` — 【コード】`crates/engine/src/world.rs`、【実測】`assets/sasebo/world.txt`

#### ゲームループ
- 可変ステップ。`frame(now_ms)` が dt を 100ms でクランプし、時間スケール (ヒットストップ、スロー) を掛けて更新。固定タイムステップは grep でヒットせず — 【コード】`crates/game/src/lib.rs:903-945`
- Web は `requestAnimationFrame` — 【コード】`apps/web/src/lib.rs:823-833`
- 時刻はアプリから注入 (`std::time::Instant` は wasm で panic) — 【文書】`CLAUDE.md`、【コード】`lib.rs` の `self.clock`
- Web はシングルスレッド。`thread::spawn` はデスクトップのファイル読込だけ — 【コード】grep、`apps/desktop/src/main.rs:362`。Wasm スレッドは「未着手の基盤」 — 【文書】`ENGINE_HISTORY.md` 末尾
- アップロードは時間予算つき — 【コード】`crates/game/src/lib.rs` (`upload_budget_ms`)

#### AI・ナビゲーション
- 単純な状態判断: 縄張り内のうろつき、発見距離 14m (ボス 30m)、離脱距離 40m (ボス 70m)、射程内で攻撃 — 【コード】`crates/gameplay/src/ai.rs:1-35`
- ナビメッシュと経路探索なし (`navmesh`, `pathfind`, `astar` は grep でヒットせず)。敵は直進 — 【コード】同上 (直進は `Intent` からの推定を含む)
- 交通: OSM から作ったグラフ上を歩行者と車が動き、信号に従う。カメラ周りの最大 360 人 / 90 台 (描画上限 512 / 256) — 【コード】`crates/engine/src/traffic.rs`、`crates/game/src/traffic.rs` (830 行)、`crates/renderer/src/crowd.rs:15-16`、人数は【文書】`ENGINE_HISTORY.md`
- 160m より遠い敵は止める — 【コード】`crates/game/src/slime.rs:97` (`ACTIVE_RANGE`)

#### ネットワーク
- オンライン同期なし。`WebSocket` / `WebTransport` / `WebRTC` は grep でヒットせず — 【コード】
- PvP は純粋なルール (拠点占領) のみ — 【コード】`crates/gameplay/src/pvp.rs`、【文書】`GAME_DESIGN.md`「オンラインの同期はない」
- 通信はアセット HTTP 取得のみ。開発用配信サーバーは自前の `TcpListener` — 【コード】`tools/terrain-gen/src/main.rs:16,127`

#### セーブ
- なし。`localStorage` / `IndexedDB` / `fn save` はいずれも grep でヒットせず — 【コード】

#### ツール・デバッグ
- GPU パス単位の計測 (`timestamp-query`、非同期読み戻し) — 【コード】`crates/renderer/src/profiler.rs`
- ページへの状態公開 `window.__stats` / `__slime` / `__cpu_ms`、URL パラメータ多数 (`?quality=`, `?cam=`, `?slow=`, `?diag=1` など) — 【文書】`CLAUDE.md`
- `?diag=1` は端末のシェーダーコンパイル失敗行を二分探索 — 【文書】`CLAUDE.md`、【コード】`apps/web/src/lib.rs:544`
- フリーカメラ、オートパイロット、ボスのデバッグキー — 【コード】`crates/game/src/autopilot.rs`、`lib.rs:1281`、【文書】`README.md`
- テスト 106 件 (主にゲームロジック) — 【実測】`#[test]` の数: engine 24、renderer 5、gameplay 25、game 33、asset-pipeline 8、city-import 11
- エディタ (シーン、マテリアル、アニメ) はリポジトリに存在しない — 【実測】`crates/`、`apps/`、`tools/` の一覧

#### プラットフォーム
- 主: Web (WebGPU 必須、WebGL フォールバックなし)。公開先 `https://slime.chanto.games/` — 【コード】`crates/renderer/src/lib.rs`、`deploy/wrangler.jsonc`
- 開発用ネイティブ (winit + Metal / Vulkan / DX12) — 【コード】`apps/desktop/`
- モバイルはブラウザ経由のみで、iOS / Android のネイティブアプリは無い — 【実測】`apps/` は `desktop` と `web` のみ
- Safari 26 (iOS 含む) は方針上の第一級対象 — 【文書】`CLAUDE.md`。ただし「iOS 実機での確認手順」は未着手、各節に「Safari では未計測」 — 【文書】`ENGINE_HISTORY.md`
- Android の Pixel 10 (PowerVR) には個別対応あり (簡易外壁) — 【コード】`apps/web/src/lib.rs:242`、`crates/renderer/shaders/gbuffer.wgsl:30`、commit log

### Inferences
- 「このゲーム規模を作れる」の要件は、描画とストリーミングでは高水準だが、物理とネットワークでは低水準。Whisker のエンジン機能で最初に問われるのは、物理エンジンの有無より GPU 駆動描画、段階読込、オフラインのアセット変換だと考えられる。
- HUD が HTML/CSS に逃げていることは Whisker に直接関係する。ブラウザの役割 (UI、音声、入力の一部、gzip 展開) を、ネイティブでは別の手段で埋める必要がある。Whisker の UI 層が HUD を担える可能性がある一方、音声は新規に必要になる。
- `window.__hud` の JSON 受け渡しは、フレームごとのゲーム→UI のブリッジの最小例で、Whisker では signal 更新の頻度と負荷が設計上の論点になりそう。

### Gaps
- デカールは `crates/renderer/src/slime_body.rs` と `slime_body.wgsl` に語のヒットがあり、濡れ跡の VFX と思われるが、汎用のデカールシステムかどうかは読んでいない。
- 水 (川と池) の描画はシェーダー中心と読めるが、実装箇所まで追っていない (`GAME_DESIGN.md` の記載のみ)。
- `.owf` の中身は未確認。

## 手書きとライブラリの内訳、および欠けている領域

### Takeaway
ランタイムの外部依存は事実上 wgpu とブラウザバインディングだけで、数学からアニメまで手書き。ライブラリが使われるのはオフラインツールに限られる。これは `CLAUDE.md` の明文の方針でもある。

### Cited Findings
- 方針: 「wgpu 以外の依存を追加するときは、自前で書くのが現実的でない理由を説明できること」 — 【文書】`CLAUDE.md`
- ランタイムの依存:
  - `engine`: 依存ゼロ — 【コード】`crates/engine/Cargo.toml`
  - `gameplay`: `engine` のみ — 【コード】`crates/gameplay/Cargo.toml`
  - `game`: 内部クレートのみ — 【コード】`crates/game/Cargo.toml`
  - `renderer`: `wgpu`, `engine` — 【コード】`crates/renderer/Cargo.toml`
  - `apps/web`: `wgpu`, `wasm-bindgen`, `js-sys`, `wasm-bindgen-futures`, `web-sys` — 【コード】`apps/web/Cargo.toml`
  - `apps/desktop`: `wgpu`, `winit` — 【コード】`apps/desktop/Cargo.toml`
- ツールの依存:
  - `asset-pipeline`: `gltf`, `meshopt`, `image`, `xatlas-rs-v2`, `rayon`, `intel_tex_2`, `ctt-astcenc`, `wgpu` — 【コード】`tools/asset-pipeline/Cargo.toml`
  - `city-import`: `quick-xml`, `earcutr`, `rayon`, `serde_json`, `meshopt` — 【コード】`tools/city-import/Cargo.toml`
  - `terrain-gen`: `engine` のみ — 【コード】`tools/terrain-gen/Cargo.toml`
- 手書きの主なもの: 数学 (`crates/engine/src/math.rs`, 356 行)、骨格アニメ、JSON 出力 (`slime.rs` の `struct Json`)、バイナリ形式、当たり判定、HTTP 開発サーバー — 【コード】
- Cargo.lock のパッケージ総数は 313 (大半は wgpu とツール由来) — 【実測】

| 領域 | 状態 | 実装 |
|---|---|---|
| 物理エンジン | **無し** | 箱と円の押し出しのみ |
| ECS / シーングラフ | **無し** | 具象構造体 + `Vec` |
| ナビメッシュ / 経路探索 | **無し** | 直進 AI、交通のみグラフ |
| ネットワーク同期 | **無し** | PvP ルールのみ |
| セーブ | **無し** | — |
| エンジン内 UI / テキスト | **無し** | DOM に委譲 |
| エンジン内音声 / 3D 定位 | **無し** | Web Audio に委譲、定位なし |
| エディタ | **無し** | URL パラメータとデバッグキー |
| マルチスレッド (Web) | **無し** | 未着手 |
| ネイティブモバイル | **無し** | ブラウザ経由のみ |
| 描画、ストリーミング、アセット変換 | **厚い** | 自前 + wgpu + オフラインツール |

### Inferences
- 依存を絞っているのは主に wasm サイズと配布の都合と読める (最適化後の wasm は 1.1MB)。ネイティブ前提の Whisker では同じ制約は弱く、物理 (rapier など) や音声クレートの採用余地はより大きい。ただしこのゲームの規模なら物理エンジンは必須ではない。
- ブラウザから「無料で」得ているもの (Web Audio、DOM、fetch、DecompressionStream、Gamepad API) はネイティブ移植時に代替が必要。この棚卸しで最も見落としやすい点と考える。

### Gaps
- 依存最小化の動機がサイズにあるという明記はない (`CLAUDE.md` は方針のみで理由は書いていない)。

## 具体的な規模

### Takeaway
コードは Rust 約 3.6 万行 + WGSL 約 5.6 千行。配布データは 476MB / gzip 185MB で約 5,650 ファイル。実データの佐世保は配置 7,409 個、HLOD の箱 13,591 個だが、エンジン自体は前身で 10〜14 万インスタンスの GPU カリングを実証した記録がある。

### Cited Findings

#### コード (【実測】コメント込みの行数)

| クレート | Rust | WGSL | テスト |
|---|---:|---:|---:|
| `crates/engine` | 3,838 | 0 | 24 |
| `crates/renderer` | 9,812 | 5,543 (25 ファイル) | 5 |
| `crates/gameplay` | 2,063 | 0 | 25 |
| `crates/game` | 10,562 | 0 | 33 |
| `apps/desktop` | 417 | 0 | 0 |
| `apps/web` | 986 (+ `index.html` 805) | 0 | 0 |
| `tools/asset-pipeline` | 4,005 | 45 | 8 |
| `tools/city-import` | 4,105 | 0 | 11 |
| `tools/terrain-gen` | 307 | 0 | 0 |
| **合計** | **36,095** | **5,588** | **106** |

ランタイムのみ (tools を除く) は Rust 27,678 行 + WGSL 5,543 行 + HTML/JS 805 行。

最大のファイル: `crates/game/src/lib.rs` 2,067、`effects.rs` 1,734、`slime.rs` 1,721、`renderer/src/slime_body.rs` 1,660、`renderer/src/lib.rs` 1,192 行。最大のシェーダー: `slime_body.wgsl` 1,217、`lighting.wgsl` 701 行。

#### 配布物 (【実測】)
- wasm 1,108,713 バイト (最適化後)、JS グルー 81,792 バイト、`index.html` 65KB (`apps/web/dist`, `deploy/public`)
- ワールド `assets/sasebo`: シンボリックリンク込み 476MB、5,654 ファイル。gzip 後 `deploy/public/data` は 185MB。README の「約 460MB・5,600 ファイル」「約 180MB」と概ね一致
- 地形: 5,461 タイル、約 192MB (レベル 6 が 144MB)
- 都市セル: 詳細 47 ファイル 15.9MB、簡略 42 ファイル 4.7MB
- HLOD: 13,591 個の箱 (`proxies.bin` 434,920 バイト、ヘッダー 8 + 32 バイト/個)。渋谷の記録では全棟に箱を作るとあるので、佐世保の全建物数もこの程度と推定 (佐世保の取り込み経路は未読)
- 配置インスタンス: 7,409 個 (`city/index.bin` と詳細セルの両方で一致)。うち簡略版にも残るのは 1,448 個で、同じセルの別版なので足さない。`GAME_DESIGN.md` の「約 9000 個」とは差があり、文書とデータの不一致
- メッシュを持つ建物エントリ: 2,689 (看板も同じ欄を使う記録があり、棟数そのものとは限らない)。当たり判定の箱: 2,193。建物メッシュ 63,855 三角形 (詳細セル、道路は別)
- アセット (RGBA8 版): `assets/terrain/assets` 直下 33 ファイル、それに `bc7/`、`astc/` 各 58MB
- 配置アセット: forest_tree 15,906 → 454 三角形 (6 段)、cliff_rock 11,980 → 372 (6 段)、fern_clump 1,910 → 88 (5 段)
- 主人公: 30,782 三角形、24 関節、13 クリップ 985 フレーム、テクスチャ 1024²
- ボス: 41,343 三角形、19 関節、8 クリップ 495 フレーム
- 群衆: 2,480 三角形、24 関節、3 クリップ
- 効果音: mp3 24 個、524KB (`apps/web/sfx`)
- ソースアセット: 計 6.3GB (Meshy GLB 2.8GB、PLATEAU 3.5GB)。リポジトリ外 (git 管理外)。`/assets` は `.gitignore` 対象

#### 実行時の上限 (【コード】前節のパス参照)
- 地形 576 枠、地形インスタンス 8,192 (`terrain.rs:15`)、都市セル 96 枠、点光源 16、関節 64、歩行者 512、車 256
- GPU 上のセルの枠: 建物 26,112 / 配置 30,720 — 【文書】`ENGINE_HISTORY.md` M4 (当時の値)

#### 性能の記録 (すべて【文書】`ENGINE_HISTORY.md`、M4 Mac / Chromium 系 / release)
- M2: 建物 10 万棟、全カリングで 3.0ms、カリングなしで 8.4ms (800x975)
- M3: 配置 14.2 万個。LOD + インポスターで 4.9ms、LOD0 のみで 67.8ms (1.76 億三角形)
- M4: 車 140km/h、フリーカメラ 300m/s でカクつき 0、p99 約 17.6ms
- ライティング完成後: 渋谷の通りで 12〜14ms (1288x1570)
- 森の景色、2858x1764、内部解像度 1: High 43ms / Medium 25ms / Low 13ms。三角形は LOD 改善で 547 万 → 199 万 (内部解像度 0.5)
- 2858x1764 で内部解像度 0.5 でも GPU 22ms。解像度に依存しない負荷 (影、形状) が残る
- CPU 側は森の景色で約 2ms。重さはほぼ GPU
- 目標: 60fps (16.7ms)、GPU 予算 13ms — 【コード】`crates/game/src/resolution.rs`
- 起動時の読込: gz 約 2MB まで削減

#### 開発の規模感
- slime 自体の commit は 2026-10-22〜29 の 52 件で、エンジンは openworld からのコピー — 【実測】`git log`、【文書】`CLAUDE.md`
- `ENGINE_HISTORY.md` の日付は 2026-10-19〜26。10-26 の項目 (動的解像度、遅延読み込みなど) は内容からコピー後の slime での作業と読める。エンジンとゲーム全体で約 11 日 — 【文書】【実測】

### Inferences
- 「少なくともこの規模」の具体的な基準案: 8km 四方のストリーミング世界、配置 7〜8 千 (前身の実証は 10 万超のカリング)、3 万三角形の主人公と 4 万三角形のボス、スキン付き群衆数百、同時パーティクル 2〜3 千、描画パイプライン 30 超、配布データ 200MB 級。
- ゲームのコード (`game` + `gameplay` で約 1.26 万行) はレンダラーと同規模。うち VFX と見た目の演出の比重が高く、エンジンにはデータ駆動の VFX より「コードで書きやすい描画 API」が求められるかもしれない。
- データの大半は地形で、ゲーム固有のアセットは比較的小さい。

### Gaps
- モバイル実機の fps、メモリ、発熱の記録はリポジトリにない。Pixel 10 対応の commit はあるが数値は残っていない。
- 計測はほぼ M4 Mac のみ。Android / iOS でどの画質で 60fps になるかは不明で、**モバイルで成立する規模の根拠はこのリポジトリからは得られない**。
- 現在の佐世保ワールドでの GPU 時間は、森の景色の値以外まとまっていない。
- 同時に画面に出るパーティクルの実数は未計測 (上限のみ判明)。

## ENGINE_HISTORY から読み取れる「難しかったこと」

### Takeaway
難所はアルゴリズムより、(1) WebGPU とドライバーの差、(2) AI 生成アセットの品質、(3) データ量と起動時間、(4) 解像度に依存しない GPU コストと計測の難しさ、の 4 つに集中している。

### Cited Findings (すべて `docs/ENGINE_HISTORY.md`、一部 `CLAUDE.md`、commit log。【文書】)

#### プラットフォームとドライバー
- Pixel 10 などの PowerVR で詳細な外壁のパイプラインが生成できない (`VK_ERROR_UNKNOWN`)。分岐ごとに G バッファ書き出しが複製されるのが一因で、書き出しを 1 回にまとめ、それでも簡易外壁へ切替 — `crates/renderer/shaders/gbuffer.wgsl:30` (【コード】)、`CLAUDE.md`
- 一部の Android は条件付きの `requestAdapter` で取得失敗し、条件なしで再試行 — `crates/renderer/src/lib.rs:281` (【コード】)
- Chrome (Tint) は非一様な分岐内の `dpdx` を拒否するが naga は通す。ネイティブで通っても Web で壊れる
- wgpu は内部エラーで panic して文言を失うため、ページ側でエラーを受ける — `CLAUDE.md`
- iOS Safari のメモリ制限で落ちる前提で設計 — `CLAUDE.md`。Safari は `timestamp-query` が無い可能性があり、フレーム間隔で判断 — `crates/game/src/resolution.rs` (【コード】)
- 検証エラー: キャンバス 1x1 で bloom が同じテクスチャを読み書きし、フレームごと破棄。後に修正した commit あり — commit log「bloom のミップを 2 段以上に」
- **未解決**: 同じタブの再読込で描画が止まる現象。約 60 回の再現試験で原因不明
- **未解決**: 手動テストで一度だけの 416ms スパイク

#### AI 生成アセット
- UV が数千〜50 万個の断片になり、ミップで色が混ざって「迷彩」に見える。xatlas で展開し直しレイで焼き直す大きな工程が必要になった
- 継ぎ目を守ると簡略化が 3 段で止まる。`Permissive` と再展開で対処。群衆でも同じ問題 (1.2 万三角形で停止)
- 入力が重い: 街路樹 326MB、761 万三角形、変換時メモリ約 2.2GB
- テクスチャは色のみなので、粗さと金属度はアセット単位で一様
- Meshy のモーションは腕が A ポーズ寄り。自動リグは人型のみで、四足は自作 (`quadruped.rs`)。髪や帯の骨も無い
- インポスターとメッシュの見た目の差 (明るさ、密度)
- Codex 生成の VFX の絵は様式が合わず破棄 — `CLAUDE.md`

#### データ量と起動
- 非圧縮テクスチャ (1 アセット 5.6MB) がダウンロードと GPU を圧迫。BC7 / ASTC で 4 分の 1 に。ただし形式ごとにファイルが増え、メッシュも重複
- Cloudflare が `application/octet-stream` を圧縮しないので自前 gzip (459MB → 178MB)
- 起動 44MB → 29MB → gz 約 2MB。遅延読込、軽量版、`reveal` 演出の組み合わせが必要だった
- 高速移動時は HTTP/1.1 の同時 6 接続が頭打ちになる。親タイルで穴は開かない
- 読み込み失敗タイルを再取得しない課題が M1 に記載 (現状は未確認)

#### GPU コスト
- 見た目を足すと 4ms → 12ms。影が最大の増分 (8.5ms → 3.9ms へ絞り込み)
- 解像度を下げても効かない: M4 では 75% の効果が小さい。頂点処理 (影、形状) が支配的
- 影を粗い LOD にして 42〜50ms → 35ms
- 初版 TAA は 8ms で、サンプル数を減らして 1ms に
- 雲の影を画素ごとにノイズ計算すると 0.8ms。マップ参照に変更
- `sin` ベースの乱数で地面が 2 倍重い
- SSR は +0.8ms に対して効果がわずか

#### 計測の難しさ
- タイルベース GPU ではパス時間の合計が実際より大きい。フレーム全体で測る必要あり
- Mac は GPU クロックが下がるので、軽い設定の比較は差が出にくい。切替時に ±10ms 揺れる
- 非表示のペインでは rAF が止まり計測不能
- Metal の一部パスで時間が 0.00ms と出る (未調査)

#### 精度と時間
- 広いワールドで f32 のワールド行列では TAA の履歴がずれるため、カメラ相対で計算
- キャラの手足の速度は TAA に入っていない (当時の記載)

#### 見た目の試行錯誤
- 水の柱や光線は「作った感」が強く、全部飛沫に変更 — `GAME_DESIGN.md`
- 黒い影が屈折で映り込み、水色の影に変更 — `GAME_DESIGN.md`

### Inferences
- Whisker が Lynx 上のモバイルフレームワークとして wgpu を使う場合、PowerVR の件が示すように、モバイルドライバー差が最大のリスクになりそう。Web より Vulkan / Metal 直の方が回避手段は多いが、端末の種類は増える。
- タイルベース GPU (モバイル全般) では、ディファード + 多数の画面空間パスの構成はそのまま持ち込みにくい可能性がある。これは本リポジトリに実測がなく、外部の知見での検証が必要。
- アセット工程の工数はランタイムに匹敵する (ツール約 8.4 千行)。AI 生成アセットを前提にするなら、エンジン機能の一部と見なすべき。

### Gaps
- 上記の難所のうち、現在もコード上で残っているかは、明記のあるもの以外は確認していない (ドキュメントは時系列の記録で、以後修正された項目もありうる)。
- 開発工数 (人時) の記録はない。commit の日付からは短期間だが、AI エージェントの関与の度合いは不明。
