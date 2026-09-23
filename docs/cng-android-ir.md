# Android の宣言的プロジェクトモデル

この文書は `whisker_plugin::project::AndroidProjectIr` の表現範囲・合成・出力規則を説明する。標準 Android 生成は `ProjectEngine` に登録する [application plugin](../crates/whisker-cng/src/android/application.rs)、[renderer](../crates/whisker-cng/src/android/project_render.rs) を通る。crate root の旧 `AndroidProjectIr` は既存 plugin の互換経路に残る。[全体の位置づけ](cng-design.md)

## 標準の生成経路

標準生成は `AndroidProjectIr::default()` の空のIRから始める。旧protocolの依存pluginがなければ、`whisker-android-application` がアプリ本体を宣言し、組み込み設定pluginと `protocol = "project"` の依存pluginを合成して出力する。application pluginも通常の `ProjectPlugin` であり、`Merge` を返して実行記録に残る。アプリの基本値の解決は共通の application policy を再利用する。テンプレートは Host ソース・起動リソース・Rust/SDK 接続の断片を供給し、それらも基盤IRの files や適切な Gradle scope に入る。renderer が後から既定のアプリや Manifest を挿入することはない。

旧protocolの依存pluginがある場合は、組み込みpluginと旧pluginを既存 engine で合成し、その結果をapplication pluginの入力にする。新engineでは空のIRにapplication pluginが宣言を一度だけ取り込み、その後に新pluginを実行する。新IRを旧pluginに渡すことはない。この互換経路では新旧段階をまたぐ `before` / `after` は使えず、解決できない参照としてエラーになる。将来の段階を予約する名前や、存在しないplugin名を無視する規則はない。

`ProjectEngine::with_initializer` は指定したpluginから他の全pluginへの先行制約を追加する。標準経路ではapplication pluginをこの方法で登録する。名前の辞書順や登録順に依存せず、applicationより前に実行しようとする制約は循環エラーになる。新pluginは `after("whisker-android-application")` も宣言できる。この名前は標準経路が予約し、依存pluginによる再使用を拒否する。全subprocessの互換性と全pluginの順序を確認してからapplication pluginを含む寄与を開始する。

新しい外部pluginは `[package.metadata.whisker.plugins.<name>]` に `bin` と `protocol = "project"` を宣言する。省略時は `legacy`。Project protocol の順序情報はバイナリの handshake で受け取るため、Cargo metadata の `after` / `before` は併記できない。Cargo features による発見対象の選択は従来の依存グラフに従う。[発見処理](../crates/whisker-cng/src/discovery.rs)

標準 build driver は `:app` と `app/` を前提とするため、メインmoduleのID・場所を変更した場合は生成時に拒否する。追加moduleのID・場所は独立して指定できる。CNGでflavorや追加targetを宣言できることと、CLIで任意のvariantを選択できることは別の契約である。

## Renderer の範囲と書き込み

