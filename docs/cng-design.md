# CNG の構成モデルと表現範囲

この文書は CNG が生成するプロジェクトと、ネイティブパッケージが所有する設定の境界を説明する。新しいサービスへの対応を判断するときは、IR のフィールドがないことと、パッケージで実装できないことを区別する。実装予定や個別サービスの API 設計は扱わない。

## 宣言的なプロジェクトモデル

`whisker_plugin::project` は、iOS・Android・macOS・Windows・Linux・Web の構成を表す型を定義する。明示的な `ProjectEngine` と versioned subprocess protocol でこのモデルを合成できるが、全6プラットフォームの標準生成へ接続されている。Windows/LinuxはCargo Hostと配布ツリーを生成し、package backendは未対応である。[モデルの入口](../crates/whisker-plugin/src/project/mod.rs)

| モデル | 所有する構造 |
|---|---|
| `project::IosProjectIr` | Native/Aggregate target、plist・Entitlements・privacy resources、header、生成 path、外部 product の embed、configuration/xcconfig、Swift package、scheme action/test plan |
| `project::MacosProjectIr` | 同じ Apple target graph。Cargo executable、helper、XPC service、bundle root への metadata 配置。Xcode project の生成を前提としない |
| `project::AndroidProjectIr` | 種類別の Gradle module graph、source set ごとの Manifest XML・ソース・リソース、省略可能な SDK 指定、build type / flavor、settings/root/module ごとの Gradle 設定と明示的な合成 API |
| `project::WindowsProjectIr` | 実行ファイルごとの Win32 manifest・icon・VERSIONINFO・resource script、配布リソース、独立した複数 MSIX package と主アプリ package の参照 |
| `project::LinuxProjectIr` | 実行ファイル・配置先、型付き Desktop Entry 値、MIME/AppStream XML、D-Bus service、Flatpak の native metadata/module/command、外部 packaging recipe |
| `project::WebProjectIr` | HTML/JS/Wasm の出力先、静的 HTML head/body、配布 URL、Web Manifest、複数 Service Worker 登録、HTTP response header の宣言 |

Apple の product type はネイティブの識別子を保持する。Share Extension や通知専用の target 型を CNG に追加する設計ではない。target ID と bundle ID は独立し、PBX object ID はモデルに含めない。link と embed を別々に宣言し、embed は build dependency を含意する。iOS と macOS の OS 固有の適合性は各 backend が判断する。[Apple model](../crates/whisker-plugin/src/project/apple.rs)

Android の Manifest と Windows/Linux の XML は、属性と順序を持つ子ノードの木として保持する。これにより SDK 固有のタグや namespace を model に追加せず表現できる。Gradle は適用 scope を型で区別し、その中の DSL は文字列を許す。native schema の検証、Gradle DSL の解釈、Android Manifest merger はこの model の仕事ではない。[Android model](../crates/whisker-plugin/src/project/android.rs)、[XML model](../crates/whisker-plugin/src/project/xml.rs)

`ProjectFiles` は生成ルートへの staging、`Resource` は最終 bundle・配布先への配置を表す。アプリ由来の入力は app crate 相対、staging 後の参照は生成ルート相対、配布先は各成果物の resource root 相対である。`ProjectPath` は絶対パス・親への traversal・Windows の drive prefix をデシリアライズ時にも拒否する。Windows の validator は代表的な Win32 禁止名と ASCII case-fold の衝突も確認する。実ファイル、symlink、完全な Unicode filename の比較は backend が担う。[ファイル契約](../crates/whisker-plugin/src/project/files.rs)

`ProjectIr::validate_structure` は main product、target/module/package 参照、Apple の platform filter ごとの依存 graph の循環、staging と宣言された配布先の重複を検証する。Android は configuration によって異なる graph を選ぶため、参照先と種類別の関係を確認し、実際の循環検出は Gradle に委ねる。検証はファイルシステムにアクセスせず、native build の成功や renderer の対応を保証しない。[構造検証](../crates/whisker-plugin/src/project/validate.rs)、[Android の検証](../crates/whisker-plugin/src/project/android/validate.rs)

新モデルの struct は未知フィールドを拒否し、情報が静かに欠落するのを防ぐ。map の key 順と list の宣言順を JSON に保持する。各プラットフォームは原子的な `merge_from` を持ち、同値の再宣言と異なる値の競合を区別する。Manifest/XML には明示的な編集 API があり、その他の意図的な置換はフィールド代入で行う。新しい `ProjectEngine` は読み取り用 context から宣言の合成または理由付きの置換を受け取り、最終構造を検証する。subprocess は事前に protocol/IR schema の一致を確認する。標準の旧 mobile engine とは別の入口である。[実行契約](cng-plugin-composition.md)署名鍵・証明書の取得、配布操作、実行時 lifecycle も model の外側にある。

