# CNG plugin の責務と合成

CNG のアプリ設定、機能ごとの寄与、ネイティブプロジェクトの基盤構造は、それぞれ異なる所有者を持つ。この文書は現行の生成経路でその境界を説明し、独立した `whisker_plugin::project` モデルで対応する構造を示す。

全6プラットフォームの標準生成は `ProjectEngine` と宣言的rendererを使う。旧protocolの依存pluginは、互換段階で crate root のモバイル用 `GenerateContext` を編集する。[全体の表現範囲](cng-design.md)

## 宣言的プロジェクトの実行経路

[ProjectEngine](../crates/whisker-cng/src/project_compose.rs) は一つのプラットフォームを対象に、初期 `ProjectIr` と `Config.plugins` を合成する。初期IRは未完成でもよい。全6プラットフォームの標準生成は空のIRから始め、[Android](../crates/whisker-cng/src/android/application.rs) / [iOS](../crates/whisker-cng/src/ios/application.rs) / [Web](../crates/whisker-cng/src/web/application.rs) / [macOS](../crates/whisker-cng/src/macos/application.rs) / [Windows・Linux](../crates/whisker-cng/src/desktop/application.rs) のapplication plugin がアプリ本体、既定値、entry point、Rust/SDK接続を `Merge` で宣言する。基本値の解決は共通の `plugins/application` を再利用する。engineやrendererはアプリ固有の既定値を補わず、元のアプリ設定を後から再適用しない。

`ProjectEngine::with_initializer(plugin)` はそのpluginを最初に実行するregistryを作る。各pluginの順序制約に initializer → 他のplugin の辺を加え、矛盾は通常の循環検出で拒否する。initializerにも同じ設定デコード・寄与適用・競合検査・実行記録を使い、専用の実行段階で検査を飛ばさない。initializerの指定がない `ProjectEngine::new()` も使用できる。完成した基盤を渡すことや、別のapplication pluginで構築することも可能である。

1. 全 subprocess plugin に、プロジェクト情報を含まない `Describe` を送る。対応 protocol/schema と plugin 名・順序制約を確認する。
2. typed plugin と subprocess の descriptor を共通の順序処理へ渡す。重複名・存在しない順序参照・循環・未登録の設定を拒否する。
3. 初期IRのコピーとアプリ入力ディレクトリから読み取り用 `ProjectContext` を作る。
4. 順序に従い各 plugin の設定を検証し、寄与を受け取り、engine が適用する。未指定の設定は plugin の default であり、自動的な無効化を意味しない。
5. 全寄与の適用後、`validate_structure` で参照先・ファイル配置・対象ごとの構造を検証する。途中の未解決参照は許す。
6. 最終 IR と engine が記録した実行順・操作・置換理由を返す。renderer の呼び出しやファイル書き込みは行わない。

呼び出し側の初期IRはエラー時にも変わらない。ただし plugin が独自に行ったファイル書き込みなどの外部副作用は巻き戻せない。型と subprocess の双方で同じ `ProjectPlugin::validate` / `contribute` を使う。[結合テスト](../crates/whisker-cng/tests/project_composition.rs)

### 合成・上書き・既定値

plugin は context を直接変更せず、[ProjectUpdate](../crates/whisker-plugin/src/project/plugin.rs) を返す。

| 操作 | 意味 |
|---|---|
| `Keep` | 寄与なし。対象外のプラットフォームにも使用する |
| `Merge` | 同じプラットフォームの宣言を `ProjectIr::merge_from` で原子的に合成する。同値は再宣言可能、異なる scalar や識別子の衝突はエラー |
| `Replace` | 意図的な編集・削除後のプロジェクト全体を返す。空でない理由を必須とし、実行記録に残す。プラットフォームの変更は禁止 |

application pluginの既定値も通常の宣言として扱う。`Merge` は「既定値だから弱い」という優先順位を推測しない。変更する plugin は受け取ったプロジェクトを複製し、型付きフィールドや XML の編集操作で変更して `Replace` を返す。これはプロジェクト全体の置換なので、無関係な寄与を保持する責任は作者にある。別 plugin の値を変更する場合は `after` / `before` で依存順序も指定する。