settings/root/module の Gradle scope、種類別module、SDK、build type/flavor、source set、Manifest XML、依存、properties、staged filesを出力する。source set の明示的なdirectory一覧はnativeの既定値を置換し、空の一覧はnativeの慣例に委ねる。初期アプリにはJava/Kotlin両方の標準source directoryを宣言する。[Gradle source directory](https://docs.gradle.org/current/kotlin-dsl/gradle/org.gradle.api.file/-source-directory-set/index.html)

現rendererは minor compile SDK と baseline profile の追加source directoryを明示的に拒否する。asset packのasset directoryはそのmoduleの`src/main/assets`だけを扱う。IRに保持できる値を省略して成功扱いにしない。必要なnative設定をraw DSLとして宣言する場合は、非対応の構造化フィールドを同時に指定しない。

`External` moduleのbuild fileはmodule直下に置き、staging後の存在を確認する。XMLの名前・制御文字・namespace prefixを検証し、値をescapeする。native Manifest mergerやGradle式の意味解析は実行しない。実際のAGP/plugin versionで利用できるDSLかどうかはnative toolingが判断する。

全出力をメモリ上で収集・検証してから既存ツリーを更新する。`AppFile` / `AppDirectory` はapp crateを起点に内容を読み、コピー内容もfingerprintへ含める。symlink入力、app crateを逸脱する入力、同じ出力の複数所有者、file/directoryの重複を拒否する。構造化されたManifest/build fileをstaged fileで暗黙に覆うことはできない。raw Manifestを使う場合は対象source setの構造化Manifestを明示的に外す。

再生成時は既存の`.gradle/`、`app/build/`、`app/src/main/jniLibs/`を維持する。追加moduleのbuild cacheを個別保存する仕組み、書き込み全体のtransaction、cache一致時の全生成ファイルの整合性確認はない。入力検証の失敗では以前の生成物を変更しないが、書き込み中のI/Oエラーまで原子的に巻き戻す契約ではない。

## Module の役割と参照

`AndroidProjectIr::default()` はmain applicationもmoduleも持たない合成開始用の値であり、そのままの出力は検証エラーになる。`application` の空文字は合成途中の未指定を表す。`Merge` は未指定を補完し、寄与側の空文字は既存のIDを消さない。異なる非空のIDは競合として拒否する。全pluginの合成完了後に、指定したmain applicationと参照先の存在を検証する。

`AndroidProjectIr.modules` は `:app` などの Gradle project path をキーに持つ。各 `AndroidModule` は生成ルート相対の directory、種類別設定、共通 build script、configuration 付きの依存を持つ。`application` はこの中の主アプリを指す。種類だけから Gradle plugin や version は推測せず、assembler が適用する plugin を宣言する。

| 種類 | 固有の設定・関係 |
|---|---|
| `Application` | `AndroidBuild`、独立した application ID、同梱する dynamic feature と asset pack の ID |
| `Library` | `AndroidBuild` |
| `DynamicFeature` | `AndroidBuild`、所属する base application ID |
| `Test` | `AndroidBuild`、instrumentation test の対象 application ID |
| `AssetPack` | pack 名、install-time / fast-follow / on-demand、asset directory。namespace・SDK は要求しない |
| `Jvm` | 共通 build script で Java/Kotlin JVM project を設定。Android extension を暗黙に作らない |
| `Custom` | KMP・Fused Library など、固有 extension を共通 build script の DSL で設定する module |
| `External` | 作者が所有する build file の参照。共通 build script と構造化された依存は空にし、二重の所有を防ぐ |

Dynamic feature の `base` は、renderer が `implementation(project(base))` を出力する契約である。app の `dynamic_features` は packaging 関係を表し、両者が相互に一致することを検証する。feature の delivery 条件は feature Manifest で記述する。Asset pack の配布方法とは別の設定である。[Feature Delivery](https://developer.android.com/guide/playcore/feature-delivery)、[Asset Delivery](https://developer.android.com/guide/playcore/asset-delivery/integrate-java)

独立 test module の `target` は `targetProjectPath` に対応する。`Custom` と `External` も module graph に登録されるため、他の module は通常の `GradleDependencySource::Project` から参照できる。外部 build file 内の依存までは解析しない。[独立 test module](https://developer.android.com/studio/test/advanced-test-setup#use-separate-test-modules-for-instrumented-tests)

## SDK と variant

Android app・library・feature・test は `AndroidBuild` を持つ。namespace は必須で、application ID とは独立する。`AndroidSdk` の compile/min/target はそれぞれ省略できる。省略は「module に宣言しない」という意味であり、CNG が具体的な SDK 値を補完する意味ではない。settings の `android_sdk` を使う場合も、必要な settings plugin を明示的に適用する。[Android settings plugin](https://developer.android.com/build/android-settings-plugin)

Compile SDK は release major・任意の minor・任意の extension、preview codename、vendor add-on を区別する。min/target は release API または preview codename を持つ。利用する AGP version での設定可否、SDK のインストール、組み合わせの妥当性は native backend の責務である。[CompileSdkVersion](https://developer.android.com/reference/tools/gradle-api/8.13/com/android/build/api/dsl/CompileSdkVersion)

`AndroidVariants` は名前をキーにした build type / product flavor と、優先順を保持した flavor dimension を持つ。renderer は名前ごとに既存オブジェクトを設定するか、一度だけ作成する契約である。空の build type map は Gradle の標準 debug/release を削除しない。matching fallback と missing dimension strategy も順序を保持する。[build variants](https://developer.android.com/build/build-variants)

defaultConfig・build type・flavor は `AndroidVariantValues` を持ち、Manifest placeholder、BuildConfig field、resValue をキー単位で合成できる。値は Kotlin の式であり、自動で文字列リテラルに変換しない。SDK の flavor ごとの上書き、application ID suffix、version、署名、R8 等は各 scope の追加 DSL で記述する。source-set 名や dependency configuration 名は native の名前を保持し、CNG が variant の全組み合わせを解決することはない。

## Gradle の scope と順序

settings・root build・各 module build は別々に保持する。root/module の `GradleBuildScript` は imports → buildscript → plugins → 種類別の構造化設定 → 追加 statements の順で出力する契約である。settings では imports → pluginManagement → buildscript → settings plugins → dependencyResolutionManagement → Android SDK defaults → includes → 追加 statements の順になる。現rendererもこの順序で出力する。[plugins DSL の制約](https://docs.gradle.org/current/userguide/plugins_intermediate.html#sec:plugins_block)

plugin 宣言は実際の plugin ID を必ず持ち、version または catalog alias、適用の有無を保持する。alias と version の同時指定は拒否する。plugin は配列で適用順を保存し、ID 単位で重複・競合を確認する。`pluginManagement.plugins` は ID と既定 version の map であり、適用用 plugin 宣言とは別の型である。

repository は安定した ID と Kotlin 式の組を配列で保持する。plugin 解決用と通常依存用を分け、検索順を保存する。後者は repositories mode と、catalog 等の追加 DSL も持つ。式内部の credentials・content filter 等は解析しない。included build も pluginManagement 内と通常 settings 内を分ける。[repository 宣言](https://docs.gradle.org/current/userguide/declaring_repositories_basics.html)

`GradleBuildScript.plugin_statements` は構造化plugin宣言の後に置くraw宣言を保持する。`GradlePluginManagement.statements` はrepositoryより前の条件付きlocal includeBuildなどを保持する。IR schemaはversion 6であり、空のAndroid application IDを未指定として合成する契約も含む。異なるschema versionのproject binaryはhandshakeで拒否する。

raw DSL は適用 scope を保持するが、意味解析はしない。構造化された設定と同じ値を後から上書きする DSL も書けるため、両者の意味上の競合を CNG が検出する保証はない。

## 合成規則

[合成 API](../crates/whisker-plugin/src/project/android/compose.rs) の `merge_from` は明示的に呼び出す加算的な操作である。`ProjectEngine` は `Merge` の寄与にこの操作を使う。旧 mobile engine の手動 journal とは別の契約である。

| 対象 | 合成の規則 |
|---|---|
| module・source set・build type・flavor | 同じキーのオブジェクトを再帰的に合成する |
| primary application ID | 空は未指定として補完する。異なる非空のIDは拒否する |
| scalar・キー付きの値 | 同値の再宣言は許可し、異なる値は field path 付きのエラーにする |
| optional 値 | 未指定の場所を補完する。指定済みの異なる値は拒否する |
| plugin・repository | ID で識別し、同一宣言は一つにする。同じ ID の異なる宣言は拒否する |
| 通常の配列 | 既存の順序を維持し、まだ存在しない値を末尾へ追加する |
| dimension・matching fallback | 空は未指定として扱う。両側が指定した優先順は完全一致を要求する |
| missing dimension strategy | dimension ごとの候補配列は順序も含めて一致を要求する |
| raw DSL | 完全に同じ文字列だけを重複排除する。等価な別表現や意味上の競合は認識しない |
| Manifest 全体 | 未指定なら補完し、指定済みなら同じ木を要求する。要素単位の変更は `edit_manifest` を使う |
| staged file | 同じ path の同じ宣言を許可し、異なる宣言は拒否する。親子 path の重複は最終構造検証で拒否する |

各 `merge_from` は途中で失敗した場合に元の値を保持する。意図した置換は呼び出し側がフィールドを明示的に代入する。変更の所有者や履歴を記録する仕組みは含まない。合成途中には未解決の module 参照があり得るため、全寄与を適用してから `ProjectIr::validate_structure` を呼び出す。

## Manifest の編集と native merger

Manifest は source set ごとに任意 XML tree を持つ。Activity・Service・Receiver・Provider、権限、queries、metadata、intent filter、`tools:` 属性等を保持できる。SDK ごとの要素型を CNG に追加する必要はない。属性の意味やクラスの存在は native schema と実装側が判断する。[Manifest 要素一覧](https://developer.android.com/guide/topics/manifest/manifest-intro#elements)

[Manifest 編集 API](../crates/whisker-plugin/src/project/android/manifest.rs) の `AndroidSourceSet::edit_manifest` は、一つの CNG tree 内で次を行う。

- 要素名と identity 属性の path で要素を選ぶ。例は application → `android:name` が一致する activity。
- `Upsert` は未存在の path を作る。属性の `Set` は同値の再宣言を許可し、異なる既存値を拒否する。`Override` と `Remove` で上書き・属性削除を明示する。
- 要素の `Remove` は未存在なら何もしない。root の削除、曖昧な複数一致、選択に使った identity 属性の変更は拒否する。
- `append` は異なる子ノードをそのまま保持し、完全一致のノードだけを重複排除する。異なる intent-filter を一つにまとめない。名前付き子要素の合成にはその要素を選ぶ `Upsert` を使う。
- 一つの batch は原子的に適用する。失敗した編集より前の変更も残らない。

selector は prefix と属性値を文字通り比較する。namespace 宣言、`.MainActivity` と完全修飾クラス名の正規化は呼び出し側の責務である。追加する root 要素は application の前に置くが、それ以外の XML の合法性・順序を自動修正しない。

この編集は variant/main/library の複数 Manifest を統合する Android Manifest merger とは別である。`tools:node` や `tools:replace` は属性として保持され、native merger が処理する。一つの tree から削除した要素が library Manifest に存在する場合、それを最終 Manifest から除外するには native merger の指示が別途必要になる。[Manifest merger](https://developer.android.com/build/manage-manifests)

## ソース・ファイルと追加 DSL の範囲

`AndroidSourceSet` は Java/Kotlin sources、Android `res`、Java-style `resources`、AIDL、shader、baseline profile、assets、JNI library の directory を分ける。全 path は生成ルート相対で、ファイル内容は `ProjectFiles` で stage する。resource qualifier は directory 名で表す。[AndroidSourceSet](https://developer.android.com/reference/tools/gradle-api/8.13/com/android/build/api/dsl/AndroidSourceSet)、[resource qualifiers](https://developer.android.com/guide/topics/resources/providing-resources)

| 領域 | 表現と責務 |
|---|---|
| 生成ソース・artifact 変換 | Gradle plugin / Android Components DSL で task/provider を接続する。静的 directory は生成順序を表さない。[AGP 拡張 API](https://developer.android.com/build/extend-agp) |
| Maven / project / BOM / catalog / kapt・KSP | configuration と Maven/Project/Kotlin 式を組み合わせる。exclusion・constraint 等は追加 dependencies DSL で記述する。[依存設定](https://developer.android.com/build/dependencies) |
| Gradle / AGP / JDK / Kotlin / NDK | plugin version・toolchain・native 設定は DSL / ファイルで表す。互換性検証や導入は行わない。Wrapper properties と `gradle.properties`、Gradle の起動 JDK と Java toolchain はそれぞれ別である。[Wrapper](https://docs.gradle.org/current/userguide/gradle_wrapper.html)、[Java versions](https://developer.android.com/build/jdks) |
| CMake / ndk-build / ABI / Rust | JNI directory は構造化し、ビルド接続は native plugin / DSL で行う。Whisker の Cargo/variant/ABI 接続は既存 Gradle plugin の責務である。[native builds](https://developer.android.com/studio/projects/gradle-external-native-builds)、[Whisker Gradle plugin](../platforms/android/gradle-plugin/whisker-gradle-plugin/src/main/kotlin/rs/whisker/gradle/WhiskerProjectPlugin.kt) |
| R8・resource shrinking・consumer rules・packaging | build type / Android extension の DSL と rule file 等で表す。[R8](https://developer.android.com/topic/performance/app-optimization/enable-app-optimization) |
| 署名・APK split・App Bundle | signingConfigs・build type・split 設定等は DSL / ファイルで表す。鍵の取得・build task 選択・署名実行・Play upload はモデル外である。[署名](https://developer.android.com/studio/publish/app-signing)、[APK splits](https://developer.android.com/build/configure-apk-splits) |
| テスト・lint・managed devices | source set、test module、任意 configuration と DSL で表す。native task 実行は別である。[test 設定](https://developer.android.com/studio/test/advanced-test-setup) |

Android resource は plugin ごとの XML ファイルや directory に分けて追加できる。同じ resource name の競合や共有 `strings.xml` の値の置換は、この合成 API では解決しない。`ProjectFiles` は directory 全体の staging とその子への追加を重複として拒否するため、独立した resource directory を追加する方法と、一つの共有木を編集する方法を区別する。[file contract](../crates/whisker-plugin/src/project/files.rs)

## 構造検証の範囲

[validator](../crates/whisker-plugin/src/project/android/validate.rs) は primary application、module ID・directory の重複、project 参照、feature/base と app/asset-pack の整合性、test の対象、External の所有権、flavor dimension、plugin/repository ID、SDK の基本的な形式、Manifest root を検証する。ファイルシステムや SDK にアクセスしない。

Android の依存は configuration ごとに異なるため、module 参照をすべて一つの graph に集めた循環判定は行わない。たとえば `:a` の `debugImplementation(:b)` と `:b` の `releaseImplementation(:a)` は構造検証を通る。実際の configuration・variant・task の選択と循環検出は Gradle が行う。[Gradle configurations](https://docs.gradle.org/current/userguide/java_plugin.html#sec:java_plugin_and_dependency_management)、[variant-aware matching](https://developer.android.com/build/build-variants#resolve_matching_errors)

[fixture](../crates/whisker-plugin/src/project/fixtures/android.json) と [テスト](../crates/whisker-plugin/src/project/android/tests.rs) は型の組み合わせ、JSON 往復、参照検証、合成の順序・競合・原子性、Manifest 編集を確認する。native build を保証するサンプルではない。実行時 permission request、Intent の解釈、通知、lifecycle 接続は各ライブラリと Host が実装する。