6 プラットフォームの [fixture](../crates/whisker-plugin/src/project/fixtures) と [テスト](../crates/whisker-plugin/src/project/tests.rs) は、型の組み合わせ・JSON 往復・参照検証を確認する。fixture は構造テスト用の部分的なネイティブ設定であり、そのままビルド・配布できるテンプレートではない。

プラットフォームごとの公式仕様との対応、検証・合成の境界は [Android](cng-android-ir.md)、[iOS/macOS](cng-apple-ir.md)、[Web](cng-web-ir.md)、[Windows/Linux](cng-desktop-ir.md) に記載する。

## 現行の生成処理

ここでは「直接表現」は公開 IR と通常の生成処理で扱えるもの、「パッケージで対応」はネイティブの manifest・ビルドスクリプト、または追加ファイルで扱えるもの、「構造の制約」は複数 plugin の寄与を合成する専用の表現がないものを指す。

旧mobileの `extra_files` は生成ルート以下にテキスト・バイナリを配置できる。Android/iOSでは宣言的rendererが出力所有者の重複を検証し、構造化Manifest/build scriptをファイルで暗黙に覆うことを拒否する。ソース・リソース等のstaged filesはIR上の明示的な置換で更新する。[Androidのファイル契約](cng-android-ir.md)


## 共通の生成経路とpluginの適用範囲

