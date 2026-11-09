# Flame と軽量・モバイル向けゲームエンジン、および UI フレームワークとの共存

調査日: 2026-10-31。版数と日付は、特記がなければ pub.dev API / GitHub Releases の値。

用語の補足(ゲーム開発に馴染みのない読者向け):
- game loop: 毎フレーム「状態更新 → 描画」を回す処理。
- hitbox: 衝突判定に使う形状。
- broad phase / narrow phase: 衝突の候補を粗く絞る段階と、精密に判定する段階。
- tunneling: 高速な物体が 1 フレームの間にすり抜ける現象。
- sprite atlas / sprite sheet: 複数の画像を 1 枚にまとめたテクスチャ。
- ECS: Entity Component System。データと処理を分離する設計。

## Flame は何を提供するか(core と bridge の境界、FCS、描画、3D、限界)

### Takeaway
Flame の core は「2D の game loop、component tree、sprite、effects、particles、camera、collision 判定、input、overlay」までです。物理、音声、tilemap、skeletal animation は bridge package に切り出されています。描画は Flutter の Canvas に乗り、3D は `flutter_gpu` に依存する別系統で、まだ experimental です。比較表の「下限」としては、この core + bridge の線引きが最も参考になります。

### Cited Findings

**版と要件**
- 安定版は flame 1.38.2(2026-08-27)、SDK 制約は Dart >=3.11 / Flutter >=3.41。prerelease は 2.0.0-dev.1(2026-10-06)— [pub.dev API](https://pub.dev/api/packages/flame)
- 2.0.0-dev.1 の最小要件は Flutter 3.47.0 / Dart 3.13.0 — [flame CHANGELOG](https://raw.githubusercontent.com/flame-engine/flame/main/packages/flame/CHANGELOG.md)
- 自己定義は "A minimalist Flutter game engine"。互いにある程度独立したモジュールから選んで使う設計 — [pub.dev](https://pub.dev/packages/flame)
- 対応 platform は Android / iOS / Linux / macOS / web / Windows — [pub.dev](https://pub.dev/packages/flame)
- GitHub は約 10.8k stars — [GitHub](https://github.com/flame-engine/flame)

**2.0 で予定されている破壊的変更(dev 版であり未確定)**
- children コンテナを OrderedSet からフラットな sorted-array の ComponentList に置換 — [CHANGELOG](https://raw.githubusercontent.com/flame-engine/flame/main/packages/flame/CHANGELOG.md)
- particle system を data-oriented な実装に置換 — 同上
- `GameWidget.controlled` を `GameWidget.managed` に改名 — 同上
- 旧 gesture detector 群を callback mixin に統一 — 同上
- `add` / `addAll` を同期メソッド化 — 同上
- `WidgetComponent` と `flame_cli` を追加 — 同上

**package の構成**
- Bridge: flame_3d_component, flame_audio, flame_behavior_tree, flame_bloc, flame_fire_atlas, flame_forge2d, flame_gamepads, flame_isolate, flame_kenney_xml, flame_lottie, flame_markdown, flame_network_assets, flame_noise, flame_rive, flame_riverpod, flame_spine, flame_sprite_fusion, flame_svg, flame_texturepacker, flame_tiled, flame_typled — [GitHub](https://github.com/flame-engine/flame)
- その他の公式拡張: flame_3d, flame_behaviors, flame_console, flame_splash_screen, flame_steering_behaviors — 同上
- Tooling: flame_cli, flame_lint, flame_test — 同上

| package | 版 / 公開日 | 依存先 | 役割 |
|---|---|---|---|
| flame_forge2d | 0.20.0 / 2026-08-17 | forge2d ^0.15.1 | 物理エンジン。forge2d 0.15 で Dart 移植版から **Box2D v3 の bindings** に置換済み |
| flame_audio | 2.12.2 / 2026-07-19 | audioplayers ^6.2.0 | audioplayers の "thin wrapper"。SFX、BGM、cache、AudioPool |
| flame_tiled | 3.1.2 / 2026-07-19 | tiled ^0.11.0 | Tiled 形式の tilemap |
| flame_rive | 1.11.2 / 2026-07-19 | rive ^0.14.6 | Rive アニメーション |
| flame_spine | 0.3.1+6 / 2026-07-19 | spine_flutter ^4.3.0 | Spine の skeletal animation |
| flame_3d | 0.3.2 / 2026-10-02 | flutter_gpu (SDK) | 実験的 3D |
| flame_3d_component | 0.1.0-dev.1 / 2026-10-06 | flame ^2.0.0-dev.1, flutter_scene ^0.23.0 | flutter_scene の scene を 2D tree に描く |

出典: [pub.dev forge2d 説明](https://pub.dev/packages/flame_forge2d)、[flame_audio](https://pub.dev/packages/flame_audio)、API([flame_tiled](https://pub.dev/api/packages/flame_tiled)、[flame_rive](https://pub.dev/api/packages/flame_rive)、[flame_spine](https://pub.dev/api/packages/flame_spine)、[flame_3d](https://pub.dev/api/packages/flame_3d)、[flame_3d_component](https://pub.dev/api/packages/flame_3d_component))

**FCS と game loop**
- `FlameGame` は `Component` を継承し、component tree(FCS)の root であり game loop の実行主体 — [docs: game](https://docs.flame-engine.org/latest/flame/game.html)
- 組み込みの `world` と `camera` を持つ — 同上
- 毎 tick、全 component に対し `update(dt)` の後に `render(canvas)` を呼ぶ。`dt` は秒単位 — 同上
- Game の lifecycle は `onGameResize` → `onLoad` → `onMount`。sizing を asset 読み込みより先に行うため、他の component とは順序が逆 — 同上
- `pauseEngine` / `resumeEngine` / `stepEngine` を持つ — 同上
- `onHotReload` は debug 時のみ呼ばれる — 同上
- `pauseWhenBackgrounded` による自動 pause は Android / iOS のみ — 同上
- 別実装の ECS である Oxygen / `flame_oxygen` は deprecated。性能はほぼ同等なので FCS を使うよう案内されている — [docs: oxygen](https://docs.flame-engine.org/latest/other_modules/oxygen/oxygen.html)

**sprite / animation**
- `Sprite`、`SpriteSheet`、`SpriteAnimation`、`SpriteBatch`、`ImageComposition` を提供 — [docs: images](https://docs.flame-engine.org/latest/flame/rendering/images.html)
- Aseprite の JSON を読めるが、trimmed sprite sheet は非対応 — 同上
- 画像形式は Flutter がサポートするものに従う。SVG は `flame_svg` が必要 — 同上
- 組み込みの network loader はない — 同上

**effects**
- effect 自体が component で、target に付いて property を変更し、完了すると既定で自動的に外れる — [docs: effects](https://docs.flame-engine.org/latest/flame/effects/effects.html)
- 種類は Move / Rotate / Scale / Size / Anchor / Color / Opacity / Sequence / Combined / Remove / Function — 同上
- 進行は effect controller が決める — 同上

**particles**
- 1.38 系は `Particle` を合成して tree を作り、`ParticleSystemComponent` で包む。Moving / Accelerated / Sprite / SpriteAnimation / Computed などがある — [docs: particles](https://docs.flame-engine.org/latest/flame/rendering/particles.html)
- CPU か GPU か、個数の上限については文書に記載なし — 同上

**collision**
- `HasCollisionDetection` と `CollisionCallbacks` の mixin で使う — [docs: collision](https://docs.flame-engine.org/latest/flame/collision_detection.html)
- hitbox は rectangle / circle / polygon(polygon は convex 必須)— 同上
- broad phase は既定で sweep and prune。collidable が約 100 超の大きなフィールド向けに quad tree があるが、既定より遅くなることもあると警告されている — 同上
- raycast と raytrace を提供 — 同上
- 限界: 衝突後の挙動は決めない。tunneling を考慮しない。同一 step の複数衝突の callback 順序は事実上ランダム — 同上
- 公式に「物理エンジンではない」とされ、力や joint が必要なら Forge2D を勧めている — 同上

**camera**
- `CameraComponent` が Viewport、Viewfinder、backdrop を持ち `World` を描く。1 つの world を複数の camera で見られる — [docs: camera](https://docs.flame-engine.org/latest/flame/camera.html)
- viewport に追加した子は HUD になる — 同上
- `follow`、`setBounds`、`visibleWorldRect`、`canSee`(culling 用)を提供 — 同上

**input と navigation**
- input は mixin を component に付ける方式で、Flutter の GestureDetector に似ている。touch、mouse、keyboard、pointer を扱う — [docs: inputs](https://docs.flame-engine.org/latest/flame/inputs/inputs.html)
- `RouterComponent` は Flutter の Navigator に相当する component 版。`OverlayRoute` で Flutter widget の overlay も扱える — [docs: router](https://docs.flame-engine.org/latest/flame/router.html)

**overlay と widget の相互運用**
- `GameWidget` は普通の widget で、Flutter tree のどこにでも置ける。`overlayBuilderMap` に名前と builder を登録し、`game.overlays.add(id)` で表示を切り替える — [docs: overlays](https://docs.flame-engine.org/latest/flame/overlays.html)
- 同じ文書内に「描画順は `overlayBuilderMap` の key 順」と「`add` の priority 引数」の両方の記述があり、関係は判然としない — 同上
- game は `build` の外で生成する必要がある。tree の rebuild で再生成されるのを防ぐため — [docs: game](https://docs.flame-engine.org/latest/flame/game.html)

**描画基盤**
- 2D は `render(canvas)` で Flutter の Canvas に描く — [docs: game](https://docs.flame-engine.org/latest/flame/game.html)
- Canvas の下の renderer(Flutter 3.47 時点、[Flutter docs: Impeller](https://docs.flutter.dev/perf/impeller)):
  - iOS は Impeller のみ
  - Android は API 29+ で既定。古い端末や Vulkan なしの端末は legacy OpenGL に fallback
  - macOS / Linux / Windows は 3.47 から既定
  - web は Skia
- flame_3d は Flutter 3.47.0 以上と Impeller が必要で、Android / iOS / macOS に対応。Windows / Linux は非対応、web は WebGPU 経由で実験的 — [pub.dev: flame_3d](https://pub.dev/packages/flame_3d)
- flame_3d は semver に従わず、"Please do not use this for production environments" と明記 — 同上
- flame_3d_component は「3D ゲームエンジンではない」と自ら述べており、本格的な 3D には flutter_scene か flame_3d を勧めている — [pub.dev: flame_3d_component](https://pub.dev/packages/flame_3d_component)
- flame_3d_component は `flutter_scene ^0.23.0` に依存し、最新の 0.24.3 は pre-1.0 の caret 規則では範囲外。3D 層の変化の速さを示す例 — [flame_3d_component API](https://pub.dev/api/packages/flame_3d_component)、[flutter_scene API](https://pub.dev/api/packages/flutter_scene)
- 性能の指針は、frame ごとの object 生成を避ける、`CollisionType.passive` を使う、`ComponentPool` を使う、の 3 点。数値上限の記載はなく、ハードウェア次第とだけある — [docs: performance](https://docs.flame-engine.org/latest/flame/other/performance.html)

### Inferences
- Flame の価値の中心は描画ではなく、**component tree、lifecycle、collision、effects という「ゲーム用の時間とオブジェクトの管理」** です。描画、text、画像 decode、gesture は Flutter に委譲しています。
- 物理と音声が外部ライブラリの薄い wrapper で済むという事実は、Whisker にとっても core に入れなくてよい候補を示しています。
- forge2d が Dart 移植から Box2D v3 bindings に移ったことは、物理を host 言語で自前実装する路線の割に合わなさを示唆します。ただし移行理由の一次記述は確認していません。Rust なら rapier や Box2D binding が同じ位置に来ると考えられます。
- 2.0-dev の flat array 化と data-oriented particles は、component tree の性能が実際に課題だったことを示唆します。Whisker が retained な tree を流用するなら同じ問題に当たる可能性があります。

### Gaps
- `flame_3d` の具体的な機能水準(skeletal animation 以外の lighting / shadow など)は詳細未確認です。
- `SpriteBatch` が内部で `drawAtlas` を使うか、1 frame あたりの現実的な sprite 数は、公式文書で確認できませんでした。独立した benchmark も未入手です。
- `docs.flame-engine.org/latest` は 1.38.2 の内容で、2.0-dev の変更は CHANGELOG でしか確認していません。
- 2.0 の安定版の時期は不明です。
- `WidgetComponent` の仕様(canvas 上に widget をどう合成するか)は CHANGELOG の 1 行しか確認していません。
- input event の伝播規則は、取得した概要ページに記載がありませんでした。

## 他の軽量エンジンはドメインごとに何を提供するか

### Takeaway
同じ「軽量」でも 3 層に分かれます。① Defold / Cocos Creator のようにエディタ込みの統合エンジン、② SpriteKit / RealityKit / Flame のように host の UI フレームワークに組み込まれたエンジン、③ raylib / three.js / RN Game Engine のようなライブラリです。runtime サイズは Defold の約 1〜3 MB が下限の目安です。

### Cited Findings

**版の一覧**

| 対象 | 確認した版 | 確認元 / 注記 |
|---|---|---|
| Defold | 1.14.0(2026-10-12) | [releases](https://github.com/defold/defold/releases) |
| Cocos Creator | 3.8.8(2025-12-16)、3.8.9 開発中、Cocos 4 は alpha | [二次情報](https://www.issoh.co.jp/md/tech/4880)。3.8.8 の内容自体は[公式](https://cocos.com/en/creator-download)で確認 |
| Solar2D | build 3736(2026-10-12) | [releases](https://github.com/coronalabs/corona/releases) |
| raylib | 6.2(2026-10-15) | [releases](https://github.com/raysan5/raylib/releases) |
| KorGE | v6.0.0(2025-05-16)。**1 年以上更新なし** | [releases](https://github.com/korlibs/korge/releases) |
| react-native-game-engine | 最終 push 2024-12-31。**約 2 年停滞**(archived ではない) | [GitHub API](https://api.github.com/repos/bberak/react-native-game-engine) |
| react-native-skia | `@shopify/react-native-skia` 2.14.0(2026-10-01)。v3 は別 package 名 `react-native-skia`(v3 の版番号は未確認) | [npm](https://registry.npmjs.org/@shopify/react-native-skia/latest)、[README](https://raw.githubusercontent.com/Shopify/react-native-skia/main/README.md) |
| three.js | r187(2026-10-30) | [releases](https://github.com/mrdoob/three.js/releases) |
| Babylon.js | 9.33.1(2026-10-30) | [releases](https://github.com/BabylonJS/Babylon.js/releases) |
| flutter_scene | 0.24.3(2026-10-09)、pre-1.0 | [pub.dev API](https://pub.dev/api/packages/flutter_scene) |

**ドメイン別カタログ**

出典は表の下に engine ごとに記載します。「—」は調査した一次資料で確認できなかったことを示し、機能がないという断定ではありません。

| ドメイン | Flame 1.38 | Defold | Cocos Creator 3.8 | SpriteKit | RealityKit | raylib | RN Game Engine |
|---|---|---|---|---|---|---|---|
| 描画 | 2D Canvas、shader、post process | 2D/3D、render script、material、compute、PBR | 2D/3D、render pipeline、material、shader | 2D(framework の abstract は "high-performance 2D content") | 3D、post process | 2D/3D、OpenGL、shader | なし(自前) |
| animation | sprite、effects、bridge で Rive / Spine / Lottie | flipbook、model、tween、Rive、Spine | clip、skeletal、Marionette、procedural | — | clip の再生 | skeletal | なし |
| 物理 | 判定のみ、Forge2D は bridge | Box2D(2D)+ Bullet(3D) | 3D: Bullet / cannon / PhysX / builtin、2D あり | — | — | なし(一覧に記載なし) | なし(Matter JS を案内) |
| 音声 | bridge(audioplayers) | sound、streaming、FMOD | AudioSource | — | — | streaming | — |
| 入力 | touch、mouse、keyboard、gamepad は bridge | keyboard、text、mouse、touch、gamepad | — | — | — | — | multi-touch event |
| UI | Flutter overlay | 専用 GUI(box、text、pie 等) | UI component 群 | SwiftUI の `SpriteView` | SwiftUI の `RealityView` | raygui(即時モード) | RN 自体 |
| asset | Images cache、texture packer は bridge | atlas、font、tile source、live update | Asset Manager、bundle | — | — | rres など | なし |
| tooling | CLI、console、devtools 拡張、hot reload | エディタ、デバッガ、hot reload | エディタ、feature cropping | — | — | なし | なし |
| 言語 | Dart | Lua + C++/C# 拡張 | — | — | Swift | C99 + 60 超の binding | JS |

**engine 別の出典と補足**

- **Defold**
  - 機能のドメイン一覧 — [manual](https://defold.com/manuals/introduction/)
  - frame の順序は input → update(fixed_update / update / late_update、physics を含む)→ render update → post update → render。message を挟みながら進む — [lifecycle](https://defold.com/manuals/application-lifecycle/)
  - 物理は 2D が Box2D、3D が Bullet。2D は legacy が既定で Box2D v3 は opt-in、None で除外も可能 — [physics](https://defold.com/manuals/physics/)
  - collection proxy が異なると physics world も別になる — 同上
  - 空プロジェクトの release サイズは Android arm64 2.56 MB、iOS arm64 .ipa 1.35 MB。同表の比較値は Unity 31.75 / 36.81 MB、Godot 25.29 / 25.29 MB — [Defold 公式](https://defold.com/gamescom2025/)
  - **注意:** 同じページの本文には別の値(Android 1.97 MB、iOS 1.47 MB)があり矛盾します。Defold 自身による競合比較でもあり、Unity 6.5 / Godot 4.7.1 / Defold 1.13.0 時点の値です。Godot の Android と iOS が同値である点も確認が必要です。計測の定義は [build-size](https://github.com/defold/build-size) に記載があります。
- **Cocos Creator**
  - ドメイン一覧 — [manual](https://docs.cocos.com/creator/3.8/manual/en/)
  - 3D physics の backend とサイズは Bullet 約 1.5 MB(既定)、cannon.js 約 141 KB、PhysX 4.1 約 5 MB、builtin は collision のみで最小 — [physics engine](https://docs.cocos.com/creator/3.8/manual/en/physics/physics-engine.html)
  - 3.8.6 で package size の最適化と feature trimming を導入。ただし数値は公表なし — [Cocos](https://www.cocos.com/en/post/f539c7888e620701228458d6b89b80c7)
- **SpriteKit**
  - Apple の metadata に deprecation の記載なし — [Apple doc JSON](https://developer.apple.com/tutorials/data/documentation/spritekit.json)
  - `SpriteView` で SwiftUI に埋め込める(iOS 14+)— [SpriteView JSON](https://developer.apple.com/tutorials/data/documentation/spritekit/spriteview.json)
  - 大きな更新が長くないという評価は二次情報 — [Fatbobman](https://fatbobman.com/en/weekly/issue-090/)
- **SceneKit**
  - 全 platform で OS 26 から deprecated。"SceneKit is deprecated, use RealityKit instead" — [Apple doc JSON](https://developer.apple.com/tutorials/data/documentation/scenekit.json)
  - soft deprecation で既存アプリは動作し、critical bug のみ修正。新機能や最適化はなく、hard deprecation の予定もなし — [WWDC25 #288](https://developer.apple.com/videos/play/wwdc2025/288/)
- **RealityKit**
  - ECS の 3D エンジン。SwiftUI の `RealityView` で統合。visionOS / iOS / iPadOS / macOS / tvOS(新規)に対応 — [WWDC25 #288](https://developer.apple.com/videos/play/wwdc2025/288/)
  - tvOS は 26.0 から。platform 情報 — [Apple doc JSON](https://developer.apple.com/tutorials/data/documentation/realitykit.json)
- **raylib**
  - 「no fancy interface, no visual helpers, no gui tools or editors」のコードのみのライブラリ。C99、OpenGL 1.1〜4.3 / ES 2.0、外部依存なし — [raylib.com](https://www.raylib.com/)
  - 同ページの platform 一覧に iOS はない — 同上
- **Solar2D**
  - API は 1000 超。Solar2D Native でネイティブ拡張。CoronaCards で既存ネイティブアプリに埋め込み — [docs](https://docs.coronalabs.com/)、[CoronaCards](https://docs.coronalabs.com/coronacards/)
- **KorGE**
  - Kotlin Multiplatform のエンジン。Views、Scenes、Audio、Physics、Atlas、Hot Reloading などの文書がある。ただし文書自体に未レビューで古い可能性があるとの注記 — [docs](https://docs.korge.org/)
- **react-native-game-engine**
  - `GameLoop` は 16ms ごとの callback。`GameEngine` は systems を entities に適用するだけの層 — [GitHub](https://github.com/bberak/react-native-game-engine)
  - 物理は "does not come with"、描画は "up to you" — 同上
- **react-native-skia**
  - 自己定義は "High-performance 2d Graphics for React Native using Skia" で、Skia を RN に持ち込むグラフィックスライブラリ。README の冒頭に game loop や物理の記載はない — [README](https://raw.githubusercontent.com/Shopify/react-native-skia/main/README.md)
  - v3 は Graphite(Vulkan / Metal)で multi-threaded、v2 は Ganesh(OpenGL / Metal)で single-threaded — 同上
- **three.js**
  - 自己定義は "JavaScript 3D library"。汎用 3D ライブラリを目指し、WebGL と WebGPU の renderer を含む。README の冒頭では engine を名乗っていない — [README](https://raw.githubusercontent.com/mrdoob/three.js/dev/README.md)
- **flutter_scene**
  - PBR、glTF import、skinned mesh、morph target、影、GI、post process、Gaussian splatting、particles を提供 — [pub.dev](https://pub.dev/packages/flutter_scene)
  - 周辺に Rapier / box3d の物理、SoLoud / FMOD の音声、multiplayer、MCP サーバー付きの desktop editor がある(一部は別 package、開発中)— 同上
- **Babylon.js**
  - 物理は Physics V2 の plugin として Havok を組み込む方式 — [docs](https://doc.babylonjs.com/features/featuresDeepDive/physics/)

### Inferences
- 軽量エンジンの多くが共通して **Box2D 系を 2D 物理に、Bullet 系を 3D 物理に**採用しています。物理は「買う」ドメインだと見てよいでしょう。
- Cocos の backend 選択と feature cropping、Defold の Physics 2D: None は、**サイズを抑えるためにモジュールを外せる設計**が軽量エンジンの共通要件であることを示します。
- flutter_scene が急速に機能を広げているため、「Flutter の 3D 下限」は Flame ではなく flutter_scene で測るほうが適切になりつつあります。

### Gaps
- SpriteKit の `SKAction`、`SKPhysicsBody`、particle、tilemap は広く知られていますが、Apple の個別ページを本調査で取得していないため、表では「—」にしました。報告書で使う場合は追加確認が望まれます。
- RealityKit の物理、音声、入力も同様に未確認です。
- three.js の「physics と game loop は同梱しない」という広く知られた認識は、README 冒頭に明記がなく一次確認できていません。
- react-native-skia の docs は URL が 404 で読めず、README 冒頭しか確認していません。UI thread 描画、Atlas、Reanimated 連携は未確認です。
- Babylon.js の audio と GUI は未確認です。
- Cocos、Solar2D、raylib、KorGE の runtime サイズの信頼できる数値は未入手です。
- raylib の iOS 対応は公式サイトの一覧にない、という確認に留まります。

## 「レンダリングライブラリ」と「ゲームエンジン」を分ける最小機能は何か

### Takeaway
公式な定義は見つかりませんでした。以下は事例からの推論です。**時間(game loop)、オブジェクトの lifecycle、入力の振り分け、空間的な問い合わせ(collision)の 4 つをフレームワークが持つ**ことが境界になると考えられます。

### Cited Findings
- raylib は自らを「ライブラリ」と呼び、エディタも GUI ツールもないとしている — [raylib.com](https://www.raylib.com/)
- RN Game Engine は loop と systems のみを提供し、物理も描画も提供しない — [GitHub](https://github.com/bberak/react-native-game-engine)
- Flame の宣言する主要機能は game loop、component / object system、effects と particles、collision detection、gesture と input、images と sprite animation、utilities — [GitHub](https://github.com/flame-engine/flame)
- Flame は物理を bridge に分離しており、物理なしでも engine を名乗っている — [docs: collision](https://docs.flame-engine.org/latest/flame/collision_detection.html)
- flame_3d_component は scene graph、camera、material を持つ flutter_scene を描画するだけの層で、「3D ゲームエンジンではない」と自称する — [pub.dev](https://pub.dev/packages/flame_3d_component)
- 一方で flutter_scene は description 上で engine の周辺機能まで広げている — [pub.dev](https://pub.dev/packages/flutter_scene)
- Babylon.js は物理を plugin として engine の API に統合している — [docs](https://doc.babylonjs.com/features/featuresDeepDive/physics/)

### Inferences
本節はすべて著者の分類であり、出典のある定義ではありません。

| 層 | 含まれるもの | 事例 |
|---|---|---|
| L0 描画ライブラリ | 描画 API、任意で scene graph と asset loader | react-native-skia、three.js(いずれも README の自己定義に基づく) |
| L1 loop フレームワーク | L0 に固定の更新ループと entity 管理を追加。描画や衝突は持たない | RN Game Engine |
| L2 最小エンジン | **update/render loop と dt、object の lifecycle、sprite / animation、衝突判定、入力の object への振り分け、camera、pause と背景化、asset 読み込み、音声への導線** | Flame core + flame_audio |
| L3 統合エンジン | 物理の統合、scene editor、asset pipeline、build と配布 | Defold、Cocos Creator |

- 音声は Flame の core にありませんが、公式 bridge で提供されています。**機能が core にあることではなく、公式に統合された経路があること**を基準にするのが妥当です。
- 物理エンジンは必須条件にしにくいです。Flame core も、Cocos の builtin backend も、collision のみで成立しています。
- UI フレームワークを持つ Whisker の場合、L2 のうち入力、text、画像 decode、音声の一部は既存基盤を流用できる可能性があります。Flame が Flutter に対して行っている委譲と同じ構図です。追加で必要なのは主に **loop、object lifecycle、collision、camera、batch 描画**だと見込まれます。これは仮説で、Whisker の実装とは突き合わせていません。

### Gaps
- 「game engine」の権威ある定義や標準は探しましたが、一次情報としては得られませんでした。上記の層は比較のための作業定義として扱ってください。

## エンジンはネイティブアプリや UI フレームワークにどう組み込まれるか、その制約は何か

### Takeaway
組み込みの難しさは「エンジンが process と window をどれだけ所有しているか」で決まります。host の UI フレームワーク上に乗る Flame、SpriteKit、flutter_scene は普通の view として扱えます。一方 Unity as a Library は、mobile では full-screen のみ、1 process に 1 instance、quit 後は再起動不可です。LibGodot はその中間で、surface の受け渡しが upstream に取り込まれている途中です。Bevy は raw window handle を使えば任意サイズの view で動きますが、確認できた例は community 実装です。

### Cited Findings

**Unity as a Library(Unity 6000.3 docs)**
- 対象は Android、iOS、Windows / UWP。ネイティブ技術の専門家向け — [Unity manual](https://docs.unity.com/en-us/engine/6000.3/manual/platform-specific/cross-platform-features/unityasa-library)
- 複数の runtime instance は不可 — 同上
- Android / iOS は full-screen rendering のみ(Unity Industry 顧客は異なる場合あり)— 同上
- host 配下では Unity は lifecycle を制御しないため、動かないシナリオがありうる — 同上
- `Application.Unload` 後も 80〜180 MB のメモリを保持(解像度に依存)— 同上
- iOS: `unloadApplication` は再実行可能だがメモリを完全には解放しない。`quitApplication:` は全解放するが同一 process で再実行不可 — [Unity iOS](https://docs.unity.com/en-us/engine/6000.3/manual/platform-specific/iphone/ios-developing/unityasa-library-ios)
- iOS: `runEmbeddedWithArgc:` で他の view と併存し、`showUnityWindow` で切り替える。status bar は host 側で制御 — 同上
- Android: quit すると Unity を動かす process が終了する。`Intent` で Unity activity を起動して制御する — [Unity Android](https://docs.unity.com/en-us/engine/6000.3/manual/platform-specific/android/developing/unityasa-library)
- Android: Play Feature Delivery の dynamic module には不可。Xamarin 非対応 — 同上
- 入力の制約は上記 UaaL ページに記載なし — 同上

**Godot / LibGodot**
- 最新の stable は 4.7.2(2026-08-18)— [releases](https://github.com/godotengine/godot/releases)
- LibGodot の初期版は 4.6 の主要機能として出荷 — [GodotCon AMS 2026](https://talks.godotengine.org/godotcon-ams-2026/talk/DQXZZC/)
- API は `GodotInstance` の `start()` / `iteration()` / `shutdown()`。host が loop を駆動する。`DisplayServerEmbedded` が host の surface に描き、host が input を注入する — [PR #90510](https://github.com/godotengine/godot/pull/90510)
- 元の設計では global data と singleton のため **1 process に 1 instance**。同時複数 instance を対応とする記述は確認できず — 同上
- 再初期化は Migeran の 4.6 ブランチでは stable、upstream PR は準備済み(2026-06 時点)— 同上
- 4.6 upstream は headless か自前 window で動作し、render target 埋め込みは未対応とのコメントあり。2026-09 に follow-up PR が多数作成され、一部 merge、多数は open — 同上
- 2026-03 時点で「DisplayServer 初期化前に host が既存 surface を渡す標準的な手段がない」。Android は内部 JNI bridge、iOS は非公開の hook に頼る状況 — [proposal #14435](https://github.com/godotengine/godot-proposals/issues/14435)
- 同 proposal で 2026-07 に、surface の再生成や Flutter のような compositor 向けに buffer を生成する方式が必要だとの指摘あり。状態は open — 同上
- **状況の記述に差があります:** GodotCon の概要は全 platform の UI 埋め込みを「current patches」として挙げています。[PR](https://github.com/godotengine/godot/pull/90510) では Apple が upstream 準備済み、Android / Windows は進行中です。どちらも「Migeran パッチでは可能、upstream は途上」と読めば整合します。
- SwiftGodotKit は 4.6 系ブランチに依存し、ブランチ不一致だと runtime で失敗すると警告 — [SwiftGodotKit(ミラー、二次)](https://gh.r-mj.com/migueldeicaza/SwiftGodotKit)

**Bevy**
- 最新は v0.20.1(2026-10-28)— [releases](https://github.com/bevyengine/bevy/releases)
- community の `bevy-in-app` は `WinitPlugin` を使わない。winit がアプリ初期化と windowing を全部奪うため。既存アプリ内に `App` を作り、UIView / SurfaceView で**任意サイズ**で動かす — [GitHub](https://github.com/jinleili/bevy-in-app)
- 対応表の最新は Bevy 0.18 → 0.6、最終 push は 2026-02-01 で **現行より 2 minor 遅れ** — 同上、[API](https://api.github.com/repos/jinleili/bevy-in-app)
- 手法は `UIView` / `ANativeWindow` を raw-window-handle の trait で包み、window entity に渡す — [作者の解説](https://jinleili.github.io/learn-wgpu-zh/integration-and-debugging/bevy)

**Flutter(Flame、Flutter GPU、flutter_scene)**
- Flame の `GameWidget` は普通の widget — [docs: overlays](https://docs.flame-engine.org/latest/flame/overlays.html)
- Flutter GPU は RGBA texture に描画し、shader-readable にして `dart:ui.Image` 化し、`Canvas.drawImage` で widget に描く手順が紹介されている(early preview と明記、Flutter 3.24 期の記事)— [flutter.cn](https://docs.flutter.cn/posts/flutter-gpu-101)
- 新しい engine commit で `GpuSurface` API が追加され、texture pool を管理して `drawImageRect` で提示する。stable に入っているかは未確認 — [engine commit](https://flutter.googlesource.com/mirrors/flutter/+/cc6d73260e1f866604c8845fc09a73d2a05fa983)
- flutter_scene は web 以外で Flutter GPU の有効化が必須(flag か platform ごとの設定)。Windows / Linux の release は Flutter 3.47.1 が必要 — [pub.dev](https://pub.dev/packages/flutter_scene)
- flutter_scene の web は WebGL2 backend で flag 不要 — 同上
- Flutter GPU は flame_3d の文書では experimental 扱い — [pub.dev: flame_3d](https://pub.dev/packages/flame_3d)

**その他**
- SpriteKit は SwiftUI の `SpriteView`、RealityKit は `RealityView` で view として組み込める — [SpriteView](https://developer.apple.com/tutorials/data/documentation/spritekit/spriteview.json)、[WWDC25 #288](https://developer.apple.com/videos/play/wwdc2025/288/)
- Solar2D の CoronaCards は native library を呼べる任意のフレームワークに組み込める — [docs](https://docs.coronalabs.com/coronacards/)

### Inferences
上記の所見を整理した比較表です。Unity と LibGodot の制約は出典付きの所見に基づきますが、Flame、flutter_scene、SpriteKit / RealityKit、Bevy の「lifecycle」「loop の所有者」の列は著者の解釈を含みます。

| 方式 | 描画先 | 複数 instance | 部分表示 | 再起動 | loop の所有者 |
|---|---|---|---|---|---|
| Flame | Flutter Canvas | 未確認 | 可 | widget の lifecycle | Flutter の tick |
| flutter_scene | GPU texture → Flutter に合成(Flutter GPU 一般の手順からの類推) | 未確認 | 可 | 未確認 | Flutter 側 |
| SpriteKit / RealityKit | OS の view | 未確認 | 可 | view の lifecycle | OS framework |
| Unity as a Library | 独自 | **不可** | **mobile は不可** | iOS は quit 後不可、unload は可 | Unity |
| LibGodot | host の surface(途上) | 元の設計では不可 | パッチ次第 | パッチで可 | **host** (`iteration()`) |
| Bevy(bevy-in-app) | raw handle の surface | 未確認 | 可 | 未確認 | **host**(推論) |

UI フレームワーク側の設計に関わる論点は次のとおりです。

1. **描画の合成方式が 2 つある。** 「engine が surface を占有し、UI はその上に native view を重ねる」方式と、「engine が texture に描いて UI 側の compositor に渡す」方式です。Flame + overlay は Flutter の同一 compositor 内で完結するので、z-order や透過の問題が起きません。独立 surface(Unity、Godot、Bevy)では z-order、透過、同期のずれが発生しやすくなります。proposal #14435 のコメントもこの点を指摘しています。
2. **loop の所有権が移植性を決める。** LibGodot の `iteration()` のように host が 1 frame ずつ駆動できる形だと、UI の vsync と揃えられます。winit が初期化を奪う問題は、Bevy を Rust で埋め込む場合の中心的な論点です。`bevy-in-app` が Winit を外しているので、loop は host が駆動していると考えられますが、README の範囲では詳細を確認できていません。
3. **global state が単一 instance 制約を生む。** Unity も Godot も同じ制約です。Rust で engine を設計するなら、最初から instance を値として持てば避けられる可能性があります。
4. **lifecycle は最初に定義すべき。** 確認できた制約の多くが「停止と解放」に集中しています(Unity のメモリ残留と再起動不可、Godot の reinitialization、Flame の backgrounding)。surface の破棄と再生成(Android の SurfaceView)も関わります。
5. **入力の routing は文書化が薄い。** 確認できた方式は、Flame の component mixin(tree の外は Flutter の gesture)と、LibGodot の host からの注入です。Unity の UaaL 文書には記載がありません。「game の view と UI の gesture が競合したときの調停」はどの engine の文書からも答えを得られなかったので、Whisker 側の設計課題になりそうです。
6. **Whisker にとって最も近いのは Flame 型の統合です。** 一方で Whisker の描画基盤に Flutter GPU に相当する口があるかは未確認であり、なければ独立 surface(wgpu + native view)の Bevy 型が現実的な候補です。これは本調査の範囲外の検証を要します。

### Gaps
- Android で UaaL を別 process に分ける運用は広く語られていますが、本調査で読んだ公式ページには記載がありませんでした。
- Unity Industry での部分表示の詳細は未確認です。
- Bevy の公式な埋め込み文書や、公式の iOS / Android lifecycle 対応(surface 再生成)の現状は確認していません。Bevy 本体の docs 確認が必要です。
- `bevy-in-app` の入力と lifecycle の扱いは README に記載がなく、ソース未読です。
- LibGodot の個々の follow-up PR(#124005、#124006 など)の merge 状態は確定できていません。
- Godot 4.7 の LibGodot の状況を明記した一次資料は未入手です。
- Flutter GPU の 2026-10 時点の公式ステータスを述べた Flutter 公式ページは見つかりませんでした。pub.dev の依存 package の記述から推定しています。
- flutter_scene の widget への統合方法は README を取得できず未確認で、表の「texture → 合成」は Flutter GPU 一般の手順からの類推です。
- Flame で複数の `GameWidget` を同時に置く場合の制約は確認していません。`SingleGameInstance` mixin の存在は単一 game 向けの最適化を示すだけです。
- Defold、Cocos Creator をネイティブアプリに埋め込む公式手段は確認していません。
