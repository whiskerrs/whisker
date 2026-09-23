# iOS・macOS の宣言的プロジェクトモデル

`whisker_plugin::project` の `IosProjectIr` と `MacosProjectIr` は、共通の `AppleProjectIr` を持つ。[型定義](../crates/whisker-plugin/src/project/apple.rs) は Xcode のオブジェクト ID に依存しない。iOSの標準生成はこのモデルを合成し、Xcode projectへ出力する。macOSも同じモデルを合成し、Cargo/bundle backendへ出力する。[全体の位置づけ](cng-design.md)

## Product と target

`targets` は安定した ID で target を識別し、主アプリの ID を `application` に保持する。Native target の `product_type` は開いたネイティブ識別子である。アプリ、Share / Notification / Widget 等の extension、framework、test、XPC service、helper ごとの enum をコアに追加する設計ではない。Aggregate target は product type を持たず、build-order dependency と script で共通の生成作業を表す。[Apple の build phase](https://developer.apple.com/documentation/xcode/customizing-the-build-phases-of-a-target)、[共通生成作業と Aggregate target](https://developer.apple.com/videos/play/wwdc2021/10210/)

Native target は Rust executable を直接使うことも、Swift 等のホストから Rust static library をリンクすることもできる。したがってアプリに Rust の Bin を一律に要求しない。ツールや static library に Info.plist・entitlements を必須にすることもない。OS ごとの product type の適法性と renderer の対応範囲は backend が判断する。

| 領域 | モデルと native 側の境界 |
|---|---|
| Info.plist / entitlements | target ごとの完全な plist 辞書。NSExtension、URL / document type、App Groups 等のキーをそのまま保持する。権限の取得や runtime handler は別である。[Bundle configuration](https://developer.apple.com/documentation/bundleresources/bundle-configuration) |
| privacy manifest | `resource_plists` に `PrivacyInfo.xcprivacy` 等を保持できる。辞書の合成は行えるが、収集データや required reason API の意味・審査適合性は検証しない。[Privacy manifest](https://developer.apple.com/documentation/bundleresources/privacy-manifest-files) |
| ソース / header | 静的ファイルまたは native build path expression。header は Public / Private / Project を区別する。[Build phases](https://developer.apple.com/documentation/xcode/customizing-the-build-phases-of-a-target) |
| native resource | `Process` は asset catalog・storyboard・string catalog 等を native tool に渡す。`Copy` は resource root 相対で配置する。ローカライズの内容は native resource を保持し、専用 XML/JSON schema は複製しない。[String catalogs](https://developer.apple.com/documentation/xcode/localizing-and-varying-text-with-a-string-catalog) |
| link | project target、Swift package product、system framework、staged library/framework/XCFramework、build output expression を区別する。build-order dependency と link、weak link は独立する |
| embed | project target、staged product、Swift package product、build output expression を別に指定する。コピー先は bundle root 相対で、sign-on-copy・header 除去の意図を保持する。XCFramework は対応 slice を選び、コンテナ全体を埋め込まない。[XCFramework](https://developer.apple.com/documentation/xcode/creating-a-multi-platform-binary-framework-bundle) |
| SwiftPM | local package、remote URL と exact / next-major / next-minor / range / branch / revision。product の実在・static/dynamic・解決可能性は SwiftPM/backend が判定する。[PackageDescription](https://docs.swift.org/package-manager/PackageDescription/PackageDescription.html) |

## 設定階層と build path

Project と target はそれぞれ common build settings と configuration map を持つ。`AppleBuildConfiguration` は staged xcconfig とその上に適用する inline settings を保持する。設定を一つの map に平坦化せず、project / target の階層を保つ。configuration が project に明示されている場合、target と scheme の参照先を確認する。空の project configuration map は backend の既定構成を使用する契約である。[xcconfig と優先順位](https://developer.apple.com/documentation/xcode/adding-a-build-configuration-file-to-your-project/)

Build setting は scalar または順序を持つ値のリストで、`$(inherited)` や SDK 条件付きのキーは native 表記を保つ。deployment target、signing、capabilities に関連する build setting、module map、検索 path 等のために DSL 全体を専用型へ翻訳しない。

`AppleBuildPath` は literal な生成ルート相対 path と、`$(DERIVED_FILE_DIR)/Generated.swift` 等の expression を区別する。expression を staging resolver に渡したり、CNG 実行時の環境で展開したりしない。script は input/output と `.xcfilelist`、dependency analysis の指定を持つ。phase position は配置の指定であり、実際の実行順序は native build system の依存関係に従う。[Custom scripts](https://developer.apple.com/documentation/xcode/running-custom-scripts-during-a-build)

ソース・link dependency・embed の `platform_filters` は native の platform token を保持し、空なら全 platform を対象にする。宣言された token ごとに有効な target graph を作って循環を検証するため、別 platform で逆向きの依存を持つ構成を、union graph の循環として誤拒否しない。token の意味や native task の暗黙依存は backend に委ねる。[Multiplatform target](https://developer.apple.com/documentation/xcode/configuring-a-multiplatform-app-target)

## Scheme とテスト

Scheme は build/run/test の target 参照、run/archive/test/profile/analyze の configuration、target ごとの build-for override を持つ。action ごとに引数・環境変数・名前付き pre/post script を分け、script の build-setting environment を提供する target も参照できる。backend が対応できない action/option の組み合わせは、黙って捨てずにエラーにする契約である。[Schemes](https://developer.apple.com/documentation/xcode/customizing-the-build-schemes-for-a-project)

Test plan は staged `.xctestplan` への参照と default plan を保持する。plan 内部のテスト条件・選択・繰り返し等は native JSON ファイルのまま扱う。extension のデバッグでは host app が必要になる場合があるため、run target が必ず主アプリでなければならないという検証はしない。[Test plans](https://developer.apple.com/documentation/xcode/organizing-tests-to-improve-feedback)

## macOS の bundle

macOS でも Cargo executable だけで metadata が不要になるわけではない。Info.plist、entitlements、resource 配置、nested product の embed を同じモデルで表せる。Xcode project の生成は前提ではなく、Cargo-based backend が bundle を組み立ててもよい。

`bundle_files` は resource root と別の、bundle root 相対の配置を表す。LaunchAgent/Daemon の plist、`Contents/Library` 以下のデータを、resource path からの `..` による移動を使わず配置できる。XPC service や login item は別 target と embed で表す。[Bundle layout](https://developer.apple.com/documentation/bundleresources/placing-content-in-a-bundle)、[SMAppService](https://developer.apple.com/documentation/servicemanagement/updating-helper-executables-from-earlier-versions-of-macos)

SMAppService の登録・IPC・通知処理・lifecycle は runtime 側、署名鍵の取得・provisioning・notarization・配布は build/distribution workflow の責務である。IR の宣言だけで実行するものではない。

## 合成と検証

`merge_from` は project、target、configuration、scheme、action options、plist に対する原子的な操作である。target/package/scheme/configuration は ID、source/header は path、script は名前、コピーは destination を基準に合成する。同値の再宣言は許可し、異なる値は field path 付きのエラーにする。途中で失敗しても元の値は変わらない。[合成実装](../crates/whisker-plugin/src/project/apple/compose.rs)

Plist dictionary は再帰的に合成する。配列、build-setting のリスト、script 本文、Rust build declaration は内部の意味を推測しない。特に引数の重複やフラグと値の組は保持し、異なる順序を自動で並べ直さない。意図した置換・配列追加は呼び出し側が明示的に行う。

構造検証は参照、configuration、Aggregate の制約、platform ごとの target cycle、明示された出力 path、script の識別子、test plan の参照を扱う。native build、署名、bundle layout、resource compiler が出す実ファイル名までは検証しない。resource root と bundle root が指す実際の場所、generated path expression、XCFramework slice の展開後の衝突も backend で確認する。[validator](../crates/whisker-plugin/src/project/apple/validate.rs)、[回帰テスト](../crates/whisker-plugin/src/project/apple/tests.rs)

Xcode の全 target/action schema を網羅するモデルではない。External Build Tool target、明示的な任意 build-rule、resource membership の platform filter、scheme の全 debugging option は専用構造を持たない。native script・build setting・resource file で表せることと、CNG が構造的に合成できることを区別する。

## iOS の標準生成と renderer

標準経路は、空の `IosProjectIr` → application plugin → 組み込み・外部の機能plugin → 最終構造検証 → rendererとなる。[application plugin](../crates/whisker-cng/src/ios/application.rs) は `whisker-ios-application` という名前で最初に登録され、主target `app`、AppDelegate、起動画面、plist、scheme、Rust build phase、SwiftPM aggregatorを `Merge` で宣言する。他のpluginも同じ `ProjectPlugin` 契約を使い、追加targetの宣言や、理由付き `Replace` による既定値の変更ができる。空の `application` は合成途中の未指定値であり、最終検証では有効な主targetを必須とする。

`Config` とCargoが解決したmodule一覧を `IosInputs` にまとめ、application pluginのコンストラクタへ渡す。ユーザー向けの `Config`、組み込み設定pluginの名前と設定方法は維持する。`WhiskerModules` は `whisker_modules` にstageし、Rust build scriptの生成物は `BuildOutput` としてリンク・埋め込みを宣言する。expressionはXcodeが展開する。IR schemaはversion 6であり、異なるschemaのproject pluginは生成前のhandshakeで拒否する。

旧protocolの依存pluginがある場合は、組み込みpluginと旧pluginを旧engineで実行し、その結果をapplication pluginが取り込む。その後にproject protocolのpluginを合成する。新旧をまたぐ `before` / `after` は使えず、互換段階の値を変更するときは新IRを明示的に編集する。旧pluginがない場合は組み込みpluginも新engineの中で動く。[共通の合成契約](cng-plugin-composition.md)

[renderer](../crates/whisker-cng/src/ios/project_render.rs) は最終IRからPBX object、shared scheme、targetごとのplist/entitlements、stageするファイルを決定する。PBX IDはIRのIDから決定的に生成する。固定pbxprojやplistテンプレートへの追記は行わず、辞書・混合配列・Data・Dateを保持して出力する。plist内の不正な文字、非有限実数、不正なDateはエラーとする。出力の所有者重複、ファイルとディレクトリの衝突、入力の欠落等を先に検証し、読み込んだファイル内容とmodeもfingerprintに含める。書き込み全体のtransactionではなく、一致時に手編集された生成物を修復する仕組みでもない。

対応する出力は、Native/Aggregate target、source/header/resource、リンクとweak link、platform filter、明示的なtarget dependency、copy/embed、SwiftPM参照、project/targetのconfigurationとxcconfig、入出力を持つscript、shared schemeとtest planである。link/embedに同じtargetの異なるplatform filterがある場合、build dependencyにはその和集合を設定する。空のfilterがあれば全platformの依存になる。resourceの改名・入れ子配置とbundle rootへの配置は明示的なcopy scriptへ変換する。

追加Extensionは独立targetとしてproduct type、Bundle ID、NSExtension辞書、Swift source、必要なentitlementsを宣言し、アプリtargetの `embeds` に `PlugIns` を追加する。Swiftソースのstageとtargetへの所属は両方必要である。[Share Extensionを合成する実例・Xcodeビルドテスト](../crates/whisker-cng/tests/ios_project.rs)

現在のbackendの境界は次のとおり。

- product typeはアプリ、App Clip、watch app/extension、App Extension/ExtensionKit、framework、bundle/test、static/dynamic library、toolの対応済み識別子を出力する。未知の識別子はエラーにする。個々のOSでの適法性や実行可能性はXcodeが判定する。全種類の実機ビルドを保証するものではない。
- project configurationが空ならDebug/Releaseを使い、それ以外を参照するtargetやschemeは拒否する。独自configurationはproject mapへ明示する。
- 標準run/buildのschemeは主アプリをrun targetとし、そのproduct名は生成project名と一致させる。追加schemeは別途宣言できる。test/analyzeのconfiguration省略時はrun、profile省略時はarchiveを使う。
- schemeのAnalyze action options、Build/Archiveの引数・環境変数は未対応として拒否する。汎用 `AppleTarget.rust` のbuild接続と、staged XCFrameworkのslice選択を伴うembedも未対応として拒否する。標準WhiskerのRust接続はapplication pluginがscriptとBuildOutputで宣言する。
- localizationのvariant groupや地域一覧の合成はまだなく、projectのdevelopment regionはen、known regionsはen/Baseを出力する。native resourceの同梱は可能である。
- AppDelegate/Sceneへのメソッド単位のlifecycle合成はない。機能pluginは必要なソースを宣言し、既存entry pointを変更する場合はstage済みソースを明示的に置換する。署名・provisioning・配布、共有データや通知の実行時処理は別の責務である。


## macOS の標準生成と renderer

空の `MacosProjectIr` → `whisker-macos-application` → project protocolの機能plugin → 最終検証 → rendererという経路を使う。application pluginはCargo manifest/main source、主target `app` と既定Info.plistを宣言する。AppIcon入力があればiconsetをstageし、`AppleResource::Process` を追加する。旧mobile protocolはmacOS contextを持たないので実行しない。[application plugin](../crates/whisker-cng/src/macos/application.rs)

[renderer](../crates/whisker-cng/src/macos/project_render.rs) は単一のCargo binary applicationを扱う。完成したInfo.plist/entitlementsとresource plistはiOSと共通のserializerを使う。`resources` のCopyは `Contents/Resources`、`bundle_files` は `.app` root相対へ配置する。directoryは実ファイルに展開し、空・欠落directoryはエラーにする。Process resourceは `.iconset` のみ対応し、iconutilによる `.icns` の出力先も衝突検証に含める。

`.whisker/macos-build.json` はCargo manifest/package/binary/features、最低OS、entitlements、bundleに含めるfile、icon変換をbuildへ渡すversion付き計画である。stagingだけではファイルをbundleへ含めない。rendererは入力bytes/modeをfingerprintに含め、書き込み前に入力欠落、出力の重複・親子の衝突、大小文字だけが異なるpath、symlinkを検証する。Unicodeの正規化やOSのすべてのファイル名規則を検証するものではない。

builderは計画のfeatures/default-featuresをCargoへ渡し、呼び出し時のhot-reload featuresを追加する。`LSMinimumSystemVersion` をdeployment target環境変数へ渡す。ファイルとiconを一時bundleへ配置し、宣言したentitlementsを使ってad-hoc署名した後に旧bundleと置き換える。入力欠落・iconutil・codesignの失敗時は旧bundleを残すが、最終置換や生成ファイルの書き込み全体のtransactionではない。配布用の署名identityやnotarizationは扱わない。[builder](../crates/whisker-build/src/macos.rs)

追加target/Extension/helper、native sources/headers、Xcode build settings/configurations/schemes、SwiftPM、link dependencies、embeds、build scriptsはまだ実行できないので、宣言されれば生成時に拒否する。plistのbuild-setting expressionは展開せず、`CFBundleExecutable` はRust binary名と一致する必要がある。標準build/runの名前契約に合わせ、主product名とgenerated package/binary名の変更はアプリ設定から行う。window titleや背景はapplication pluginが宣言するRust sourceにあり、Info.plistの変更から再合成しない。

[whisker-asset](../packages/whisker-asset/README.md) はiOSと同じfolder resource宣言で、macOSでは `Contents/Resources/whisker_assets` へ配置する。実行時はbundle内のexecutableからresource rootを解決するため、作業ディレクトリに依存しない。[生成テスト](../crates/whisker-cng/tests/macos_project.rs)、[asset生成テスト](../packages/whisker-asset/tests/plugin_e2e.rs)

newsのmacOSリリースビルドで `.app` の13アセットが元ファイルと一致すること、`codesign --verify --strict` が成功すること、起動後に写真が表示されることを確認した。小さなCargo fixtureではfeature/default-featureの適用、別の作業ディレクトリからの起動、entitlementsの署名反映、入力欠落時の旧bundle維持を検証している。