競合時は新しい writer、先行 plugin 一覧、IR 内の競合箇所が診断に含まれる。実行記録は field 単位の所有権表ではなく、`Replace` の内部差分を検証するものでもない。native DSL の意味的な衝突は検出しない。最終検証の成功は native build の成功を保証しない。

設定は `Config::project_plugin::<P>` で保存し、実装は `ProjectEngine::register` または `register_subprocess` で登録する。設定と実装の登録は独立している。Android/iOSの標準コマンドはCargo metadataの `protocol` を読み、新旧の実装をそれぞれの経路へ登録する。旧 `Plugin` をそのまま新しい engine に登録することはできない。新旧段階の順序・制約は [Android](cng-android-ir.md) / [iOS生成経路](cng-apple-ir.md) を参照する。

### subprocess の互換性

[project::protocol](../crates/whisker-plugin/src/project/protocol.rs) は全メッセージに protocol と IR schema の必須 version を付ける。双方とも完全一致が必要で、欠落や不一致は typed payload のデコード前に拒否する。IR/context/update の wire schema は default 付きフィールドの追加でも schema version を更新する。空のapplication IDの合成規則など、非互換の合成契約もschema versionに含む。crate version や生成レポートの version とは独立した契約である。

`Describe` と `Contribute` は別プロセスで実行する。寄与要求には確認済み descriptor を含め、plugin 側とengine側の両方で再確認する。応答の protocol/schema も再検証する。古い mobile plugin への暗黙のフォールバックはしない。

応答は context 全体ではなく明示的な操作であり、アプリ入力ディレクトリや過去の実行記録を書き換えられない。ただし、同じ schema を宣言した作者が意図的に `Replace` した内容まで自動修復するものではない。これは互換性の検証であり、外部バイナリを sandbox 化する仕組みではない。

## 標準生成経路の所有者

| 責務 | 現在の所有者 | 境界 |
|---|---|---|
| アプリ名・ID・version・SDK 指定などの初期化と既定値 | [plugins/application](../crates/whisker-cng/src/plugins/application.rs) | 必須の内部処理。ユーザーが登録する任意の `Plugin` ではない |
| メイン成果物、entry point、Rust/SDK 接続、標準 native 構造 | 各プラットフォームのapplication plugin | 標準経路では自動登録するpluginの寄与 |
| アイコンやライブラリ固有のファイル・metadata | 機能 plugin と platform 固有の処理 | 機能を使うための複数の寄与をまとめる |
| plist・Manifest・Gradle などの直接設定 | 組み込みの設定 plugin | 呼び出し側が指定した native 設定を適用する |
| plugin 登録一覧 | [mobile registry](../crates/whisker-cng/src/plugins/project.rs)、[旧registry](../crates/whisker-cng/src/plugins/mod.rs) | 公開名と module path を維持する。登録順は実行順ではない |
| plugin 順序、設定のデコード、実行、寄与の合成 | 標準経路はProjectEngine、旧mobile互換経路は [Engine](../crates/whisker-cng/src/compose.rs) | 個別 SDK の設定内容は所有しない |
| Cargo features に応じた依存・module/plugin 発見 | [dependency graph](../crates/whisker-cng/src/dependency_graph.rs)、[discovery](../crates/whisker-cng/src/discovery.rs)、[generator](../crates/whisker-cng/src/generator.rs) | engine に渡す構成の選択。plugin の native 寄与とは別の層 |
| ファイルへの変換、fingerprint、生成先への書き込み | platform renderer | 全6プラットフォームで最終IRを出力する |
| Intent・通知・共有データの処理と Rust への受け渡し | 各ライブラリと Host の実行時実装 | CNG は設定とコードの組み込みを担当する |

`application` は iOS/Android の初期 context と、iOS/Android/macOS/Web のアプリ基本値の解決を担当する。背景の検証は共通の [background](../crates/whisker-cng/src/background.rs)、画像の読み込み・変換は各機能側に残る。アプリの全設定や native 構造をこの module に集約する境界ではない。

## 標準生成経路のアプリ設定と実行順序

旧protocol pluginを含むAndroid/iOSの互換段階は次の順で処理する。

