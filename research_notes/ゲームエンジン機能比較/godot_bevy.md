# Godot 4 / Bevy / Fyrox 機能・アーキテクチャ調査(調査日: 2026-10-31)

調査方法の注記: 一次情報(公式ドキュメント、リリース記事、GitHub の manifest)を WebFetch で取得した。WebFetch は要約モデルを介するため、数値や crate 名は原文の再確認が望ましい。「ページに記載なし」は、そのページに書かれていなかったという意味であり、機能が存在しないことの証明ではない。

## 各エンジンはドメインごとに何を提供し、何が組み込みで、何が外部依存か

### Takeaway
Godot は editor 込みで全ドメインを組み込みで提供する統合型である。Bevy は ECS・rendering・UI・asset・animation・audio までが本体で、physics・navigation・networking・scripting・particles は本体の crate 一覧になく第三者 crate に委ねる。Fyrox は Godot に近い統合型(editor 付き、scene graph)を Rust のみで実現し、physics は Rapier に依存する。

### Cited Findings

#### 対応表(◎=本体組み込み、△=部分的または実験的、外=第三者)

| ドメイン | Godot 4.7 | Bevy 0.20 | Fyrox 1.0 |
|---|---|---|---|
| Rendering | ◎ 3 renderer | ◎ wgpu | ◎ deferred PBR |
| Skeletal / blend | ◎ AnimationTree | ◎ AnimationGraph | ◎ ABSM |
| IK | ◎ 4.6 で刷新 | crate docs に記載なし | 未確認 |
| Physics | ◎ Jolt(3D 既定)+ Godot Physics | 外 Avian / Rapier | Rapier を統合 |
| Audio | ◎ bus + effects | ◎ 基本(rodio)、高度なものは外 | ◎ HRTF、処理 graph |
| Particles | ◎ GPU / CPU | 外 hanabi / enoki | ◎ node あり |
| UI | ◎ Control | ◎ bevy_ui + feathers(発展途上) | ◎ 30+ widget |
| Scripting | ◎ GDScript、C#、GDExtension | 外 | Rust のみ |
| Navigation / AI | ◎ navmesh、A* | 外 | ◎ A*、navmesh、BT |
| Networking | ◎ ENet、RPC、WebRTC | 外 | 未確認 |
| Editor | ◎ | 公式なし | ◎ |

#### Rendering
用語: PBR は物理ベースの material モデル。GI(global illumination)は間接光の計算。render graph は描画 pass の依存関係を管理する仕組み。
その他の用語(一般的な説明で、出典付きの調査結果ではない):
- 描画方式: forward は object ごとに直接 lighting を計算する。deferred は先に表面情報を buffer に書き、後で lighting する。clustered は画面空間を区切って light を割り当て、多数の light を扱う。
- GI の手法: VoxelGI は voxel 化した scene で、SDFGI は距離場(SDF)で、SSIL は画面上の情報で間接光を近似する。lightmap は事前計算した光を texture に焼く方式。
- 反射と影: SSR は画面内の情報で作る反射。SSAO は隅の陰りの近似。PSSM は距離で分割した shadow map。PCSS は距離に応じて輪郭をぼかす影。contact shadow は接地部の短距離の影。
- AA と upscale: TAA は過去 frame を使う antialiasing。FSR2 と DLSS は低解像度描画からの upscale。
- 形状の詳細度: LOD は遠方を低詳細 mesh に替える手法で、HLOD は複数 object をまとめて替える手法。occlusion culling は隠れた物体の描画を省く手法。virtual geometry / meshlet は mesh を小片に分けて必要な詳細度だけ描く手法(Nanite 型)。
- その他: mesh shader は vertex 処理を置き換える新しい GPU pipeline。navmesh は歩行可能領域を表す mesh で、pathfinding に使う。