標準の生成経路は `whisker.rs` → `Config` → プラットフォーム別の Cargo 依存グラフ → 生成入力 → `gen/<platform>` である。全6プラットフォームは空のIRから始め、`ProjectEngine` でapplication pluginと機能pluginの寄与を合成する。Android/iOSでは旧protocol pluginが存在するときはその前に旧engineで互換処理を行う。Web/macOS/Windows/Linuxでは旧mobile pluginは実行しない。`GenerateContext` と変更履歴の `Target` も iOS/Android のみを持つ。`desktop` は生成コマンドでは macOS の別名であり、Windows/Linuxはそれぞれ明示的な生成先を持つ。[生成の分岐](../crates/whisker-cng/src/generator.rs#L99)、[plugin context](../crates/whisker-plugin/src/lib.rs)、[生成先](../crates/whisker-cng/src/runner.rs#L8)

アプリ基本値の初期化・既定値解決は [application policy](../crates/whisker-cng/src/plugins/application.rs)、組み込み plugin の登録は [plugins](../crates/whisker-cng/src/plugins/mod.rs) が所有する。engine はその初期 context に plugin を順次適用する。基盤テンプレート・設定 helper・機能 plugin の責務と新モデルへの対応は [plugin の合成](cng-plugin-composition.md) に記載する。

この節は旧mobile context・engineの互換契約を説明する。標準生成経路と新protocolの制約は [Android IR](cng-android-ir.md) / [Apple IR](cng-apple-ir.md) を参照する。

### plugin構成と条件の境界

| 項目 | 現在の表現と限界 | 根拠 |
|---|---|---|
| Cargo features・プラットフォーム | 対象アプリの実行時依存グラフに含まれるcrateからmoduleとpluginを選択する。features対応は既にある。生成後のnative sliceと宣言の一致も検証する。 | [dependency graph](../crates/whisker-cng/src/dependency_graph.rs#L13)、[selection](../crates/whisker-cng/src/selection.rs#L13) |
| pluginが読む生成条件 | `GenerateContext`にはアプリ情報と対象プラットフォームのIRがあるが、CargoSelection、crateごとの有効features、native build configurationを読む専用フィールドはない。アプリのfeaturesとホストで動くgeneratorのfeaturesは別に扱われる。 | [context](../crates/whisker-plugin/src/lib.rs)、[generatorの分離](../crates/whisker-cng/src/project.rs#L11)、[pluginビルド](../crates/whisker-cng/src/generator.rs#L409) |
| 同一pluginの複数設定 | `Config.plugins`は名前をキーにしたmap。`plugin::<P>`を再度呼ぶと以前の設定を置換する。複数インスタンスを扱う場合はplugin自身の設定を配列などにする。 | [Config::plugin](../crates/whisker-config/src/lib.rs#L134) |
| ローカルplugin | 標準の発見処理は依存crateのmetadataを読む。アプリ自身のmetadataは対象外。ローカルpath依存crateは使える。独自の生成プログラムでは公開された`Engine::register`も使えるが、標準runnerがアプリ内の任意のplugin型を自動登録する機構ではない。 | [discovery](../crates/whisker-cng/src/discovery.rs#L102)、[Engine](../crates/whisker-cng/src/compose.rs) |
| 順序制約 | `before` / `after`と循環検出がある。参照先のpluginが存在しない場合はエラーであり、「存在すればその後」の条件付き制約はない。 | [topo_sort](../crates/whisker-cng/src/compose.rs) |
| 外部入力 | pluginはファイルや環境変数を自分で読み、IRへ反映できる。ただし、入力ファイル・環境変数・外部ツールを宣言して収集する共通インターフェースはない。 | [Plugin::apply / context](../crates/whisker-plugin/src/lib.rs) |

### 合成・プロトコル・生成結果の境界

`MutationJournal`はpluginが手動で記録する。エンジンは同じプラットフォーム・同じ文字列パスへの`Set`を検出し、`Override`を許可する。`ArrayPush`は競合判定から除外される。従って、全IRの差分から変更履歴が自動生成されるわけではなく、配列内の同じ識別子、同じファイルパス、親の辞書の置換と子キーの変更などがすべて検証されるわけでもない。IRを直接変更・削除することはできるが、汎用のキー単位のマージ・削除・所有権の意味は共通化されていない。[journal](../crates/whisker-plugin/src/lib.rs)、[競合検出](../crates/whisker-cng/src/compose.rs)

外部pluginにはcontext全体をJSONで渡し、返されたcontext全体を置換する。`PluginRequest` / `PluginResponse`にはプロトコル版や対応IR機能の宣言がない。古い型でcontextをデシリアライズして返すpluginは、その型が知らないフィールドを保持できない場合がある。IRを拡張するときに、単に新フィールドへ`serde(default)`を付けるだけで往復の互換性が保証される構造ではない。生成完了時の`GenerationReport.schema_version`は別の契約である。[envelope](../crates/whisker-plugin/src/lib.rs)、[context置換](../crates/whisker-cng/src/compose.rs)、[生成レポート](../crates/whisker-cng/src/runner.rs#L63)

| 項目 | 現在の動作と限界 | 根拠 |
|---|---|---|
| 出力ファイル | モバイルの`extra_files`は文字列・バイナリ・modeを扱える。別ファイルを追加できるが、renderer所有の出力との衝突は拒否する。構造化設定やstage済みファイルの変更は新IRを明示的に編集する。 | [FileEntry](../crates/whisker-plugin/src/lib.rs)、[iOS書き込み](../crates/whisker-cng/src/ios.rs)、[Android書き込み](../crates/whisker-cng/src/android.rs) |
| 再生成 | 最終的なInputsのfingerprintを比較し、一致すると生成をスキップする。モバイルでは通常のsync呼び出し時にplugin合成を先に行うため、`extra_files`に読み込んだ内容もfingerprintに反映される。入力ファイルの変更が一律に無視される設計ではない。 | [iOS inputs/sync](../crates/whisker-cng/src/ios.rs)、[Android sync](../crates/whisker-cng/src/android.rs)、[generator](../crates/whisker-cng/src/generator.rs#L178) |
| 生成物の検証 | fingerprintの一致判定は生成先ファイルの存在・内容をすべて比較するものではない。変更時は生成ツリーを整理して書き直す。pluginごとの出力所有権表や、全ファイル生成成功後に一括で切り替えるtransactionはない。 | [iOS書き込み](../crates/whisker-cng/src/ios.rs)、[Android書き込み](../crates/whisker-cng/src/android.rs)、[Web sync](../crates/whisker-cng/src/web.rs) |
| 事前確認 | 標準runnerは生成を実行する。設定のみの評価、最終IRの表示、書き込み前のファイル差分確認を選ぶモードはない。公開`Engine::compose`で独自の確認プログラムを作ることはできる。 | [runner契約](../crates/whisker-cng/src/runner.rs#L81)、[compose](../crates/whisker-cng/src/compose.rs) |


## iOS

標準生成は空のIRから始め、application pluginと機能pluginを合成した最終IRをXcode projectへ出力する。メインアプリに加え、Extensionなどの追加target、target間の依存とembed、targetごとのplist/entitlements、source/resource、SwiftPM product、configuration/xcconfig、scriptとschemeを宣言できる。詳細な出力範囲とbackendの非対応項目は [Apple IR](cng-apple-ir.md) に記載する。

既存pluginの `InfoPlistExtra` と `IosPbxprojOps` は主targetの宣言へ変換する。plistは完成した辞書をシリアライズし、混合配列やネストした辞書を保持する。SwiftPM aggregatorとRust build phaseはapplication pluginが所有する。追加targetへメインアプリのBundle IDやentitlementsを一律に適用せず、機能pluginが必要な対象へ宣言する。

lifecycle handlerの合成、署名・provisioning・配布、Extensionとアプリの実行時通信はCNGのproject生成とは別の責務である。SwiftPM manifestから最低iOS major versionを読む既存処理は限定的な文字列解析であり、manifest全体の評価ではない。[SwiftPM integration](../crates/whisker-cng/src/ios_modules.rs)

## Android

標準生成は空のIR、application pluginを先頭とするplugin合成、宣言的rendererの経路を使う。メインアプリだけでなく、追加Gradle module、source setごとのManifest、scope別のGradle設定を出力できる。詳細な表現範囲とbackendの非対応項目は [Android IR](cng-android-ir.md) に記載する。

旧mobile pluginは新IRに直接アクセスせず、互換段階で実行した結果をapplication pluginが宣言に取り込む。各パッケージが所有するlibrary Manifest・Gradle build scriptと、CNG所有のアプリ/module構造は別の層である。ライブラリManifestのmerge、nativeのvariant解決、実行時のActivity/Intent処理は、引き続きGradle/SDK/各ライブラリが所有する。

## Web

標準Web生成も空のIRからapplication pluginと機能pluginを合成し、最終IRからHTML・Manifest・Service Worker登録・追加リソースの配布計画を出力する。builderも計画を読み、宣言されたファイルをdistへ配置する。対応する出力名やHTTP headerの制限は [Web IR](cng-web-ir.md) を参照する。

## macOS の生成範囲

macOSも空のIRからapplication pluginを適用し、依存crateのproject pluginを合成する。完成したIRからCargo Host、Info.plist、entitlements、resource plist、iconsetとbundle配置計画を生成する。builderは計画に従ってCargoを実行し、宣言されたファイルだけを `.app` へ配置する。[macOSの構成](../crates/whisker-cng/src/generator.rs)、[renderer](../crates/whisker-cng/src/macos/project_render.rs)

現在のbackendは単一のCargo binaryを持つapplicationに対応する。任意のInfo.plist/entitlements、resourceとbundle root相対のファイル配置はpluginで構成できる。追加target、Swift/native source、Xcode設定、SwiftPM、embed/scriptはエラーにする。AppIconは既存の設定からapplication pluginがiconsetを宣言し、builderがiconutilを実行する。詳細は[Apple IR](cng-apple-ir.md)に記載する。

build/runは同じ配置計画を読み、最低OSを `MACOSX_DEPLOYMENT_TARGET` に渡す。bundleを一時ディレクトリで組み立て、IRのentitlementsを使ってad-hoc署名する。署名鍵を使う配布署名・provisioning・notarizationはこの経路に含まない。[macOS bundling](../crates/whisker-build/src/macos.rs)

## CNGと実行時機能の責務

CNGはホストプロジェクトへの設定・ソース・依存の組み込みを担う。受信Intentの解釈、通知の内容、保存データの管理、Rustへのデータ変換は各ライブラリの実行時実装である。Androidの生成Activityは`ComponentActivity`であり、既存の`currentActivity`・ホスト接続通知からAndroidXのlistenerへ接続できる。共有受信を理由にCNGへ共有専用callbackを追加しなければならない構造ではない。[MainActivity](../crates/whisker-cng/src/templates/android/app/src/main/kotlin/MainActivity.kt#L14)、[AppContext](../platforms/android/module/src/main/kotlin/rs/whisker/runtime/WhiskerAppContext.kt#L105)、[AndroidX listener](https://developer.android.com/reference/androidx/activity/ComponentActivity#addOnNewIntentListener(androidx.core.util.Consumer%3Candroid.content.Intent%3E))

Expoとの違いは、標準modsが解析済みManifestやXcode project、各種設定ファイルの編集を公開しているのに対し、Whiskerは限られた構造化IRと完全なファイル出力を公開している点にある。従って、Whiskerの表現範囲を評価するときは、ネイティブツールなら可能か、専用ライブラリ内で完結するか、複数pluginがアプリの同じ構造を合成できるかを分ける必要がある。[Expo Mods](https://docs.expo.dev/config-plugins/mods/)
