# Unreal Engine 5 / Unity 6 機能カタログ(比較の上限として、2026-10 時点)

凡例: 「Cited Findings」は今回のセッションで取得したページまたは検索結果に基づく。公式ドキュメント以外(フォーラム、報道、ベンダー)はその旨を明記する。今回裏付けられなかった項目は、一般知識(学習データ)として各節の Gaps に分離した。**Gaps の「未検証」項目は確定事実として扱わないこと。** 用語の括弧書き説明は筆者による補足。

## 現行バージョンとライセンスの基本(2026 年後半時点)

### Takeaway
Unreal は UE 5.8(2026-06-17 発表)が現行で、UE5 系の最終版となる予定。UE6 Early Access は 2027 年末が目標。Unity の Update 系現行は 6000.6.7f1(2026-10-21)、LTS 現行は 6.3 で、6.7 が beta、7.0 が alpha。課金モデルは対照的で、Unreal はゲームに売上ロイヤリティ、Unity は席(seat)課金。

### Cited Findings

**Unreal: バージョン**
- UE 5.8 は State of Unreal 2026(シカゴ、2026-06-17)で発表された — [wnhub/検索結果の報道](https://wnhub.io/zh/news/engines/item-51157); リリースノートは [Epic 公式](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-5-8-release-notes)
- 5.8 の後に公式の UE5 リリースを出す計画は現時点でないが、必要なら 5.9 の可能性は残す。UE5 の残りの変更は UE6 へマージされ、逆方向はない — [GamesBeat](https://gamesbeat.com/unreal-engine-6-will-combine-ue5-and-uefn-into-a-unified-engine-state-of-unreal/)
- UE6 は「UE5 + UEFN(Fortnite 向けエディタ)」の統合。Early Access は 2027 年末が目標、正式版はその 12〜18 か月後。公開開発ストリームが GitHub にあるが alpha ではないとされる — [GamesBeat](https://gamesbeat.com/unreal-engine-6-will-combine-ue5-and-uefn-into-a-unified-engine-state-of-unreal/)

**Unreal: ライセンス**(公式ライセンスページは直接取得で HTTP 403。以下は検索結果に出た同ページの抜粋と報道による)
- ゲーム: 製品ごとの生涯総収入のうち $1M を超える部分に 5% のロイヤリティ。Epic Games Store での売上はロイヤリティ対象外 — [Epic ライセンスページ(検索抜粋)](https://www.unrealengine.com/en-US/license)
- 2025-01-01 以降、Epic Games Store で同日発売するゲームは 3.5% に低減。同日発売しない場合は 5% のまま — [PocketGamer.biz](https://www.pocketgamer.biz/unreal-engine-royalty-fee-reducing-to-35-for-games-landing-on-epic-games-store-on-launch-day)
- 非ゲーム用途: 直近 12 か月の収入が $1M 超で、エンジンのコードをランタイムで第三者に提供するゲーム/アプリを作っていない商用ユーザーは、$1,850/席/年 — [Epic ライセンスページ(検索抜粋)](https://www.unrealengine.com/en-US/license); [Engadget(2024)](https://www.engadget.com/epic-will-charge-non-game-developers-1850-per-seat-to-use-unreal-engine-162015997.html)

**Unity: バージョン**
- リリース情報ページの最新は 6000.6.7f1(2026-10-21) — [Unity What's New](https://unity.com/releases/editor/whats-new)
- ドキュメントのバージョン選択では 6000.6 = Current、6000.3 / 6000.0 = LTS、6000.7 = Beta、7000.0 = Alpha、6000.5 / 6000.4 / 6000.2 / 6000.1 = Legacy — [Unity 6.6 Manual](https://docs.unity.com/zh-cn/engine/6000.6/manual)
- LTS は年 1 回で 2 年サポート(Enterprise/Industry は +1 年)。6.3 LTS は 2027-12 まで、6.0 LTS は 2026-10 まで。Update リリースは年に複数回で、次の公開までサポートされ、新規・開発途中のプロジェクトに推奨 — [Unity 6 Support](https://unity.com/releases/unity-6/support)
- 6.7 LTS を Mono ベースの最終版、7.0 で Mono 撤去とする計画(.NET 10 / C# 14 の CoreCLR エディタと desktop player が目標)。6.7 の CoreCLR Desktop Player は実験的公開で本番用途外 — [Unity フォーラム公式更新 2026-06](https://discussions.unity.com/t/coreclr-scripting-and-serialization-update-june-2026/1723299)。ただし同時期の検索要約や二次情報には「6.8 で Mono 撤去」とするものがある — [Digital Production 2025-11](https://digitalproduction.com/2025/11/26/unitys-2026-roadmap-coreclr-verified-packages-fewer-surprises/)、[March 2026 更新](https://discussions.unity.com/t/coreclr-scripting-and-ecs-status-update-march-2026/1711852)。**取得本文に基づく 7.0 説を採用しつつ、番号は流動的と見るべき。**

**Unity: ライセンス**
- Personal: 無料、売上+資金調達 $200,000 まで、Unity 6 ではスプラッシュ画面は任意。Pro: $200,000 超で必須、$2,310/席/年(または $210/席/月)、コンソールと Vision Pro への公開を含む。Enterprise: 年間売上 $25M 超で必須、個別見積もり、ソースコードアクセスを含む。Pro/Enterprise は 2026-01-12 から 5% 値上げ — [Unity pricing updates](https://unity.com/products/pricing-updates)
- Runtime Fee(インストール数課金案)は 2024-09-12 に撤回、実施されなかった — [Unity pricing updates](https://unity.com/products/pricing-updates)
- Unity 6.3 以降、Havok Physics for Unity は Pro/Enterprise/Industry に含まれない — [Unity pricing updates](https://unity.com/products/pricing-updates)

### Inferences
- 比較の「上限」として UE は 5.8 を基準にしてよいが、スクリプティングと entity モデルは UE6 で置き換わる方向にある(Verse + Scene Graph)。Unity 側も 7.0 でランタイムが変わる。両者とも 2026-10 の機能一覧は移行期のスナップショットと見るべき。

### Gaps
- Unreal のライセンス条項が 2026 時点で不変である一次確認はできなかった(公式ページ 403、EULA 未読)。
- 6.7 LTS の正式リリース日は未確認。

## ドメイン別機能カタログ

### Takeaway
どちらも本カタログの全ドメインに標準機能または公式パッケージを持つ。差が出るのは、Unreal が高性能機向けの統合スタック(Nanite/Lumen/MegaLights)を持ち実行時サイズが大きいのに対し、Unity は URP 中心でモバイル/Web に幅広く、空アプリサイズも小さいとみられる点(Unity は公式値、Unreal は逸話のみで同条件比較ではない)。

### Cited Findings

#### レンダリング
**Unreal 5.8**
- Nanite(仮想化ジオメトリ。見えている分の細部だけを処理し、手作業の LOD を不要にする)。対象はコンソール/デスクトップ、DX12 + SM6。Static Mesh、Geometry Collection、skeletal mesh、インスタンス、spline、foliage、landscape grass などに対応。制約は Opaque/Masked のみ、Forward 非対応、VR stereo と MSAA 非対応、ストリーム済みインスタンス上限 1,600 万 — [Nanite docs](https://dev.epicgames.com/documentation/en-us/unreal-engine/nanite-virtualized-geometry-in-unreal-engine)
- Lumen(動的グローバルイルミネーション=間接光の実時間計算)。Software RT はメッシュ距離場を使い SM6 が必要(GTX-1070 以上)。Hardware RT は RTX-2000 / RX-6000 以降。PS4/Xbox One は非対応、VR と Forward は非対応。Android については Vulkan + Mobile Renderer に関する記述がある — [Lumen technical details](https://dev.epicgames.com/documentation/en-us/unreal-engine/lumen-technical-details-in-unreal-engine)
- 5.8 の状態: MegaLights(多数の動的シャドウ付きライト)= Production Ready、Lumen Lite(中品質 GI、携帯機向け)= Beta、Substrate(マテリアルシステム)の toon shading と Fog Screen Space Scattering = Experimental、Movie Render Graph = Production Ready — [UE 5.8 release notes](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-5-8-release-notes)
- モバイルは Forward と Deferred の両シェーディングを提供、5.8 でモバイルは Deferred が既定 — [Creating mobile games](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-creating-mobile-games); [5.8 notes](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-5-8-release-notes)

**Unity 6**
- 戦略: URP を全プラットフォーム向けに前進させ、HDRP は保守(Switch 2 対応と安定性)に絞る。Built-in RP は 6.5 で deprecated、少なくとも 6.7 LTS まで利用可、公式サポートは少なくとも 2028 年末まで、撤去版は未定 — [Render Pipelines strategy for 2026](https://unity.com/topics/render-pipelines-strategy-for-2026)
- URP = 全プラットフォーム向けのスケーラビリティ、HDRP = ハイエンド向けフォトリアル。HDRP は iOS/Android/WebGL 非対応 — [Choose a render pipeline](https://docs.unity.com/en-us/engine/6000.6/manual/render-pipelines/choose-a-render-pipeline/pipeline); [Feature comparison](https://docs.unity3d.com/6000.6/Documentation/Manual/render-pipelines-feature-comparison.html)
- シェーダー作成: Shader Graph(ノード型)が全パイプラインで利用可、URP/HDRP では ShaderLab 手書きより推奨 — [Feature comparison](https://docs.unity3d.com/6000.6/Documentation/Manual/render-pipelines-feature-comparison.html)
- 6.3: URP/HDRP 共通の Render Graph 基盤、Ray tracing hit shader、UnifiedRayTracing API、DLSS4、GPU Lightmapper が既定 — [Unity 6.3 What's New](https://docs.unity.com/en-us/engine/6000.3/manual/whats-new/unity63)
- 6.6: Unity Compute Light Baker 追加、Progressive CPU Lightmapper と dynamic batching を非推奨化、DXC コンパイラ、Web の WebGPU が experimental を脱却 — [Unity 6.6 What's New](https://docs.unity.com/en-us/engine/6000.6/manual/whats-new/unity66)

#### アニメーション
**Unreal**
- IK Rig(Full-Body IK を含むソルバーを対話的に構築し、Animation Blueprint から実行時に制御)と IK Retargeter(骨格間のモーション転送。チェーン自動生成、Python API あり) — [IK Rig](https://dev.epicgames.com/documentation/unreal-engine/ik-rig-in-unreal-engine); [Retargeting](https://dev.epicgames.com/documentation/en-us/unreal-engine/ik-rig-animation-retargeting-in-unreal-engine); [Auto Retargeting](https://dev.epicgames.com/documentation/unreal-engine/auto-retargeting-in-unreal-engine?lang=en-US)
- 5.8: Control Rig Physics = Beta、RigMapper = Beta、Animation Mixer と Direct Mesh Controls = Experimental、Mutable(実行時キャラクターカスタマイズ)= production ready — [5.8 notes](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-5-8-release-notes)
- スプライトは Paper 2D プラグインで 2D / 2D+3D プロジェクトを作成 — [Get Started](https://dev.epicgames.com/documentation/unreal-engine/get-started?lang=en-US)

**Unity**
- Mecanim: Animator Controller = クリップ間の遷移を管理するステートマシン。Blend Tree = パラメータ値で複数モーションを加重合成(2D 方向ブレンドあり) — [Mecanim overview](https://docs.unity3d.com/6000.6/Documentation/Manual/AnimationOverview.html); [Blend Trees](https://docs.unity3d.com/6000.3/Documentation/Manual/class-BlendTree.html)
- 6.3: Legacy Animation の評価が最大 30% 高速化、`Animator.ResetControllerState` — [6.3 What's New](https://docs.unity.com/en-us/engine/6000.3/manual/whats-new/unity63)
- 6.6: Animation Rigging、Timeline、Cinemachine が core package 化 — [6.6 What's New](https://docs.unity.com/en-us/engine/6000.6/manual/whats-new/unity66)

#### 物理・衝突
**Unreal**
- 5.8 のリリースノートに Chaos の Cloth(production ready)、Destruction、Hair、Flesh、Modular Vehicles、Visual Debugger がある — [5.8 notes](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-5-8-release-notes)
- Mover(キャラクター移動、予測とネットワークを含む)に ChaosMover の変更あり、Experimental 脱却へ移行中と記載 — [5.8 notes](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-5-8-release-notes)

**Unity**
- 3D 組み込み物理は NVIDIA PhysX 統合。Rigidbody、collider、joint、articulation、ragdoll、cloth、character control、複数シーン物理 — [Physics overview 6.3](https://docs.unity.com/en-us/engine/6000.3/manual/physics-section/physics-overview)
- 6.3: Box2D v3 ベースの低レベル API `UnityEngine.LowLevelPhysics2D`(マルチスレッド、決定性向上、視覚デバッグ)。物理バックエンドを無効化・除去してビルドサイズ削減が可能 — [6.3 What's New](https://docs.unity.com/en-us/engine/6000.3/manual/whats-new/unity63)
- ECS 向けに Unity Physics パッケージ — [Entities 6.7](https://docs.unity3d.com/Packages/com.unity.entities@6.7/manual/index.html)。Havok は上記の通りプランから外れた — [Unity pricing](https://unity.com/products/pricing-updates)

#### オーディオ
- **Unreal 5.8**: Audio Insights = Production Ready(ラウドネス計測、信号フロー)、MetaSound(ノード型の音声合成・処理)の node config と CAT = Experimental、Windows 既定が WASAPI — [5.8 notes](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-5-8-release-notes)
- **Unity 6.3**: Scriptable audio pipeline(Burst コンパイルの C# プロセッサを音声チェーンに差し込む) — [6.3 What's New](https://docs.unity.com/en-us/engine/6000.3/manual/whats-new/unity63)

#### 入力
- **Unreal**: Enhanced Input。Input Action(bool/1D/2D/3D)、Mapping Context(実行時に優先度付きで切替)、Modifier(dead zone など)、Trigger(Hold など)、実行時リマップ — [Enhanced Input](https://dev.epicgames.com/documentation/en-us/unreal-engine/enhanced-input-in-unreal-engine)。5.8 で Android コントローラ、iOS/iPadOS のキーボード・マウス、gesture event(Experimental) — [5.8 notes](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-5-8-release-notes)
- **Unity**: Manual に Input セクションあり — [6.6 Manual](https://docs.unity.com/zh-cn/engine/6000.6/manual)。Input System の Gamepad Haptics で Android ゲームパッド振動対応(6.6) — [6.6 What's New](https://docs.unity.com/en-us/engine/6000.6/manual/whats-new/unity66)

#### カメラ
- **Unity**: Cinemachine(仮想カメラによる追従・構図制御)は 6.6 で core package — [6.6 What's New](https://docs.unity.com/en-us/engine/6000.6/manual/whats-new/unity66)
- **Unreal**: 今回一次情報を確認できず(Gaps 参照)。

#### パーティクル / VFX
- **Unreal**: Niagara。モジュールで構成し、Niagara Script Editor で独自モジュール作成可。流体(炎、煙)、衝突(GPU レイトレース衝突は experimental)、Lightweight Emitters、Debugger — [Niagara](https://dev.epicgames.com/documentation/en-us/unreal-engine/creating-visual-effects-in-niagara-for-unreal-engine)
- **Unity**: VFX Graph は GPU 上のシミュレーションまで扱う — [Choose a render pipeline](https://docs.unity.com/en-us/engine/6000.6/manual/render-pipelines/choose-a-render-pipeline/pipeline)。6.6 で Shader Graph が組み込み Particle System と URP の互換用ノードを追加 — [6.6 What's New](https://docs.unity.com/en-us/engine/6000.6/manual/whats-new/unity66)
- 注意: 検索要約では feature table が「VFX Graph パーティクルは HDRP のみ、URP は 2D 用」と読めた — [Feature comparison](https://docs.unity3d.com/6000.6/Documentation/Manual/render-pipelines-feature-comparison.html)。筆者の知識と矛盾するため要確認で、この記述を根拠にしないこと。

#### UI
- **Unreal**: 公式に Creating User Interfaces セクション — [5.8 docs hub](https://dev.epicgames.com/documentation/unreal-engine/unreal-engine-5-8-documentation?lang=en-US)
- **Unity 6.3 公式の推奨**: runtime は uGUI、editor は UI Toolkit。ただし UI 中心の新規 runtime では UI Toolkit も有力。UI Toolkit = UI Builder、textureless、world-space、データバインディング、SVG、RTL。uGUI = keyframe/Timeline 連携、シーン内編集。UI Toolkit は活発に開発中、uGUI と IMGUI は成熟・更新頻度低 — [UI system compare](https://docs.unity.com/en-us/engine/6000.3/manual/uitoolkits/ui-system-compare)
- 6.3: UI Shader Graph、USS filter、UI Test Framework、ベクターグラフィクス組み込み — [6.3 What's New](https://docs.unity.com/en-us/engine/6000.3/manual/whats-new/unity63)。6.6: backdrop-filter、drop-shadow、uGUI SafeArea — [6.6 What's New](https://docs.unity.com/en-us/engine/6000.6/manual/whats-new/unity66)

#### シーン / エンティティモデル
**Unreal**
- MassEntity(データ指向 ECS 相当): Fragment(データのみ)、Tag、Archetype(同一構成のエンティティをチャンクで格納)、Processor(ステートレスなロジック)、EntityQuery — [MassEntity overview](https://dev.epicgames.com/documentation/en-us/unreal-engine/overview-of-mass-entity-in-unreal-engine)。5.8 で MassCore モジュール、スパース fragment、スケジューリング刷新 — [5.8 notes](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-5-8-release-notes)
- Scene Graph は Verse 上に新規構築する gameplay framework で UE6 の核。Actors/Blueprints は初期 UE6 で利用可だが、最終的に deprecated の予定で変換ツールも計画 — [GamesBeat](https://gamesbeat.com/unreal-engine-6-will-combine-ue5-and-uefn-into-a-unified-engine-state-of-unreal/)

**Unity**
- Manual の主要章は GameObjects と Scenes — [6.6 Manual](https://docs.unity.com/zh-cn/engine/6000.6/manual)
- Entities パッケージ 6.7.0(DOTS の ECS 実装)、Baking で GameObject から変換、Entities Graphics、Unity Physics、Netcode for Entities と併用 — [Entities 6.7](https://docs.unity3d.com/Packages/com.unity.entities@6.7/manual/index.html)
- 6.6: Entities も標準 Hierarchy に表示、managed component を非推奨化 — [6.6 What's New](https://docs.unity.com/en-us/engine/6000.6/manual/whats-new/unity66)

#### スクリプティング
**Unreal**
- Blueprint(ビジュアルスクリプティング)は gameplay 向けの公式セクションあり — [5.6 docs hub](https://dev.epicgames.com/documentation/unreal-engine/unreal-engine-5-6-documentation?application_version=5.6&lang=en-US)。C++ API リファレンスも公式 — [C++ API](https://dev.epicgames.com/documentation/en-us/unreal-engine/API)
- Verse は Epic の新言語で C++ を「トランザクション化」すると説明され、現在はシングルスレッド動作 — [GamesBeat](https://gamesbeat.com/unreal-engine-6-will-combine-ue5-and-uefn-into-a-unified-engine-state-of-unreal/)

**Unity**
- C#。Mono から CoreCLR へ移行中(上記)、6.6 の新規プロジェクトは domain reload オフが既定、Burst が組み込みモジュール化 — [6.6 What's New](https://docs.unity.com/en-us/engine/6000.6/manual/whats-new/unity66); [CoreCLR update](https://discussions.unity.com/t/coreclr-scripting-and-serialization-update-june-2026/1723299)

#### AI / ナビゲーション
- **Unreal**: Behavior Tree(意思決定ツリー)+ Blackboard(キーを持つデータストア) — [Behavior Trees](https://dev.epicgames.com/documentation/en-us/unreal-engine/behavior-trees-in-unreal-engine)。StateTree(BT の selector とステートマシンの state/transition を合わせた階層型 FSM、プラグイン) — [State Tree](https://dev.epicgames.com/documentation/en-us/unreal-engine/state-tree-in-unreal-engine); [Overview](https://dev.epicgames.com/documentation/en-us/unreal-engine/overview-of-state-tree-in-unreal-engine)
- **Unity**: NavMesh Agent がアニメーションと連動する例(2023.1 文書で古い) — [Coupling Animation and Navigation](https://docs.unity3d.com/2023.1/Documentation/Manual/nav-CouplingAnimationAndNavigation.html)。AI Navigation パッケージは 6.6 beta のノートで 2.0.14 に更新 — [6000.6.0b1](https://unity.com/releases/editor/beta/6000.6.0b1)

#### ネットワーク / マルチプレイヤー
- **Unreal**: Replication(クライアント/サーバー間でデータと手続き呼び出しを同期)、Iris Replication System、Replication Graph、Replay System — [Networking](https://dev.epicgames.com/documentation/en-us/unreal-engine/networking-and-multiplayer-in-unreal-engine)。Iris は 5.8 で licensee 向け production-ready — [5.8 notes](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-5-8-release-notes)
- **Unity**: Netcode for GameObjects(高水準ライブラリ。6.6 beta 時点で 2.13.1) — [NGO docs](https://docs.unity3d.com/Packages/com.unity.netcode.gameobjects@2.5/manual/index.html); [6000.6.0b1](https://unity.com/releases/editor/beta/6000.6.0b1)。Netcode for Entities(DOTS 向け、1.13.2 が 6000.4 向けに記載) — [Manual](https://docs.unity3d.com/Manual/com.unity.netcode.html)。Dedicated Server は 6.3 で Linux Arm64 対応 — [6.3 What's New](https://docs.unity.com/en-us/engine/6000.3/manual/whats-new/unity63)

#### アセットパイプライン / インポート形式
- **Unreal**: FBX Content Pipeline、Interchange Framework、glTF のインポート/エクスポート — [Working with Content](https://dev.epicgames.com/documentation/en-us/unreal-engine/working-with-content-in-unreal-engine)。5.8: uFBX で高速化、USD asset import = production ready、Incremental Cooking(クック=配布形式への変換)= Beta、Zenserver 既定 — [5.8 notes](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-5-8-release-notes)
- **Unity**: Manual に Assets and media 章 — [6.6 Manual](https://docs.unity.com/zh-cn/engine/6000.6/manual)。SVG を Vector Images として標準インポート(6.3)、Shader variant を Build Profile で制限 — [6.3 What's New](https://docs.unity.com/en-us/engine/6000.3/manual/whats-new/unity63)

#### ストリーミング / LOD
- **Unreal**: World Partition(ワールドをグリッドセルに分割し、streaming source からの距離でロード)、Data Layers、HLOD(遠景の簡略化)、One File Per Actor(チーム編集の衝突を低減) — [World Partition](https://dev.epicgames.com/documentation/en-us/unreal-engine/world-partition-in-unreal-engine)。Mesh Terrain と Fast Geometry Streaming = Experimental — [5.8 notes](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-5-8-release-notes)。Chunk 分割、ChunkDownloader、Google Play Asset Delivery — [Mobile](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-creating-mobile-games)
- **Unity**: Addressables の最新は 4.1 — [Addressables](https://docs.unity3d.com/Packages/com.unity.addressables@latest/)。Web で Progressive Asset Loading(シーン単位、6.6) — [6.6 What's New](https://docs.unity.com/en-us/engine/6000.6/manual/whats-new/unity66)

#### セーブ / シリアライズ
- **Unreal**: `USaveGame` 派生クラスに値を持たせ、`AsyncSaveGameToSlot` などで保存。複数スロット対応、バイナリ API あり、開発環境では `.sav` — [Saving and Loading](https://dev.epicgames.com/documentation/en-us/unreal-engine/saving-and-loading-your-game-in-unreal-engine)
- **Unity**: 6.6 で `Dictionary` の `[SerializeField]` をネイティブ対応。com.unity.serialization は deprecated(代替案は System.Text.Json、Odin、MemoryPack) — [CoreCLR update](https://discussions.unity.com/t/coreclr-scripting-and-serialization-update-june-2026/1723299)

#### エディタ / ツール
- **Unreal 5.8**: Editor Gizmo System、Sandboxes、MCP server(Experimental、LLM 連携)、World Partition Insights、Horde(Experimental) — [5.8 notes](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-5-8-release-notes)
- **Unity 6.3**: Profiler の Highlights モジュール、Frame Debugger の VRS 対応、Render Graph Viewer、Build Profiles、パッケージ署名、Adaptive Performance 組み込み — [6.3 What's New](https://docs.unity.com/en-us/engine/6000.3/manual/whats-new/unity63)。6.6: 新 Hierarchy が既定、Systems window 改善、Profiler で GPU Resident Drawer を表示 — [6.6 What's New](https://docs.unity.com/en-us/engine/6000.6/manual/whats-new/unity66)

#### プラットフォームとモバイル
**Unreal 5.8**
- Android: 最低インストール SDK 26、コンパイル SDK 34、NDK r27c。Apple 系(iOS/tvOS/visionOS): 最低ターゲット SDK 17、Xcode 26.0 以上。Windows、macOS(Sonoma 以上)、Linux(Ubuntu 22.04、Rocky/RHEL 8+)、Switch/Switch 2、PS4、PS5 の SDK が記載 — [5.8 notes](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-5-8-release-notes)
- Android は GPU により Vulkan または OpenGL ES 3.2 — [Mobile](https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-creating-mobile-games)

**Unity 6.3**(6.6 で一部更新)
- Android 7.1(API 25)+、OpenGL ES 3.0+/Vulkan。iOS/iPadOS 15+(Metal)。tvOS 15+。Windows 10 21H1+。UWP。macOS 12+。Ubuntu 22.04/24.04。Embedded Linux/QNX(Industry 経由)。Web(WebGL 2 + Wasm)。Switch/Switch 2、Xbox、PS4/PS5。Quest、visionOS、HoloLens など XR — [System requirements 6.3](https://docs.unity.com/en-us/engine/6000.3/manual/get-started/install-and-upgrade/getting-started-installing-unity/system-requirements)
- 6.6: Android は OpenGL ES 3.1 以上に、Web で WebGPU と Wasm64(最大 16 GB、Safari 非対応)、macOS Intel を deprecated — [6.6 What's New](https://docs.unity.com/en-us/engine/6000.6/manual/whats-new/unity66)

#### 最小ランタイムサイズ
- **Unity iOS(公式)**: 空プロジェクトは最適化なしで App Store 約 20 MB、stripping などで zip + DRM 後 12 MB 未満まで可能 — [Unity Manual: player size](https://docs.unity3d.com/Manual/iphone-playerSizeOptimization.html)
- **Unity Android(古い逸話)**: Unity 2021.3 の空 2D で APK 約 14.5 MB、インストール後約 40 MB — [Unity フォーラム](https://discussions.unity.com/t/how-to-reduce-the-built-size-for-an-empty-project/827895)。**Unity 6 の値ではない。**
- **Unreal(公式値なし、逸話のみ)**: UE4.6.1 で 37 MB(UMG + Paper2D)、iOS 52 MB / Android 72 MB の報告、UE4.13 の ~123 MB IPA のうちエンジンコード 82.5 MB、UE5.3.2 で `lib.so` 最低 90 MB、UE5 の最小レベルで APK 400 MB 超。 — フォーラム群: [最小サイズ](https://forums.unrealengine.com/t/what-is-the-minimum-size-of-the-unreal-engine-based-mobile-application/461166)、[iOS 100MB 未満](https://forums.unrealengine.com/t/how-many-people-have-successfully-packaged-an-ios-game-under-100mb/63936)、[UE5 モバイル最適化](https://forums.unrealengine.com/t/mobile-optimization-in-ue5-how-to-reduce-package-size-without-sacrificing-visual-quality/2511477)、[APK 最小](https://forums.unrealengine.com/t/what-is-the-minimum-size-one-can-achieve-for-android-apk/296898)。**数値と各スレッドの対応は検索要約経由で未照合。UE 5.8 の数値ではない。**
- **Unreal 公式の削減手段**: dead code 除去と ICF は UE5 で既定。MinSDK を 23/28 に上げると relocation テーブル圧縮が可能で、62 MB のテーブルが APS で約 8 MB、RELR で約 600 KB に。Play の cellular 上限 150 MB に言及 — [Reducing Android Binary Size](https://dev.epicgames.com/documentation/en-us/unreal-engine/reducing-android-binary-size-in-unreal-engine-projects)

### Inferences
- モバイル最小サイズは Unity が 10〜20 MB 台、UE5 は 100 MB 前後以上が目安と推定される。ただし Unreal 側は版も条件も揃わない逸話で、同一条件の実測ではない。比較表では「桁の違い」程度に留めるべき。
- Unreal の目玉機能(Nanite、Lumen 通常版)はデスクトップ/コンソール寄りで、モバイルで同等に使える前提は置けない。「上限」の記述はプラットフォーム別に分ける必要がある。
- UI の推奨が Unity 公式でも uGUI と UI Toolkit に分かれており、Unity は同じドメインに新旧系統を併存させがち(UI、物理、エンティティ、レンダーパイプライン)。

### Gaps
**調査範囲の限界**
- UE 5.8 リリースノートは全 357,273 文字のうち先頭 200,000 文字のみ確認。UMG/Slate、Verse、Niagara、Live Coding の 5.8 変更は残りの部分にある可能性がある。
- Lumen の Android 対応記述は要約経由で文意が曖昧。Mobile Renderer で本当に Lumen が動くのか、Lumen Lite との関係も含め原文確認が必要。Nanite のモバイル対応(iOS SM6 = Experimental との関係)も同様。
- UE 5.8 と Unity 6 の Android/iOS 最小サイズの公式値または同条件実測は見つからなかった。正確な比較には自前ビルドが必要。
- Animation overview と Verse reference は取得できたが本文が実質空で、根拠に使っていない。
- UE6 の Blueprints の扱いは二次情報間で表現に差あり — [Ludus](https://ludusengine.com/blog/will-verse-replace-blueprints)、[Outlook India](https://respawn.outlookindia.com/gaming/gaming-news/preparing-for-ue6-key-features-ai-blueprint-changes-revealed)。本ノートは GamesBeat の報道に従った。Epic 一次の "The Road to Unreal Engine 6" は未取得。
- Unity ECS の core package 化(6.4)は二次情報のみ — [Digital Production](https://digitalproduction.com/2025/11/26/unitys-2026-roadmap-coreclr-verified-packages-fewer-surprises/)。

**未検証(今回は出典を取れなかった一般知識。カタログ完全性のための候補で、確定事実として引用しないこと)**
- Unreal レンダリング: Virtual Shadow Maps、TSR、Material Editor、Post Process Volume。アニメーション: Animation Blueprint、Blend Space、Montage、Control Rig、Motion Matching、ML Deformer。カメラ: Camera Manager、Spring Arm、Gameplay Cameras プラグイン、Sequencer。UI: UMG、Slate、CommonUI。AI: Recast ベースの NavMesh、EQS、Smart Objects。ネットワーク: RPC、dedicated/listen server、Online Subsystem。ツール: Unreal Insights、Live Coding、Blueprint デバッガ。Web 出力: 公式 HTML5 対応は UE4 期に廃止。
- Unity レンダリング: GPU Resident Drawer の詳細、Adaptive Probe Volumes、STP、シャドウカスケード、Volume ベースのポストプロセス。アニメーション: 2D Animation パッケージ、Sprite Renderer、Animation Rigging の IK、Humanoid retargeting。VFX: Particle System(Shuriken)。オーディオ: Audio Mixer。入力: Input System の Action Map と旧 Input Manager。AI: Unity Behavior。ネットワーク: Unity Transport、Relay/Lobby。セーブ: JsonUtility、PlayerPrefs。ストリーミング: AssetBundle、LOD Group。インポート: FBX、OBJ など。ツール: Memory Profiler、C# デバッガ連携。