**Godot**
- renderer は 3 種類。Forward+(desktop 向け最高機能)と Mobile は RenderingDevice 経由で Vulkan / Direct3D 12 / Metal を使い、Compatibility は OpenGL で web の既定 — [Renderers](https://docs.godotengine.org/en/stable/tutorials/rendering/renderers.html)
- light 上限は Forward+ が cluster あたり 512、Mobile が mesh あたり 8・view あたり 256、Compatibility が mesh あたり 8 — [Renderers](https://docs.godotengine.org/en/stable/tutorials/rendering/renderers.html)
- VoxelGI、SDFGI、SSIL、volumetric fog、SSR、SSS、TAA、FSR2 は Forward+ 限定。compute shader は Compatibility で使えない — [Renderers](https://docs.godotengine.org/en/stable/tutorials/rendering/renderers.html)
- GI は baked lightmap、VoxelGI、SDFGI、SSIL。material は Disney PBR。shadow は PSSM、PCSS 風 blur。post-process は tonemap、auto exposure、DoF、SSAO、glow、LUT。AA は TAA、FSR2、MSAA、FXAA、SSAA — [Feature list 4.7](https://docs.godotengine.org/en/stable/about/list_of_features.html)
- shader は GLSL 風のテキスト言語と visual shader editor の 2 系統 — [Feature list 4.7](https://docs.godotengine.org/en/stable/about/list_of_features.html)
- render pipeline への介入手段として CompositorEffects が Mobile と Forward+ にある — [Renderers](https://docs.godotengine.org/en/stable/tutorials/rendering/renderers.html)
- 4.7 で HDR output(Windows、macOS、iOS、visionOS、Linux Wayland)、AreaLight3D が追加された — [Godot 4.7](https://godotengine.org/releases/4.7/)
- 4.6 で SSR 刷新、octahedral probe、Windows の新規 project 既定が D3D12 に変更された — [Godot 4.6](https://godotengine.org/releases/4.6/)
- 4.8 beta で contact shadow、multi-bounce AO、animated decal、mip streaming が入る(未 stable) — [4.8 beta 1](https://godotengine.org/article/dev-snapshot-godot-4-8-beta-1/)

**Bevy**
- `bevy_render` 0.20.0 は wgpu 30 に依存し、dx12、metal、vulkan を既定で有効化する。webgl、webgpu、gles は feature — [bevy_render Cargo.toml v0.20.0](https://raw.githubusercontent.com/bevyengine/bevy/v0.20.0/crates/bevy_render/Cargo.toml)
- 0.19 で `RenderGraph` が廃止され、`Core3d`、`Core2d` などの ECS schedule に置き換わった。pass は普通の system で `.after(...)` で順序付ける — [Bevy 0.19](https://bevy.org/news/bevy-0-19/)
- 0.19 の追加: GPU light clustering、contact shadow、物理ベース SSR、矩形 area light(影なし)、vignette、lens distortion、device loss 回復の `RenderErrorHandler`。`many_cubes` は 49.47 ms から 18.77 ms に改善 — [Bevy 0.19](https://bevy.org/news/bevy-0-19/)
- 0.20 で shader が WESL(WGSL の標準拡張)に移行し、独自 WGSL 方言を削除。mesh shader は初期基盤のみで web 非対応。sprite renderer が 3D 基盤を再利用するよう統合された — [Bevy 0.20](https://bevy.org/news/bevy-0-20/)
- Solari(raytraced lighting)は experimental 扱いの feature。meshlet(virtual geometry)と PCSS も experimental — [cargo_features.md](https://raw.githubusercontent.com/bevyengine/bevy/main/docs/cargo_features.md)
- Solari は Metal でも動くが built-in denoiser がない。DLSS-RR 4.5 は `dlss_wgpu` 経由 — [Bevy 0.20](https://bevy.org/news/bevy-0-20/)
- GPU-driven rendering の完全対応は Windows / Linux の Vulkan のみで、他 backend は部分対応または非対応、WebGL2 は非対応(0.16 時点、古い情報) — [Bevy 0.16](https://bevy.org/news/bevy-0-16/)
- 0.18 の atmosphere は scene lighting を shading し、`ScatteringMedium` で custom 可能 — [Bevy 0.18](https://bevy.org/news/bevy-0-18/)

**Fyrox**
- metallic workflow の PBR、HDR、tone mapping、deferred shading(透過は forward)、shadow、SSAO、FXAA — [Fyrox book](https://fyrox-book.github.io/introduction/introduction.html)
- repo に `fyrox-graphics-gl` と `fyrox-graphics-wgpu` の両 directory がある — [Fyrox GitHub](https://github.com/FyroxEngine/Fyrox)

#### Animation
用語: skeletal は bone 階層で mesh を変形する方式。blend graph は複数 clip の混合を graph で表すもの。IK は末端の目標位置から関節角を逆算する手法。root motion は animation 内の移動量でキャラクターを動かす手法。

- **Godot**: AnimationTree が blend tree(Blend2/3、OneShot、TimeScale、Transition)、BlendSpace1D/2D、state machine(`travel()` は A* で経路探索)、root motion を提供する — [AnimationTree](https://docs.godotengine.org/en/stable/tutorials/animation/animation_tree.html)
- **Godot IK**: 4.6 で IKModifier3D 系に刷新。TwoBoneIK3D、SplineIK3D、FABRIK3D、CCDIK3D、JacobianIK3D と関節制約がある — [Godot 4.6](https://godotengine.org/releases/4.6/)
- **Godot 2D**: AnimatedSprite2D — [Feature list](https://docs.godotengine.org/en/stable/about/list_of_features.html)。4.7 で `Tween.tween_await()` 追加 — [Godot 4.7](https://godotengine.org/releases/4.7/)
- **Bevy**: `bevy_animation` は AnimationClip、AnimationPlayer、animation graph(blend)、transition、event、glTF curve を持つ。docs top に state machine と IK の記載はない — [docs.rs 0.20.1](https://docs.rs/bevy_animation/latest/bevy_animation/)
- **Bevy の例**: Animated Mesh、Animation Graph、Masks、Morph Targets、Easing、Events — [examples README](https://raw.githubusercontent.com/bevyengine/bevy/main/examples/README.md)
- **Bevy の第三者**: bevy_tween、bevy_tweening、seldom_state、bevy_spine、bevy_spritesheet_animation — [Bevy Assets](https://bevy.org/assets/)
- **Fyrox**: state machine、retargeting、blend space、root motion — [Fyrox book](https://fyrox-book.github.io/introduction/introduction.html)。ABSM は Unity Mecanim に近い概念という二次情報あり — [i-programmer](https://www.i-programmer.info/news/144-graphics-and-games/19015-fyrox-game-engine-reaches-version-100.html)

#### Physics & collision
- **Godot**: 新規 3D project の既定は 4.6 から Jolt。既存 project は設定維持 — [Godot 4.6](https://godotengine.org/releases/4.6/)
- **Godot Jolt の制約**: 組み込み module 化済みで、旧 extension は maintenance mode。joint の soft limit など一部 property 非対応、contact impulse は推定値、別 thread 実行は experimental — [Using Jolt](https://docs.godotengine.org/en/stable/tutorials/physics/using_jolt_physics.html)
- **Godot の機能**: 2D / 3D ともに static、rigid、character body、joint。3D は vehicle(arcade 系)、soft body、ragdoll — [Feature list](https://docs.godotengine.org/en/stable/about/list_of_features.html)。4.8 beta は Jolt 5.6.0 と非 allocating な query — [4.8 beta 1](https://godotengine.org/article/dev-snapshot-godot-4-8-beta-1/)
- **Bevy**: 65 crate の一覧に physics crate がない — [crates/](https://github.com/bevyengine/bevy/tree/main/crates)。第三者として avian、bevy_rapier、bevy-tnua(character controller)が掲載 — [Bevy Assets](https://bevy.org/assets/)
- **Avian**: ECS native で「別の physics world を持たない」設計。2D / 3D、CCD、sleeping、joint、spatial query、interpolation、f32 / f64。collision は Parry — [Avian](https://github.com/avianphysics/avian)
- **Avian 版対応**: Bevy 0.20 は Avian 0.8。Bevy の release ごとに対応 release が必要 — [Avian](https://github.com/avianphysics/avian)
- **Rapier**: Dimforge 製の独立 engine。rigid body、joint、soft body、snapshot、任意の cross-platform determinism、JS / C / Python binding。guide 上は Rust 0.36、Bevy plugin 0.37 — [Rapier docs](https://rapier.rs/docs/)
- **Fyrox**: Rapier 0.32 に移行(1.0 時点) — [Fyrox 1.0](https://fyrox.rs/blog/post/fyrox-game-engine-1-0-0/)

#### Audio
- **Godot**: 5.1 / 7.1、2D / 3D positional、Doppler、再ルート可能な bus と effect、procedural audio、mic 入力、TTS、MIDI 入力 — [Feature list](https://docs.godotengine.org/en/stable/about/list_of_features.html)
- **Bevy**: `bevy_audio` は rodio 0.23 に依存 — [Cargo.toml](https://raw.githubusercontent.com/bevyengine/bevy/main/crates/bevy_audio/Cargo.toml)。例は再生、制御、soundtrack、spatial 2D / 3D、custom decodable — [examples README](https://raw.githubusercontent.com/bevyengine/bevy/main/examples/README.md)
- **Bevy の第三者**: bevy_kira_audio、bevy_fundsp、bevy_oddio、bevy_rrise — [Bevy Assets](https://bevy.org/assets/)。0.18、0.19、0.20 の release 記事に audio の section はない — [0.18](https://bevy.org/news/bevy-0-18/)、[0.19](https://bevy.org/news/bevy-0-19/)、[0.20](https://bevy.org/news/bevy-0-20/)
- **Fyrox**: HRTF binaural、reverb、処理 graph — [Fyrox book](https://fyrox-book.github.io/introduction/introduction.html)

#### Input / Camera
- **Godot input**: 再割当可能な input action、deadzone、物理 key、gamepad 最大 8、pen 圧。4.7 で VirtualJoystick node と gyro — [Feature list](https://docs.godotengine.org/en/stable/about/list_of_features.html)、[Godot 4.7](https://godotengine.org/releases/4.7/)
- **Bevy input**: `bevy_input`、`bevy_gilrs`、`bevy_input_focus`、`bevy_picking` が本体 — [crates/](https://github.com/bevyengine/bevy/tree/main/crates)。action mapping の第三者として leafwing_input_manager、bevy_enhanced_input — [Bevy Assets](https://bevy.org/assets/)
- **Bevy UI 操作**: `AutoDirectionalNavigation` で gamepad / keyboard の空間的な focus 移動 — [Bevy 0.18](https://bevy.org/news/bevy-0-18/)
- **Godot camera**: perspective、orthographic、frustum — [Feature list](https://docs.godotengine.org/en/stable/about/list_of_features.html)
- **Bevy camera**: `bevy_camera` と `bevy_camera_controller`。FreeCamera と PanCamera は 0.18 — [Bevy 0.18](https://bevy.org/news/bevy-0-18/)。PanOrbitCamera は 0.20 で `bevy_editor_cam` から upstream — [Bevy 0.20](https://bevy.org/news/bevy-0-20/)

#### Particles / VFX
- **Godot**: GPU particle(subemitter、trail、attractor、collision)と CPU particle — [Feature list](https://docs.godotengine.org/en/stable/about/list_of_features.html)。4.8 beta で Trail3D — [4.8 beta 1](https://godotengine.org/article/dev-snapshot-godot-4-8-beta-1/)
- **Bevy**: particle crate は本体にない — [crates/](https://github.com/bevyengine/bevy/tree/main/crates)。bevy_hanabi(3D)、bevy_enoki(2D)が掲載 — [Bevy Assets](https://bevy.org/assets/)
- **Fyrox**: scene node に particle system — [Fyrox book](https://fyrox-book.github.io/introduction/introduction.html)

#### UI
- **Godot**: Control node 群(button、LineEdit、TextEdit、CodeEdit、Tree、RichTextLabel、color picker)、anchor と container の layout、theme editor、StyleBox — [Feature list](https://docs.godotengine.org/en/stable/about/list_of_features.html)
- **Godot i18n / a11y**: bidi text、RTL 自動 mirror、screen reader 対応 — [Feature list](https://docs.godotengine.org/en/stable/about/list_of_features.html)
- **Bevy layout**: `bevy_ui` は taffy 0.15(block、flexbox、grid)と accesskit 0.25 に依存 — [bevy_ui Cargo.toml](https://raw.githubusercontent.com/bevyengine/bevy/main/crates/bevy_ui/Cargo.toml)
- **Bevy 3 層構成**: `bevy_ui`(layout / node)、`bevy_ui_widgets`(headless)、`bevy_feathers`(editor 向け styled)。描画は `bevy_ui_render` に分離 — [crates/](https://github.com/bevyengine/bevy/tree/main/crates)、[Bevy 0.20](https://bevy.org/news/bevy-0-20/)
- **Bevy text**: 0.19 で cosmic_text から parley に移行。`EditableText` は selection、clipboard、IME、multiline に対応するが、placeholder、undo/redo、password mask は未対応(0.19 時点) — [Bevy 0.19](https://bevy.org/news/bevy-0-19/)
- **Bevy 0.20 の追加**: Color Input、List View、Dropdown、Lazy Menu、headless Tab、`Val::Em` / `Rem`、`FixedNode`、`InlineBox`、contextual theming — [Bevy 0.20](https://bevy.org/news/bevy-0-20/)
- **Bevy reactivity**: 「収束した reactivity」は未解決で、来年の優先項目に reactivity ecosystem が入る — [Sixth Birthday](https://bevy.org/news/bevys-sixth-birthday/)
- **Fyrox**: node ベース、30+ widget、message passing — [Fyrox book](https://fyrox-book.github.io/introduction/introduction.html)

#### Scene / entity model
用語: ECS は entity(ID)に component(data)を付け、system(関数)が query で一括処理する data 指向の設計。

- **Godot の層構成**: Scene(SceneTree、Node)→ Servers(startup 時に初期化される singleton で rendering、audio、physics などを実装)→ Drivers / Platform。Core と Main が横断する — [Architecture](https://docs.godotengine.org/en/stable/engine_details/architecture/godot_architecture_diagram.html)
- **Godot の連携**: object-oriented 設計、signals と groups — [Feature list](https://docs.godotengine.org/en/stable/about/list_of_features.html)。4.6 で rename や移動に強い Unique Node ID — [Godot 4.6](https://godotengine.org/releases/4.6/)
- **Bevy ECS 基礎**: World、Entity、Component、Resource、System、Schedule。system の引数型から並列実行の可否を判定する。storage は table(既定、iteration が速い)と sparse set(追加削除が速い)。change detection、message、observer — [bevy_ecs README](https://raw.githubusercontent.com/bevyengine/bevy/main/crates/bevy_ecs/README.md)
- **Bevy ECS の進化**: relationships で親子も `ChildOf` / `Children` に統一(0.16) — [Bevy 0.16](https://bevy.org/news/bevy-0-16/)。resource が singleton entity 上に格納され hook や observer が適用可能に(0.19) — [Bevy 0.19](https://bevy.org/news/bevy-0-19/)
- **Bevy 0.20 の ECS**: `chain_weak`、per-column change tick、panic の error 化(既定は re-panic)、`Ready` event、schedule randomization — [Bevy 0.20](https://bevy.org/news/bevy-0-20/)
- **BSN**: `bsn!` macro による scene。合成可能な patch、relationship、parameter、observer を記述。`.bsn` file loader は未提供 — [Bevy 0.19](https://bevy.org/news/bevy-0-19/)、[Bevy 0.20](https://bevy.org/news/bevy-0-20/)
- **Fyrox**: 「Classic object-oriented design with an ability to be extended with ECS」、複数 scene、scene graph — [Fyrox book](https://fyrox-book.github.io/introduction/introduction.html)

#### Scripting
- **Godot**: GDScript、C#(.NET 8+、Android / iOS は 4.2 から experimental、web 非対応)、GDExtension(公式 C / C++、community の Rust ほか) — [Feature list](https://docs.godotengine.org/en/stable/about/list_of_features.html)。4.8 は .NET 10 以上 — [4.8 beta 1](https://godotengine.org/article/dev-snapshot-godot-4-8-beta-1/)
- **Godot と Rust**: gdext は「usable state」。Wasm、Android、iOS は experimental — [gdext](https://github.com/godot-rust/gdext)。4.6 で GDExtension interface が JSON 定義になり、required parameter を指定可能 — [Godot 4.6](https://godotengine.org/releases/4.6/)
- **Bevy**: 本体はなく、bevy_mod_scripting、bevy_scriptum、bevy_wasm が掲載 — [Bevy Assets](https://bevy.org/assets/)
- **Bevy hot patch**: `subsecond` と Dioxus `dx` で system を実行中に差し替え。binary crate のみ、Wasm 不可、system parameter 変更は反映不可(0.17 時点) — [Bevy 0.17](https://bevy.org/news/bevy-0-17/)
- **Fyrox**: game は Rust で記述、native code hot-reload — [Fyrox book](https://fyrox-book.github.io/introduction/introduction.html)

#### AI / Navigation
- **Godot**: A*、2D / 3D navmesh、dynamic obstacle avoidance、editor と runtime で生成可能 — [Feature list](https://docs.godotengine.org/en/stable/about/list_of_features.html)
- **Bevy**: 本体になし。vleue_navigator、bevy_rerecast、oxidized_navigation、big-brain、bevior_tree — [Bevy Assets](https://bevy.org/assets/)
- **Fyrox**: A*、navmesh、behavior tree — [Fyrox book](https://fyrox-book.github.io/introduction/introduction.html)

#### Networking
- **Godot**: TCP、UDP、HTTP、ENet ベースの Multiplayer API と RPC、WebSocket、WebRTC、UPnP — [Feature list](https://docs.godotengine.org/en/stable/about/list_of_features.html)
- **Bevy**: 本体になし。lightyear、bevy_replicon、bevy_quinnet、aeronet、bevy_ggrs、naia、bevy_matchbox — [Bevy Assets](https://bevy.org/assets/)

#### Asset pipeline
- **Godot**: 取り込み時に変換する import 方式で custom import plugin 可。3D は glTF 2.0(推奨)、.blend、FBX、Collada、OBJ。export 後の runtime でも glTF を load 可能 — [Feature list](https://docs.godotengine.org/en/stable/about/list_of_features.html)。4.6 で patch PCK の delta encoding — [Godot 4.6](https://godotengine.org/releases/4.6/)
- **Bevy format**: 画像は png、jpeg、webp、ktx2、dds、exr など、音声は mp3、flac、vorbis、wav など feature 単位で選択 — [cargo_features.md](https://raw.githubusercontent.com/bevyengine/bevy/main/docs/cargo_features.md)
- **Bevy glTF**: 0.18 の `GltfExtensionHandler` で読込時に処理を差し込める — [Bevy 0.18](https://bevy.org/news/bevy-0-18/)
- **Bevy の処理と保存**: 0.20 の `CompressedImageSaver` は `ctt` で BCn / ASTC を出力し mip も生成。Basis Universal は別 feature — [Bevy 0.20](https://bevy.org/news/bevy-0-20/)。0.19 で asset 保存 `save_using_saver` と handle の serialize — [Bevy 0.19](https://bevy.org/news/bevy-0-19/)
- **Bevy の計画**: Assets as Entities は未出荷で次期計画 — [Bevy 0.20](https://bevy.org/news/bevy-0-20/)
- **Fyrox**: async loading。PNG、JPG、FBX、GLTF、WAV、OGG、MP3 など — [Fyrox book](https://fyrox-book.github.io/introduction/introduction.html)

#### Streaming / LOD
- **Godot**: meshoptimizer による import 時の自動 mesh LOD、手動の visibility range(HLOD)、occlusion culling — [Mesh LOD](https://docs.godotengine.org/en/stable/tutorials/3d/mesh_lod.html)。mip streaming は 4.8 beta — [4.8 beta 1](https://godotengine.org/article/dev-snapshot-godot-4-8-beta-1/)
- **Bevy**: Nanite 風 virtual geometry は `meshlet` feature — [Bevy 0.16](https://bevy.org/news/bevy-0-16/)。「Visibility range」の例はあるが LOD 専用 section はない — [examples README](https://raw.githubusercontent.com/bevyengine/bevy/main/examples/README.md)

#### Save / serialization
- **Godot**: 公式の推奨は group で対象 node を集めて JSON または `store_var` / `get_var` で保存、設定は ConfigFile — [Saving games](https://docs.godotengine.org/en/stable/tutorials/io/saving_games.html)
- **Bevy**: `bevy_reflect` の例に Dynamic Types、Function Reflection、Serialization。Scene には World Serialization の例 — [examples README](https://raw.githubusercontent.com/bevyengine/bevy/main/examples/README.md)。reflect の自動登録は 0.17 — [Bevy 0.17](https://bevy.org/news/bevy-0-17/)。`SettingsPlugin` は 0.19 — [Bevy 0.19](https://bevy.org/news/bevy-0-19/)
- **Fyrox**: 多くの engine entity が serialize 可能 — [Fyrox book](https://fyrox-book.github.io/introduction/introduction.html)

#### Editor / tooling
- **Godot**: scene tree、script editor、debugger、visual profiler、live edit、remote inspector、game embedding、Tracy / Perfetto — [Feature list](https://docs.godotengine.org/en/stable/about/list_of_features.html)。ObjectDB snapshot — [Godot 4.6](https://godotengine.org/releases/4.6/)。Asset Store — [Godot 4.7](https://godotengine.org/releases/4.7/)
- **Bevy editor**: 「We did not land a baseline editor platform upstream」。community の Jackdaw が BSN scene editor の prototype。Cart は upstream Editor MVP を最優先と位置付ける — [Sixth Birthday](https://bevy.org/news/bevys-sixth-birthday/)
- **Bevy tooling**: BRP は JSON-RPC 2.0 で ECS を遠隔操作(`RemoteHttpPlugin`) — [bevy_remote](https://docs.rs/bevy_remote/latest/bevy_remote/)。transform gizmo と diagnostics overlay は 0.19 — [Bevy 0.19](https://bevy.org/news/bevy-0-19/)。第三者に bevy-inspector-egui、skein — [Bevy Assets](https://bevy.org/assets/)
- **Fyrox**: editor、project manager、animation editor、`export-cli` — [Fyrox 1.0](https://fyrox.rs/blog/post/fyrox-game-engine-1-0-0/)

#### Platforms / mobile
- **Godot**: editor と export は Windows、macOS、Linux、Android(editor は experimental)、web。iOS は export のみ — [Feature list](https://docs.godotengine.org/en/stable/about/list_of_features.html)
- **Godot web の記述**: 同ページは web を experimental とし HTML5 には 3.x を推奨する。editor についての記述か export についてかは要原文確認 — [Feature list](https://docs.godotengine.org/en/stable/about/list_of_features.html)
- **Godot mobile**: 4.7 で GABE stable(端末上で export)、PiP、Java interface の GDScript 実装、VirtualJoystick。iOS HDR と SDL3 controller — [Godot 4.7](https://godotengine.org/releases/4.7/)。4.6 で Mali / Adreno の crash 修正 — [Godot 4.6](https://godotengine.org/releases/4.6/)
- **Godot 埋め込み**: LibGodot は Linux、Windows、macOS(4.6 時点) — [Godot 4.6](https://godotengine.org/releases/4.6/)
- **Bevy Android**: `cargo-ndk`、JDK 21、Gradle。GameActivity 既定(API 23+)、`bevy_audio` は API 26+ — [examples README](https://raw.githubusercontent.com/bevyengine/bevy/main/examples/README.md)
- **Bevy iOS / web**: iOS は `examples/mobile` で `make run`。web は WebGL2 と experimental の WebGPU、`wasm-opt` — [examples README](https://raw.githubusercontent.com/bevyengine/bevy/main/examples/README.md)
- **Bevy mobile の成熟度(古い情報)**: 「Android は iOS ほどではないが十分使える」は Bevy 0.12 時点 — [Cheatbook](https://bevy-cheatbook.github.io/platforms.html)。0.20 記事の mobile 関連は ASTC 出力のみ — [Bevy 0.20](https://bevy.org/news/bevy-0-20/)
- **Fyrox**: book は desktop、Web、Android、iOS — [Fyrox book](https://fyrox-book.github.io/introduction/introduction.html)。1.0 の export 対象記述は PC、Wasm、Android で、iOS の記載なし — [Fyrox 1.0](https://fyrox.rs/blog/post/fyrox-game-engine-1-0-0/)

#### Binary size(公式の絶対値は見つからず、数値は逸話)
- **Godot 公式**: debug symbol 除去で 5〜10 倍減、`disable_3d` で約 15% 減。ほかに LTO、`optimize=size_extra`(4.5+)、`disable_advanced_gui`、`module_*_enabled=no`、`.gdbuild` profile — [Optimizing for size](https://docs.godotengine.org/en/stable/engine_details/development/compiling/optimizing_for_size.html)
- **Godot 逸話**: Android で 4.x 標準約 19 MB、module 削減で約 13 MB — [Forum](https://forum.godotengine.org/t/game-size-too-big-compared-to-apk/39416)。4.5.1 で 244 KB の project の APK が 114 MB という報告もあり幅が大きい — [Forum](https://forum.godotengine.org/t/absurd-file-size-increase-when-my-project-is-exported-to-apk/130000)
- **Bevy 逸話**: `DefaultPlugins` で Windows release 50 MB 超、size 最適化で約 30 MB(2025-01、古い) — [Reddit mirror](https://red.127.pp.ua/r/bevy/comments/1ieacpi/bevy_large_binary_size)。Wasm は最適化後 15 MB 程度(Bevy 0.12、古い) — [Cheatbook](https://bevy-cheatbook.github.io/platforms/wasm/size-opt.html)

### Inferences
- Rust framework が取り込むなら、Bevy の方式(本体を狭くし第三者 crate で埋める)は依存追従の負担を生む。Avian の対応表のように、本体の破壊的変更が release ごとに ecosystem へ波及する。
- Godot の「renderer 3 段」は、mobile を真面目に扱うなら高機能 GI を諦める tier が必要という実例として読める。
- Bevy 0.19 の render graph 廃止は、render pass を app の system と同じ schedule モデルに統合する方向で、ECS を土台にする framework の参考になる。
- Bevy の hot patch は `subsecond` を使っており、同じ `subsecond` を使う Whisker の hot reload と技術基盤が近い(Whisker 側はプロジェクトメモに基づく)。

### Gaps
- Bevy の sprite animation の組み込み範囲、built-in action mapping の有無は、crate 一覧と Assets からの推定に留まる。
- Bevy の IK、state machine の不在は docs top に記載がないという確認のみで、code までは確認していない。
- Avian の中核 solver: README は custom constraint に XPBD と記すのみで、現行の solver は未確認。
- Bevy asset の hot reload、Godot の 2D physics の backend、Godot の ResourceSaver による save、Fyrox の networking は一次情報を未確認。
- Fyrox の wgpu backend の成熟度は directory の存在以外不明。
- 最新版の binary size の測定値はなく、自前 build での計測が必要。
- 実運用での Bevy の iOS / Android 成熟度を示す 2026 年の一次情報は見つからなかった。

## Bevy の repo はどう crate 分割され、何を単体で使えるか

### Takeaway
Bevy は workspace の細かな crate 群を `bevy` という傘 crate が束ねる。必須部分は ECS、app、math、reflect、tasks、input、time、transform などで、rendering 以下はすべて optional である。`bevy_ecs` は公式に standalone 利用が明言されている。

### Cited Findings
- main branch の `crates/` には 65 directory がある — [crates/](https://github.com/bevyengine/bevy/tree/main/crates)
- v0.20.0 の `bevy_internal` で必須なのは: app、derive、diagnostic、ecs、input、math、shape、platform、ptr、reflect、time、transform、utils、tasks(Android は bevy_android も) — [bevy_internal v0.20.0](https://raw.githubusercontent.com/bevyengine/bevy/v0.20.0/crates/bevy_internal/Cargo.toml)
- 同 manifest の optional: a11y、animation、anti_alias、asset、audio、camera、camera_controller、clipboard、color、core_pipeline、curve、dev_tools、extract、feathers、gilrs、gizmos、gizmos_render、gltf、image、input_focus、light、material、mesh、picking、pbr、post_process、remote、render、scene、settings、shader、solari、sprite、sprite_render、state、text、ui、ui_render、ui_widgets、window、winit、world_serialization、log — [bevy_internal v0.20.0](https://raw.githubusercontent.com/bevyengine/bevy/v0.20.0/crates/bevy_internal/Cargo.toml)
- `bevy` は subcrate の container で、個別の `bevy_*` を直接使うこともできる。feature は profile(`2d`、`3d`、`ui`、`default`)、collection(`default_app`、`default_platform`、`common_api`)、個別 flag の 3 層 — [docs.rs bevy 0.20.1](https://docs.rs/bevy/latest/bevy/)
- `bevy_ecs` は「can easily be used as a standalone crate in other projects」 — [bevy_ecs README](https://raw.githubusercontent.com/bevyengine/bevy/main/crates/bevy_ecs/README.md)
- `bevy_ecs` の必須依存は bevy_ptr、bevy_tasks、bevy_utils、bevy_ecs_macros、bevy_platform。bevy_reflect は optional。既定 feature は std、bevy_reflect、async_executor、backtrace で、`multi_threaded` は既定外 — [bevy_ecs Cargo.toml](https://raw.githubusercontent.com/bevyengine/bevy/main/crates/bevy_ecs/Cargo.toml)
- 0.16 で `bevy` 本体が `no_std` に対応(`default-features = false`)。rendering、audio、assets は対象外 — [Bevy 0.16](https://bevy.org/news/bevy-0-16/)
- 0.17 で cameras、lights、shaders、meshes、sprites、text、UI、picking、animation、scenes の user-facing API の多くが `bevy_render` 非依存になり、custom renderer で置換可能に。shader 専用の `bevy_shader` も追加 — [Bevy 0.17](https://bevy.org/news/bevy-0-17/)
- 0.18 の mid-level feature(`2d_api` など)は既定 renderer を省ける — [Bevy 0.18](https://bevy.org/news/bevy-0-18/)
- `bevy_ui` は bevy_camera、bevy_text、bevy_sprite、bevy_input_focus などに依存するが、描画は `bevy_ui_render` に分離されている — [bevy_ui Cargo.toml](https://raw.githubusercontent.com/bevyengine/bevy/main/crates/bevy_ui/Cargo.toml)、[crates/](https://github.com/bevyengine/bevy/tree/main/crates)
- 対照として Godot は crate のような分割ではなく、SCons の flag と build profile で機能を外す。第三者の拡張は GDExtension を使う — [Optimizing for size](https://docs.godotengine.org/en/stable/engine_details/development/compiling/optimizing_for_size.html)、[Feature list](https://docs.godotengine.org/en/stable/about/list_of_features.html)
- Fyrox も fyrox-core、fyrox-graphics、fyrox-ui、fyrox-sound、fyrox-animation、fyrox-resource などに分割されている — [Fyrox GitHub](https://github.com/FyroxEngine/Fyrox)

### Inferences
- `bevy_ecs` 単体の依存は小さく、framework の state 層として採用する候補になる。ただし破壊的変更が約 3 か月ごとに来る前提を負う。
- `bevy_math`、`bevy_color`、`bevy_reflect` は依存の薄さから単体利用しやすそうだが、個別には manifest を確認していない。
- 0.17 の render 分離により、「UI / scene API は Bevy、描画は自前(Whisker なら Lynx など)」という組み合わせは設計上可能になった。`bevy_ui` が `bevy_sprite` や `bevy_text` を引く点は確認が要る。

### Gaps
- main と v0.20.0 で crate 構成に差がある。`bevy_annotations`、`bevy_bsn`、`bevy_inspector` は main の一覧にあるが v0.20.0 の `bevy_internal` の依存にはない。各 crate の役割は未確認。
- main の `bevy_ecs` と `bevy_ui` の manifest が `0.20.0-dev` と表示され、0.20.1 既出と整合しない。取得時の要約またはキャッシュの問題かもしれず未解決。
- 個別 crate の crates.io での単体 API 安定性は公式の保証を確認できていない。

## 現行バージョンと、まだ欠けているもの(2026-10 時点)

### Takeaway
Godot は 4.7.2 が stable で 4.8 は beta 2、Bevy は 0.20.1 で 1.0 前、Fyrox は 1.0.0 である。Bevy の最大の欠落は公式 editor とその前提の `.bsn` format で、Godot の課題はドメイン不足より個別の成熟度(web、C# mobile など)にある。

### Cited Findings
- **Godot**: 4.7 は「Lights, Camera, Action!」(2026-06-18、二次経由で blog 一覧に記載)— [Godot blog 検索結果](https://godotengine.org/blog/index.html)。4.7.2 は 2026-08-18、4.8 beta 2 は 2026-10-29、3.6.3 は 2026-08-22 — [Godot blog](https://godotengine.org/blog/)
- **Godot 4.8**: feature freeze 済み、4.7 以降 1,523 fix、340 contributor — [4.8 beta 1](https://godotengine.org/article/dev-snapshot-godot-4-8-beta-1/)
- **Bevy**: 0.20 は 2026-10-08、0.19 は 06-19、0.18 は 01-13、0.17 は 2025-09-30 — [Bevy news](https://bevy.org/news/)。docs.rs の最新は 0.20.1(10-28) — [docs.rs](https://docs.rs/bevy/latest/bevy/)
- **Bevy の安定性**: 「approximately once every 3 months」の破壊的変更。README 自身が早期開発中、機能と文書の不足を明記 — [README](https://raw.githubusercontent.com/bevyengine/bevy/main/README.md)
- **Bevy の規模**: 0.20 は 817 PR。contributor は本文中で 227 と 226 の両記載 — [Bevy 0.20](https://bevy.org/news/bevy-0-20/)。累計 contributor 1,527、47,529 stars — [Sixth Birthday](https://bevy.org/news/bevys-sixth-birthday/)
- **Bevy の次期計画(日付なし)**: `.bsn` asset format、Assets as Entities、remote inspector、Template 連携の required components、mutually exclusive components、HDR display — [Bevy 0.20](https://bevy.org/news/bevy-0-20/)
- **Bevy の欠落(Cart の認識)**: `.bsn` loader、Assets as Entities、reactivity の収束、shader / material workflow。資金は「drastically underfunded」。同記事に 1.0 の言及なし — [Sixth Birthday](https://bevy.org/news/bevys-sixth-birthday/)
- **Bevy の本体未提供**: physics、navigation、networking、scripting、particles の crate がない — [crates/](https://github.com/bevyengine/bevy/tree/main/crates)
- **Fyrox**: 1.0.0 は 2026-03-24 に first stable — [Fyrox blog](https://fyrox.rs/blog/)。release 後は bug 修正中心で、大型機能は数か月保留と表明 — [Fyrox 1.0](https://fyrox.rs/blog/post/fyrox-game-engine-1-0-0/)
- **Godot の残課題の例**: C# は Android / iOS で experimental、web 非対応。visionOS は平面 app のみ(4.7 文書、4.8 で hand tracking 等を追加中) — [Feature list](https://docs.godotengine.org/en/stable/about/list_of_features.html)、[4.8 beta 1](https://godotengine.org/article/dev-snapshot-godot-4-8-beta-1/)
- **Godot Wayland**: touch は基本のみで pan、scale、long-press は未対応 — [Godot 4.7](https://godotengine.org/releases/4.7/)

### Inferences
- Bevy を土台にする判断は、定期的な破壊的 migration と editor 不在を受け入れられるかに帰着する。README は約 3 か月ごとと書くが、直近の release 間隔の実績は約 3.5〜5 か月である。
- Fyrox は 1.0 を名乗るが、book の「production-ready」は自己申告であり、採用事例は本調査では確認していない。

### Gaps
- Godot 4.6 の正確な release 日: 二次情報の 2026-01-26 と forum 掲載の 02-04 があり、一次情報で未確定。
- Godot 4.7 の wasm64 対応は二次情報のみ([gtstu](https://gtstu.com/?p=4758))で、公式 release page の要約には記載がなかった。
- Fyrox 1.0.0 以降の patch release の有無は未確認。
- Godot 4.8 stable の予定日は確認できていない。