1. `Config` のアプリ情報を `application::initial_context` で既存 IR に入れる。プラットフォーム別 ID は共通 ID より優先する。iOS の orientation と Android の URL scheme もここで初期化する。
2. engine が未登録 plugin の設定を拒否し、`before` / `after` に従って順序を決める。
3. 各 plugin の設定をデコードし、その plugin の `validate`、`apply` を順に実行する。
4. engine が返された journal の競合を確認する。
5. platform adapter が最終 IR を `application` に渡し、必須の名前・IDを確認して、未指定の基本値を解決する。その結果を生成入力へ変換する。

基本設定は plugin が ID などを参照するために実行前から存在する。実行後に元の `Config` を一括で適用すると、plugin が変更・解除した値を戻してしまう。既定値は最終 IR から解決する。たとえば iOS plugin がアプリ名を変更し scheme を `None` にした場合、scheme は変更後のアプリ名になる。[回帰テスト](../crates/whisker-cng/tests/core_field_override.rs)

`Engine::compose` 単体は手順 4 までであり、renderer 向けの必須値確認・既定値解決は行わない。初期値は journal に記録されず、plugin の `Set` / `Override` はその後の記録に対して検査される。最終段階で任意の「アプリ上書き plugin」を自動実行する仕組みはない。

Web/macOSもapplication pluginを先頭にproject protocolのpluginを合成し、旧mobile pluginは実行しない。macOSのAppIcon設定はapplicationの入力解決で読み、iconsetとそのprocess resourceをIRへ宣言する。Windows/Linuxも専用application pluginを先頭に合成し、Cargo Hostと配布planを生成する。

## 組み込み plugin と新モデルの対応

以下は10個の組み込み設定pluginと新モデルの対応である。Android/iOS標準経路はこれらに加え、application pluginを先頭に登録する。Androidでは設定デコード・画像処理を既存実装と共有し、メインmoduleのManifest・Gradle・filesへ寄与する実装を持つ。iOSではメインtargetのplist・source/resource・settings・filesへ寄与する。対象外のプラットフォームでは寄与なしとなる。

| plugin | 現在の寄与 | 新モデルで対応する構造 |
|---|---|---|
| `InfoPlistExtra` | メイン iOS アプリの追加 plist 値 | 選択した Apple target の `info_plist` |
| `AndroidPermissions` | permission 名の追加 | アプリ module の main source set の Manifest XML |
| `AndroidMetaData` | application の `meta-data` | 対象 Manifest の application 配下の XML node |
| `AndroidApplicationAttributes` | application 属性 | 対象 Manifest の application の属性 |
| `GradlePlugins` | アプリの Gradle plugin 宣言 | 対象 module の `build.plugins`。ID・version・alias・apply を表す。raw宣言は `plugin_statements` へ保持する |
| `GradleDependencies` | アプリの Gradle dependencies DSL | 対象 module の dependencies と configuration / Kotlin expression。任意の block は module の statements で表せるが、既存 DSL の自動解析は行わない |
| `IosExtraFiles` | iOS 生成ルートへのファイル配置 | `ProjectFiles` の staging。source/resource 所属は別途宣言する |
| `AndroidExtraFiles` | Android 生成ルートへのファイル配置 | `ProjectFiles` と module/source set の参照 |
| `IosPbxprojOps` | メイン target の source/resource/build setting/framework 操作 | Apple target の source/resource/settings/dependencies。PBX object の編集操作そのものは新モデルに持ち込まない |
| `AppIcon` | アイコン画像・metadata・参照 | 機能が所有するファイル、選択したメイン成果物の resource と metadata |

plist・Manifest・Gradle の設定 helper は native 設定を知っている利用者向けの低層の入口である。AppIcon のような機能 plugin は、利用者の意図を受けて必要なファイルと設定をまとめて作る。両者に別の合成 engine は必要ない。

`IosPbxprojOps` と raw Gradle DSL は既存の native 形式に近い入口である。特に raw DSL を解析せずに構造化フィールドへ無損失変換できるとは扱わない。新モデルの表現力と、既存 helper の互換性は別の契約になる。

