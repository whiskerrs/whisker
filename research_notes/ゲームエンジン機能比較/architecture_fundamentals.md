# ゲームエンジンの一般アーキテクチャと語彙(製品非依存 primer)

調査日: 2026-10-31。Unreal / Unity / Godot / Bevy / Flame 比較レポートの分類軸を提供する。個別エンジンの機能カタログは対象外。

出典の扱い: Cited Findings は取得できたページ本文に基づく。本文を取得できなかった事項、および一般知識に頼る説明は、各節の Gaps に「未検証」と明記した。Flame は 2D 中心の想定だが、本ノートでは検証していない。

## 1. 標準的なレイヤーとサブシステム(Gregory『Game Engine Architecture』ほか)

### Takeaway
Gregory 第4版の章立ては「基盤 → 低レベルシステム → 描画・アニメーション・物理・音声 → gameplay」の順で、engine を下位から積み上げる構成を反映している。ランタイムと tools / asset pipeline は別の節として扱われ、比較でもこの二分が第一の軸になる。

### Cited Findings
- 第4版は2巻18章構成。Vol. I: Introduction / Tools of the Trade / Fundamentals of Software Engineering / Parallelism and Concurrent Programming / 3D Math / Engine Support Systems / Resources and the File System / The Game Loop and Real-Time Simulation / Human Interface Devices / Tools for Debugging and Development — [gameenginebook.com TOC](https://www.gameenginebook.com/toc.html)
- Vol. II: Rendering / Lighting and Post-Processing / Animation Systems / Collision and Rigid Body Dynamics / Audio / Introduction to Gameplay Systems / Runtime Gameplay Systems / You Mean There's More? — [同上](https://www.gameenginebook.com/toc.html)
- 第4版では 1.5 が "Runtime Engine Architecture"、1.6 が "Tools and the Asset Pipeline"。TOC に小節の記載はなく、層の一覧は読めない(依頼文などで参照される「1.6 Runtime Engine Architecture」は旧版の番号付けとみられる) — [同上](https://www.gameenginebook.com/toc.html)
- 第3版(2018)は Foundations / Low-Level Engine Systems / Graphics, Motion and Sound / Gameplay / Conclusion の5部構成 — [Perlego](https://www.perlego.com/book/4364482/game-engine-architecture-pdf)(書誌情報)
- Runtime Gameplay Systems 章の小節: Gameplay Foundation の構成要素 / Runtime Object Model Architectures / World Chunk Data Formats / Loading and Streaming Game Worlds / Object References and World Queries / Updating Game Objects in Real Time / Concurrent Game Object Updates / Events and Message-Passing / Scripting / High-Level Game Flow — [同上 TOC](https://www.gameenginebook.com/toc.html)
- Game Loop 章の小節: Rendering Loop / Game Loop / Architectural Styles / Abstract Timelines / Measuring and Dealing with Time / Multiprocessor Game Loops — [同上 TOC](https://www.gameenginebook.com/toc.html)

### Inferences
TOC から再構成した分類案。以下の層の切り方は筆者の整理で、Gregory の図の引用ではない。

| 層 | サブシステム | 存在理由 |
|---|---|---|
| Platform | OS、GPU API、音声 API、入力、ファイル、ライフサイクル | 移植性の吸収 |
| Core | memory allocator、math、container、job system、logging、profiler | リアルタイム制約下の共通基盤 |
| Resources | asset 読み込み、参照管理、streaming | 実行時データの寿命管理 |
| Simulation 基盤 | game loop、timeline、HID 入力 | 時間進行と外部入力の分離 |
| 機能サブシステム | rendering、animation、collision/physics、audio、UI | 専門計算 |
| Gameplay Foundation | object model、world 読み込み、event、scripting、game flow | ゲーム固有ロジックの土台 |
| ゲーム本体 | ルール、AI、レベル | engine の外 |
| Tools(別系統) | editor、importer/cooker、debugger | 制作と変換 |

1フレームの典型的な流れ(各節の出典を組み合わせた推論。順序やスレッド分割は engine ごとに異なる):
1. OS の表示同期(CADisplayLink / Choreographer)でフレーム開始、経過時間を計測
2. 入力とイベントを取り込む
3. accumulator を消費する回数だけ fixed update(gameplay の一部 → physics → 衝突イベント)
4. variable update(カメラ、UI、見た目の gameplay)
5. animation のサンプリングとブレンド、ボーン行列の計算、必要なら IK
6. 描画用データの抽出(alpha による transform 補間を含む)
7. culling、LOD 選択、ソート、batching / instancing、render graph の構築
8. GPU コマンドの提出と present(frame pacing を適用)
9. 音声は別の高優先度スレッドで常時動き、game thread からはキュー経由でコマンドを受ける

### Gaps
- 二次資料で確認できた層: Platform Independence Layer(ゲーム側の操作とシステム依存の実装を変換) — [UMD CMSC425 講義ノート](https://www.cs.umd.edu/class/spring2018/cmsc425/Lects/lect02-architecture.pdf)。Core Systems(assert、memory allocation、データ構造、math など) — [Hamburg 大学の講義抜粋](https://tams.informatik.uni-hamburg.de/lectures/2026ss/seminar/engine/doc/GameEngineCourseExtract.pdf)。Gameplay Foundation は engine と game の境界のすぐ下に位置する — [O'Reilly 第3版 16章](https://www.oreilly.com/library/view/game-engine-architecture/9781351974271/chapter16.xhtml)。いずれも検索抜粋で確認したもので、書籍の図そのものではない。
- 未検証: 層図の全体(記憶では Hardware / Drivers / OS / 3rd Party SDKs / Platform Independence Layer / Core Systems / Resources / 低レベル描画など / Gameplay Foundations / Game-Specific Subsystems)。引用する場合は書籍で確認が必要。

## 2. Game loop: fixed / variable、update/render 分離、補間

### Takeaway
実務上の標準形は「固定 dt の simulation + 可変レートの rendering + 残り時間での補間」。可変 dt は実装が簡単だが、物理とネットワークの再現性を損なう。

### Cited Findings
- Game loop の意図は「game time の進行を user input と processor speed から切り離す」こと — [Game Programming Patterns: Game Loop](https://gameprogrammingpatterns.com/game-loop.html)
- 4変種: (a) fixed step・sync なし(速度がマシン依存)/ (b) fixed step・sync あり(sleep で調整、遅い時はゲーム自体が減速)/ (c) variable step(非決定的で物理やネットワークが不安定、多くの開発者が非推奨)/ (d) fixed update + variable render(描画を飛ばして追いつける、複雑さが増す) — [同上](https://gameprogrammingpatterns.com/game-loop.html)
- Fiedler の段階的な整理: fixed dt → variable dt(dt 依存で発散しうる)→ semi-fixed(上限つき分割、ただし厳密には再現しない)→ "free the physics"(renderer が時間を生み、simulation が固定 dt で消費) — [Gaffer On Games: Fix Your Timestep!](https://gafferongames.com/post/fix_your_timestep/)
- accumulator にフレーム時間を足し、dt 以上ある間だけ step を回し、端数は次フレームへ繰り越す — [同上](https://gafferongames.com/post/fix_your_timestep/)
- Spiral of death: step の計算時間が simulate する時間を上回ると遅れが拡大する。対策は CPU の余裕確保か step 数の上限で、上限をかけると負荷時に simulation が遅く見える。最終コードは frame time を 0.25 秒に clamp — [同上](https://gafferongames.com/post/fix_your_timestep/)
- `alpha = accumulator / dt` で前回と今回の状態を線形補間し、回転は quaternion の slerp を使う — [同上](https://gafferongames.com/post/fix_your_timestep/)
- Game Programming Patterns は同じ端数を、速度を使った extrapolation に用いる例を示す。次の update で外れることがあるが、stutter より目立ちにくいとする — [Game Loop](https://gameprogrammingpatterns.com/game-loop.html)
- Loop の所有者は3択: platform(簡単だが timing 制御を失う)/ engine / 自前(platform のイベント処理に時間を返す必要がある) — [同上](https://gameprogrammingpatterns.com/game-loop.html)
- Mobile ではフレームレートを 30 / 60 に clamp し残りを sleep する方式が多く、電池と発熱の面で有利なことが多い — [同上](https://gameprogrammingpatterns.com/game-loop.html)

### Inferences
- 補間は「すでに計算済みの2状態の間」を表示するため、表示は最新の simulation より最大で約 1 fixed step 遅れる(定義からの導出)。Extrapolation は遅延を増やさない代わりに予測誤差が出る。比較軸は「補間を標準提供するか、ユーザー実装か」が有効。
- mobile アプリ埋め込みでは OS 側が display callback を持つので、loop の所有権(platform 所有か engine 所有か)はアーキテクチャ上の重要な比較軸になる。

### Gaps
- 補間による遅延を定量化した一次資料は確認できなかった(Fiedler の本文は数値なし)。

## 3. Entity model: inheritance / component / ECS

### Takeaway
深い継承は機能の組み合わせに耐えず、component による合成が主流になった。ECS はさらにデータと振る舞いを分け、キャッシュ効率と並列化に向くが、構造変更のコストや学習負荷を伴う。

### Cited Findings
- 継承の問題: `Zone`(衝突)と `Decoration`(描画)の両方が要る `Prop` は多重継承なしでは書けない。階層を組み替えるとコードが重複し、基底に寄せると不要なデータでメモリを浪費する。多重継承は deadly diamond を招く — [GPP: Component](https://gameprogrammingpatterns.com/component.html)
- Component 間の通信は3種類: 共有状態(疎結合だが実行順に依存)/ 直接参照(速いが密結合)/ message(疎結合だがフィードバックループの恐れ)。併用を推奨 — [同上](https://gameprogrammingpatterns.com/component.html)
- Component のコスト: 組み立ての複雑さと、取得の indirection や仮想呼び出し — [同上](https://gameprogrammingpatterns.com/component.html)
- RAM アクセスは数百サイクルかかりうる。キャッシュライン(64〜128 byte 程度)に沿った連続配置が効き、著者のベンチマークではキャッシュミスの差だけで約50倍の差が出た — [GPP: Data Locality](https://gameprogrammingpatterns.com/data-locality.html)
- 対策は component 種別ごとの連続配列と hot/cold 分割。ただしコードは複雑で柔軟性を失うので、まず計測する — [同上](https://gameprogrammingpatterns.com/data-locality.html)
- ECS の定義: Entity は一意な ID、Component は振る舞いのないデータ、System は特定の component 集合を持つ entity 全部に働く関数 — [ECS FAQ (Sander Mertens)](https://github.com/SanderMertens/ecs-faq)
- EC(Unity GameObject など)では component がデータと振る舞いを持つ。ECS では振る舞いは system 側にある — [同上](https://github.com/SanderMertens/ecs-faq)
- Storage: archetype は同じ component 集合を持つ entity を1テーブルにまとめ、iteration が速いが追加削除はテーブル間移動で高い。sparse set は追加削除が速いが、複数 component の結合時に lookup が増える — [同上](https://github.com/SanderMertens/ecs-faq)
- ECS の弱点: engine と統合されないと glue code が増える、OOP の直感に反する、木や空間構造を要するクエリに不向き、階層表現で小テーブルが増えると性能が落ちうる。FAQ の著者は ECS ライブラリ Flecs の作者であり、ECS 寄りの立場である点に注意 — [同上](https://github.com/SanderMertens/ecs-faq)

### Inferences
比較に使う4分類: (1) 継承中心のノード木、(2) ノード木 + 合成、(3) EC(object に behaviour を持つ component を付ける)、(4) ECS。engine はこれらの混合になりやすいので、「scene graph の有無」と「データレイアウトの制御」を別の軸として評価する。ECS の性能上の利点は大量の同種 entity で現れ、少数の entity では複雑さが勝る可能性がある(Data Locality の「計測せよ」からの推論)。

### Gaps
- ECS と EC の実アプリでの生産性を比較した信頼できる定量研究は見つからなかった(探索も限定的)。
- 未検証: ECS の自動並列化(system の読み書き宣言によるスケジューリング)。一般に語られる利点だが、今回の取得本文では確認していない。

## 4. Rendering の基礎と mobile で必須のもの

### Takeaway
Mobile GPU の支配的な制約はメモリ帯域と電力で、tile memory の外に出るデータを減らす設計が中心になる。Desktop の定石(巨大な G-buffer、多数の中間パス)はそのままでは高くつく。

### Cited Findings
**Forward / Deferred**
- Forward は object ごとに全ライトを計算し、深度の重なりで無駄が出る。Deferred は geometry pass で G-buffer に書き、lighting pass で一度だけ計算するので多ライトに強い。欠点はメモリ、透明物(別途 forward が必要)、MSAA との相性で、少数ライトでは forward より遅いこともある — [LearnOpenGL: Deferred Shading](https://learnopengl.com/Advanced-Lighting/Deferred-Shading)

**PBR**
- Filament の設計資料は mobile リアルタイム性能を第一目標とし(OpenGL ES 3.x 級)、パラメータは base color / metallic / roughness / reflectance / emissive / AO。物理単位を使うことでライティングを再利用できる。mediump の精度問題に対応して roughness を約0.089に下限 clamp する — [Filament Materials/PBR doc](https://google.github.io/filament/Filament.md.html)

**Shadow**
- Shadow map は光源視点の深度を描き、通常描画時に比較する。典型的な artifact は shadow acne(bias で緩和)、peter panning(bias 過多)、aliasing(PCF で緩和) — [LearnOpenGL: Shadow Mapping](https://learnopengl.com/Advanced-Lighting/Shadows/Shadow-Mapping)

**Culling / Instancing / Draw call**
- Frustum culling は bounding volume(sphere / AABB)で視野外を除外する。多数の entity には octree などの空間分割を併用する。デモでは約15%のみ GPU に送った — [LearnOpenGL: Frustum Culling](https://learnopengl.com/Guest-Articles/2021/Scene/Frustum-Culling)
- Draw call のコストは主に CPU 側の状態準備にある。Instancing は1回の呼び出しで多数のコピーを描く。例では1,000個で約1,001回の call が2回に減り、100,000個まで拡張 — [LearnOpenGL: Instancing](https://learnopengl.com/Advanced-OpenGL/Instancing)(OpenGL 前提の説明であり、低オーバーヘッド API では比率が変わりうる)
- Draw call の目安は資料間で食い違う: Arm 関係者の投稿で約500 — [Arm forum](https://community.arm.com/support-forums/f/graphics-gaming-and-vr-forum/48848/overhead-of-the-driver)。Arm のブログ(古い記事)では高品質3Dで約100/render target、2Dで20〜30 — [Arm blog](https://community.arm.com/developer/tools-software/graphics/b/blog/posts/mali-performance-1-checking-the-pipeline)。固定の予算とせず実機計測が妥当。なお数値は検索抜粋に基づく。

**Render graph**
- Frostbite の FrameGraph(GDC 2017)は全パスと resource のグラフに基づく抽象化で、機能を疎結合に実装できる。毎フレーム構築し setup / compile / execute の3段階を踏む。transient resource の確保と memory aliasing を担う — [GDC Vault](https://gdcvault.com/play/1024612/FrameGraph-Extensible-Rendering-Architecture-in)、[slides](https://www.slideshare.net/DICEStudio/framegraph-extensible-rendering-architecture-in-frostbite)
- 再実装の例では不要パスの除去と lifetime 計算を行う — [fragr](https://hackage-content.haskell.org/package/fragr)。barrier の自動挿入は二次情報 — [DEV Community](https://dev.to/p3ngu1nzz/advanced-vulkan-rendering-building-a-modern-frame-graph-and-memory-management-system-15kn)

**Tile-based GPU**
- IMR は fragment をすぐメモリに書く。Tiler は framebuffer を tile に分け、binning pass の後に pixel pass を行い、結果を tile ごとに一度書き出す。Geometry を2回処理するのが典型 — [Vulkan Guide: TBR Best Practices](https://docs.vulkan.org/guide/latest/tile_based_rendering_best_practices.html)
- メモリ帯域がしばしば支配的な性能要因で、tile memory に留めれば高速かつ省電力 — [同上](https://docs.vulkan.org/guide/latest/tile_based_rendering_best_practices.html)
- Tile memory はデバイスメモリより帯域も遅延も数倍以上優れ、エネルギーも大幅に少ない — [Apple: TBDR](https://developer.apple.com/documentation/metal/tailor-your-apps-for-apple-gpus-and-tile-based-deferred-rendering.md)
- 用語の罠: Apple は deferred shading を「TBDR とは無関係の lighting 手法」と明記し、G-buffer の書き戻しは帯域を食うが tile 内で完結させれば中間テクスチャを省けるとする — [同上](https://developer.apple.com/documentation/metal/tailor-your-apps-for-apple-gpus-and-tile-based-deferred-rendering.md)
- 実践項目: `CLEAR` / `DONT_CARE` の load op、不要 attachment の `DONT_CARE` store op、MSAA の on-chip resolve(`vkCmdResolveImage` を避ける)、lazily allocated の transient attachment、subpass merge や `VK_KHR_dynamic_rendering_local_read`、vertex position の別バッファ化(binning 帯域削減) — [Vulkan Guide](https://docs.vulkan.org/guide/latest/tile_based_rendering_best_practices.html)
- Overdraw は tiler では HSR により軽減されることが多いが、対応は GPU 次第でコストはゼロではない。`discard` や `gl_FragDepth` は early depth を無効化しうる。新しい GPU には fragment pre-pass を持つものがある — [同上](https://docs.vulkan.org/guide/latest/tile_based_rendering_best_practices.html)
- Subpass merge の注意: Arm GPU は fragment shading の前に binning を完了する必要があり、merge は初期 geometry 負荷を増やす。G-buffer が on-chip 予算を超えると tile が小さくなりうる — [Arm: Render subpasses](https://arm-software.github.io/vulkan_best_practice_for_mobile_developers/samples/performance/render_subpasses/render_subpasses_tutorial.html)、[Arm blog](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/vulkan-subpasses-the-good-the-bad-and-the-ugly)

### Inferences
Mobile の重要度(筆者の判断):
- 必須: frustum culling、instancing / batching、texture 圧縮、load/store op の正確な指定、解像度スケーリング、PBR の軽量版または unlit
- 重要: LOD、shadow map の軽量化、post-process パス数の抑制(各フルスクリーンパスは tile 外への書き戻しを生みやすい)、render graph(transient の宣言を自動化できる)
- 条件付き: deferred(subpass merge が効く場合に限る)、高度な GI、ray tracing

Render graph の mobile での価値は、aliasing だけでなくパス結合と load/store の自動推論にありうる。比較では「tile を意識した render pass 結合をするか」を確認項目にする。

### Gaps
- 未検証(一般知識): Forward+/clustered forward は tile や cluster ごとにライトを割り当てる手法。LOD は距離に応じたメッシュ切替で、微小三角形の抑制にも効く。Batching は同一 material の結合や state 変更削減。Post-processing は bloom / tone mapping / TAA など。これらの本文資料は今回未取得。Filament の clustered forward 採用理由も読んだ範囲に記述がなかった。
- Arm の [GPU Best Practices Guide](https://support.arm.com/documentation/101897/latest/) は本文を取得できなかった(JS 描画と推定)。Mali 固有の数値予算は未確認。
- 未検証: Apple の memoryless storage(Vulkan の transient に相当するとされる)。取得した TBDR 記事の本文には記述がなかった。

## 5. Animation、physics、audio、asset pipeline、scene format

### Takeaway
どれも「authoring データを runtime 向けに変換し、実行時は予算内で評価する」構造を持つ。Physics は固定 step、audio は独立スレッドという、game loop とは異なる時間制約を持つ点が重要。

### Cited Findings
**Animation**
- glTF の定義: skinning は頂点ごとの変換の計算と適用、morph target は頂点属性の差分、animation は timestamp つき keyframe と対象プロパティ — [glTF 2.0 Spec](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html)
- 実行時パイプライン例: clip サンプリング → ブレンド → IK → ボーン行列を GPU へ。state machine と blend tree が上位制御。ただし小規模プロジェクトの例で標準ではない — [SparkEngine wiki](https://github.com/Krilliac/SparkEngine/wiki/Animation)
- State machine は状態のグラフで各状態が clip か blend tree を参照し、blend tree は float パラメータで類似 clip を混ぜる — [Unity 5.1 glossary](https://docs.unity3d.com/510/Documentation/Manual/AnimationGlossary.html)(古い資料、語彙の確認用)
- Linear blend skinning が最も広く使われ、dual quaternion は関節の潰れを避ける代替。IK は目標から関節角を解く — [mocaponline](https://mocaponline.com/blogs/mocap-news/skeletal-mesh-animation)(二次資料)

**Physics**
- Box2D は 1/60 s の固定 step と 4 sub-step を推奨(1/30 以下の step を許容) — [Box2D Simulation](https://box2d.org/documentation/md_simulation.html)
- 接触は AABB の重なり開始で作られ、実際に触れる前から存在しうる。静止物は sleep する。CCD は既定で動的物体の静的物体すり抜けを防ぎ、bullet 指定で対象を広げる。sensor は応答なしの重なり検出 — [同上](https://box2d.org/documentation/md_simulation.html)
- Solver は反復法(PGS ≒ Sequential Impulses)の系統が中心で、Catto は Gauss-Seidel 系 8 種を比較。Box2D v3 は Soft Step 系 — [Solver2D](https://box2d.org/posts/2024/02/solver2d/)、[box2d README](https://github.com/erincatto/box2d)

**Audio**
- Callback 型 API がハードウェアの要求に応じて呼ばれる。遅れると音切れ、先行しすぎると latency になり、Unreal は複数バッファで対処。platform 層が音量、pitch、spatialization、DSP を担う — [Unreal Audio Mixer](https://dev.epicgames.com/documentation/en-us/unreal-engine/audio-mixer-overview-in-unreal-engine)
- Bus は submix で reverb などを持つ(ミキサーの比喩) — [CRI](https://blog.criware.com/index.php/wp-json/wp/v2/posts/4875)
- Render 側では allocation / lock / OS 呼び出しを禁止し、wait-free queue で入力を受け、voice 上限超過時に古いものを steal する実装例 — [cranpose-audio](https://docs.rs/crate/cranpose-audio/latest/source/src/mixer.rs)(小規模実装の例)

**Asset / scene**
- glTF は「transmission format」で、runtime 効率と API 中立性を重視する。Authoring format ではなく authoring データを保持しない。ストリーミング形式でも runtime でもなく、描画の振る舞いを規定しない。`.gltf` + `.bin`、または `.glb` — [glTF 2.0 Spec](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html)
- Scene は root node のリスト、node は階層と local transform を持つ — [同上](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html)
- KTX は GPU テクスチャのコンテナで、KTX 2.0 は Basis Universal を格納し、実行時に各種 GPU 形式へ transcode できる。ETC1S は小容量、UASTC は高品質。`KHR_texture_basisu` で glTF から利用する — [Khronos KTX](https://www.khronos.org/ktx/)
- GPU 圧縮形式は生 RGBA より高速かつ省メモリ。ASTC はブロックサイズで品質と圧縮率を調整でき Play デバイスの >80%。ETC2 は GLES 3.0+ と Vulkan で利用でき >95%、Google は既定形式に推奨。BCn は Play Games for PC 向け。Play は端末ごとに形式を配信でき、engine の runtime 変換は読み込みや性能を損なう場合がある — [Android: Texture compression](https://developer.android.com/guide/playcore/asset-delivery/texture-compression)(ページの日付は不明)
- ASTC は ETC より品質/サイズで有利だがエンコードが遅い — [Arm slides 2018](https://www.slideshare.net/OwenWu/mobile-graphics-optimization-guides)

### Inferences
- Asset pipeline は「source(DCC / PSD)→ import → cook(platform 別圧縮、mesh 最適化、shader コンパイル)→ package → runtime load」と整理できる。cook が engine の価値の大部分を占めるので、ランタイムだけの engine(Bevy や Flame のような軽量構成が該当するかは他の調査者が確認)との差が出やすい。
- 比較軸: 独自 scene 形式か glTF 取り込みか、テキスト形式か(diff やマージのしやすさ)、prefab やネストの有無、hot reload。
- 2D engine では skeletal / PBR / shadow などが省かれ、sprite、atlas、tilemap、2D 物理が中心になる。3D 基準の表でそのまま比較すると不公平になるため、「2D 向けに該当なし」を区別すべき。

### Gaps
- 未検証(一般知識): broadphase は AABB tree や sweep-and-prune などで候補ペアを絞る工程、narrowphase は GJK / SAT などで接触点を求める工程。今回の Box2D 本文はこの構造を明示しなかった(一部未読)。Rapier や Jolt の資料も未確認。
- 未検証: Basis の transcode 先に ASTC / ETC2 / BC が含まれること。Khronos ページは形式名を挙げていない。
- HRTF など spatialization の詳細資料は未確認。GPU skinning の一次資料も見つからなかった。Animation 節の資料は全体に弱く、Gregory 13 章での確認が望ましい。
- Cooking の製品非依存な一次資料は未取得(Gregory 1.6 の本文が該当するはず)。

## 6. Runtime と tooling/editor の分離、must-have と optional

### Takeaway
Gregory も runtime と tools/asset pipeline を別の節に分けており、runtime に載らない制作系が engine の実力を大きく左右する。must-have の線引きに権威ある基準はなく、以下は対象ジャンルに依存する判断である。

### Cited Findings
- 第4版 1.5 Runtime Engine Architecture と 1.6 Tools and the Asset Pipeline を分離。別に第10章 Tools for Debugging and Development がある — [TOC](https://www.gameenginebook.com/toc.html)
- glTF は authoring 形式と補完関係にあり、配布のための「蒸留」先と位置づけられる — [glTF Spec](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html)

### Inferences
筆者の暫定分類(比較表の行候補、要レビュー):

| 区分 | 項目 |
|---|---|
| Runtime 必須 | game loop、入力、renderer(2D または 3D)、asset 読み込み、audio 再生、object model、platform 層、ライフサイクル処理 |
| Runtime 準必須 | animation、collision/physics、UI、text、streaming |
| Runtime 任意 | scripting、networking、navmesh、particles/VFX の高度な機能、GI |
| Tools 必須 | asset import/cook、ビルドとパッケージング、profiler / debugger |
| Tools 準必須 | scene editor、animation editor、hot reload |
| Tools 任意 | 統合 material graph、visual scripting、marketplace |

Editor がなくても code-first の engine として成立するが、その場合は外部ツール(Blender、Tiled など)と import の質に依存する。「editor は runtime に同梱されるか」「editor 自身がその engine で動くか」は比較に使える。

### Gaps
- must-have の線引きを定めた権威ある資料は見つからなかった。上表は推論である。
- Gregory 1.6 の本文未読。

## 7. Mobile 固有の制約

### Takeaway
Mobile の性能設計は「ピークではなく持続」を目標とする。熱で性能が変動し、OS が surface、GPU、メモリをいつでも取り上げうる前提で engine を作る必要がある。

### Cited Findings
**Thermal**
- Android Thermal API: `getThermalHeadroom` は API 30、NDK は API 31。0.0(なし)〜1.0(`SEVERE`)で N 秒先を予測。呼び出しは10秒に1回以下(超えると `NaN`)。status と headroom の対応は端末依存 — [Android: Thermal API](https://developer.android.com/games/optimize/adpf/thermal)
- 対応策: 早めにフレームレートや解像度、fidelity を下げる。過熱後は持続水準より下げて放熱し、段階的に戻す — [同上](https://developer.android.com/games/optimize/adpf/thermal)
- ADPF は Thermal API、Game Mode / State、Fixed Performance Mode、Power Efficiency Mode(Android 15+)などからなる — [Android: ADPF](https://developer.android.com/games/optimize/adpf)
- iOS: 低電力モードや発熱時に高リフレッシュレートが無効化されうる。発熱時はフレームレートが下がりうる。また refresh rate に反応して描画品質を動的に変えると GPU throttle を招きうるので、振動するフィードバックループを避け、フレームレートごとの code path を実機で検証する — [Apple: ProMotion](https://developer.apple.com/documentation/quartzcore/optimizing-iphone-and-ipad-apps-to-support-promotion-displays.md)

**Memory**
- iOS のメモリ上限は端末依存で、超えると終了させられる。前景で終了するとユーザーからはアプリが消えたように見える。`EXC_RESOURCE` / `MEMORY` は上限接近の警告で、終了を意味しない。計測はページ単位(通常 16 KB)で、dirty memory が計上される。背景の app をストレージへ退避することがあり、復帰に遅延が出る — [Apple: Reducing memory use](https://developer.apple.com/documentation/xcode/reducing-your-app-s-memory-use.md)

**App pausing**
- iOS/tvOS は背景アプリの GPU 利用を制限する。非アクティブ通知で新規の提出を止め、`applicationDidEnterBackground` で `waitUntilScheduled()` まで済ませる。背景での提出は `MTLCommandBufferError.Code.notPermitted` になる — [Apple: Metal background](https://developer.apple.com/documentation/metal/preparing-your-metal-app-to-run-in-the-background.md)
- Android では背景化で surface が破棄されうる。acquire 後 present 前でも起こるので、swapchain だけでなく surface の再作成も必要 — [SDL 修正](https://discourse.libsdl.org/t/sdl-gpu-vulkan-handle-vk-error-surface-lost-khr-in-acquire-path/67125)、[SDL 修正2](https://discourse.libsdl.org/t/sdl-gpu-better-detection-of-surface-destruction/65318)。Pause で swapchain を破棄し resume で再構築する例 — [Mali Vulkan SDK](https://arm-software.github.io/vulkan-sdk/class_mali_s_d_k_1_1_android_platform.html)

**GPU API**
- Android: Vulkan は「primary low-level graphics API」で OpenGL ES は機能開発停止(サポート継続)。新規は Vulkan を推奨。API 24 から利用可、Android 10 以降の 64-bit 端末は Vulkan 1.1、アクティブ端末の 85% が対応。Android Baseline profile あり — [Android: Vulkan overview](https://developer.android.com/games/develop/vulkan/overview)
- ANGLE: Android 15+ で任意の層。最終的に GLES は ANGLE 経由のみになる見通しだが、全端末で GLES サポートは継続。Android 17 から manifest で prefer を指定可能 — [同上](https://developer.android.com/games/develop/vulkan/overview)
- 2026年時点で GLES 移行を義務づける Play ポリシーは検索では確認できなかった(不在の証明ではない)。
- wgpu 30.0.1(docs.rs 表示時点): Vulkan は Windows/Linux/Android で first-class、Metal は macOS/iOS で first-class、GLES 3.0+ は Linux/Android で downlevel/best-effort、MoltenVK と ANGLE は任意 — [wgpu docs](https://docs.rs/wgpu/latest/wgpu/)、[wgpu README](https://raw.githubusercontent.com/gfx-rs/wgpu/trunk/README.md)

**Frame pacing**
- Android の問題: 60 Hz 端末で 30 FPS を回すと 49 / 16 / 33 ms のような不均一が起きる(stutter)。全力提出は queue-stuffing で1フレームの遅延を生む。Choreographer 単体では不十分 — [Android: Frame Pacing](https://developer.android.com/games/sdk/frame-pacing)
- Swappy: Choreographer、presentation timestamp、sync fence を使い、refresh rate の選択も行う。GLES は `EGL_ANDROID_presentation_time`、Vulkan は `VK_GOOGLE_display_timing`。Mir 2 では slow session が 40% → 10%(Google 掲載の事例) — [同上](https://developer.android.com/games/sdk/frame-pacing)
- iOS: `CADisplayLink` を使い、タイマーは使わない。animation や physics は常に `targetTimestamp` で駆動する。callback が `targetTimestamp` を超えると次の interval の callback は来ない。ProMotion は iPhone で 10〜120 Hz、iPad Pro で 24〜120 Hz。iPhone で 60 Hz 超には `CADisableMinimumFrameDurationOnPhone` が必要。iOS 15+ でゲームは 30/60 Hz の優先を得られる。実 refresh rate は強制できず、どの rate でも動くよう備える — [Apple: ProMotion](https://developer.apple.com/documentation/quartzcore/optimizing-iphone-and-ipad-apps-to-support-promotion-displays.md)

### Inferences
- 比較チェックリスト候補: Swappy 統合の有無、`targetTimestamp` の利用、thermal API との連携(自動品質調整)、resume 時の surface / GPU resource 復元、低メモリ時の cache 破棄、GLES fallback の有無(非対応の約15%をどこまで追うか)。
- 固定 step + 補間は可変 refresh rate(10〜120 Hz)と相性がよい。一方、fixed step の spiral of death は throttle 時に現実の問題となりうる。
- Android の「headroom に応じて下げる」と Apple の「振動を避ける」は矛盾せず、ヒステリシスつきの品質制御が両立案となる。
- App への埋め込み(フルスクリーン engine か、UI の一部か)は、loop の所有、surface 管理、メモリ予算に直結する。Whisker のようなアプリ UI フレームワーク視点では特に重要な軸となる。

### Gaps
- 未検証: Android Performance Hint Session の詳細(API 31 の `PerformanceHintManager` とされる)。取得ページに本文がなかった。
- 未検証: `os_proc_available_memory`、jetsam の詳細。Apple の該当子ページ(Identifying high-memory use with jetsam event reports)は未読。Apple はアプリ別の上限値を公表ページに記載していない(今回の読了範囲)。
- Android 側の lifecycle は公式ドキュメントでなく SDL の修正記録と Mali サンプル(古い)に依拠している。LMK(Low Memory Killer)、`onTrimMemory` は未調査。
- iOS での frame pacing に関する Metal 専用の推奨(`presentDrawable afterMinimumDuration` など)は未確認。
- iOS 26 系で `accessRevoked` によるクラッシュ報告が1件あるが([whisper.cpp issue](https://github.com/ggml-org/whisper.cpp/issues/3531))、一般化はできない。