`AppIcon` は既存の Android application attributes plugin より先に実行する制約を持つ。ユーザーの属性指定が後から適用されるという意味であり、登録一覧の位置だけでは保証されない。

外部の [whisker-asset plugin](../packages/whisker-asset/src/plugin.rs) も機能側に属する。組み込みregistryには含まれず、project protocolで発見・実行する。`project_plugin::<WhiskerAsset>` が既存の `dir` / `file` 設定を受け取り、iOS/macOSではapplication targetへのresource folder、Androidではapplication moduleのmain assets、Webでは配布resourceとURL prefixのmetadataを `Merge` で宣言する。Windowsでは実行ファイル横、Linuxではshare/<executable名>以下のresourceを宣言する。コピーはrenderer／builderの責務である。

## 基盤構造とテンプレート

| プラットフォーム | 現在の基盤構造 | 新モデルで構造を表す場所 |
|---|---|---|
| iOS | application pluginが宣言するメイン app target、AppDelegate、launch screen、標準 plist、scheme、Rust build phase、SwiftPM aggregator 接続 | Apple target graph、source/resource、plist、configuration、scheme、build phase、package/product 参照 |
| Android | application pluginが宣言するapp module、MainActivity、起動 Manifest、theme、settings/root/app の Gradle 設定、wrapper、Rust Gradle 接続 | module graph、source sets、Manifest XML、scope 別 Gradle 設定、staged files |
| macOS | Cargo Host、Info.plist、entitlements、Resources とアイコンの配置 | Apple executable/bundle、metadata、resources、files。Xcode project を必須にしない |
| Web | application pluginが宣言するCargo/Wasm Host、index の mount/bootstrap、出力名と base path、favicon | document、generated outputs、head/body、URL、files |
| Windows | Cargo Host、Win32 manifest、VERSIONINFO、ICOと配布plan | executable、resources、files |
| Linux | Cargo Host、Desktop Entry、iconとinstallation plan | executable、desktop_entries、resources、files |

実装の入口は [iOS](../crates/whisker-cng/src/ios.rs)、[SwiftPM integration](../crates/whisker-cng/src/ios_modules.rs)、[Android](../crates/whisker-cng/src/android.rs)、[macOS](../crates/whisker-cng/src/macos.rs)、[Web](../crates/whisker-cng/src/web.rs)。現在の生成物はこれらと [templates](../crates/whisker-cng/src/templates) の組み合わせで決まる。全6OSのrendererが最終IRから出力する。Windows/Linuxの入口は [desktop](../crates/whisker-cng/src/desktop.rs)。

新モデルではメイン成果物を明示し、追加 target/module を独立して定義できる。アプリ名やメイン bundle ID、orientation などを全 target に一律適用する契約ではない。Share Extension を表す場合、Extension の ID・plist・Swift source・メインアプリへの embed はその機能の寄与になる。App Group のように複数 target に必要な設定は、対象ごとに宣言する。

ファイルの staging とビルド対象への所属も独立する。Swift ファイルを `ProjectFiles` に追加するだけではコンパイル対象にならず、Extension target の source として参照する必要がある。この境界はファイル内容の所有者と、成果物に組み込む所有者を明確にする。[Apple IR](cng-apple-ir.md)、[ファイル契約](../crates/whisker-plugin/src/project/files.rs)

## 合成と subprocess の境界

旧 mobile engine の競合確認は手動の `MutationJournal` に基づく。IR 全体の変更差分やファイル所有権を再構成する処理ではない。新モデルの `merge_from` は同値の再宣言・識別子の競合を扱い、失敗時に receiver を変更しないが、現在の journal 検査を置き換えるものではない。struct を直接変更する経路も存在する。

旧 mobile protocol の subprocess plugin は context 全体を受け取り、返した context 全体で engine の値を置き換える。この旧 protocol には version/capability negotiation がなく、古い plugin に未知フィールドの保持を期待できない。新しい `ProjectEngine` は上記の versioned protocol と別の plugin 契約を使い、この経路には新 IR を追加しない。公開契約は [PluginRequest / PluginResponse](../crates/whisker-plugin/src/lib.rs)、実行処理は [compose](../crates/whisker-cng/src/compose.rs) にある。
