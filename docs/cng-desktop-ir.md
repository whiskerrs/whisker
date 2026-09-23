# Windows・Linux の宣言的プロジェクトモデル

`whisker_plugin::project` の Windows/Linux モデルは、実行ファイル、OS metadata、配布先、package 宣言を分ける。[型定義](../crates/whisker-plugin/src/project/desktop.rs) を空IR → application plugin → 機能plugin → renderer → Cargo build／配布物の順で利用する。macOS は [Apple モデル](cng-apple-ir.md) を共有する。[全体の位置づけ](cng-design.md)

## Windows

Windows executable は Cargo binary または staged executable と、その配布 path を持つ。Win32 manifest、icon、VERSIONINFO、追加 `.rc` は executable ごとの metadata である。Win32 manifest の任意 XML により UAC、DPI、互換性、activation 等を保持できる。宣言した XML と実際の PE image への埋め込みは別であり、prebuilt executable の変更可否も backend が確認する。[Application manifests](https://learn.microsoft.com/en-us/windows/win32/sbscs/application-manifests)

VERSIONINFO は数値4成分の file/product version、任意の flags mask/flags/OS/type/subtype、language-codepage ごとの文字列を持つ。文字列表の ID は8桁の hex とし、同じ ID の大文字小文字違いを拒否する。renderer はこれらの ID から VarFileInfo Translation を生成する。追加 `.rc` と標準 icon/manifest/version resource の ID 衝突は native resource compiler が確認する。[VERSIONINFO](https://learn.microsoft.com/en-us/windows/win32/menurc/versioninfo-resource)

`packages` は MSIX package を ID ごとに持つ。`application_package` が指定された場合、その package には主 executable が必要である。補助の resource/optional/framework package にはこの制約を適用しない。それぞれの package は executable ID と resources を明示し、unpackaged distribution の resources は暗黙に継承しない。[MSIX package formats](https://learn.microsoft.com/en-us/windows/msix/package/app-package-formats)

各 MSIX は完全な native XML を保持し、`AppxManifest.xml` の出力 path を予約する。package kind、capability、extension、Identity、native schema の version 等の正当性は native package tooling が判定する。IR に同じ tag があるだけで、resource package や Store 提出として有効だと保証することはない。[Package schema](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-f-package)

Windows の staging/distribution/package path は、ASCII case-fold 後の同一 path・親子 path の衝突、代表的な Win32 禁止文字・予約 device 名・末尾の空白/`.` を検証する。完全な Unicode filesystem equivalence や symlink 解決は扱わない。実際の filesystem と native packaging の検証は引き続き backend が担う。[Windows file naming](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file)

## Linux

Linux は installation prefix 相対の executable/resources、Desktop Entry、Shared MIME-info XML、AppStream XML、D-Bus service を持つ。icon、locale、systemd unit、portal 定義、policy file 等は任意 resources でも配置できる。ファイル種類ごとに常に専用 Rust 型を追加する必要はない。

Desktop Entry は主 group と action group を分け、値を logical scalar/list/bool/number として保持する。文字列は desktop-file escape 前の内容であり、list 内の literal `;` と list delimiter を区別できる。`Exec` は native command/field-code syntax を保持し、shell command へ変換しない。locale suffix を含むキーを利用できる。[Desktop Entry specification](https://specifications.freedesktop.org/desktop-entry/latest-single/)

`Actions` は ordered list とし、参照する action group が存在するか検証する。一般の list は全体が同じ場合だけ合成できる。`append_list` を明示的に呼ぶと、既存順を保った重複のない追加ができる。既に scalar があるキーへの list 追加は拒否する。Type=Application だけに限定せず、Link/Directory 等の native 表現は保持する。

D-Bus service は native key/value を保持する。session/system bus の違いによる User 要件や service filename の規則を、一律の制約として適用しない。MIME/AppStream は XML root の基本確認だけを行い、namespace/schema や database 更新は native backend に委ねる。[D-Bus activation](https://dbus.freedesktop.org/doc/dbus-specification.html#message-bus-starting-services)、[Shared MIME-info](https://specifications.freedesktop.org/shared-mime-info-spec/latest/)

Linux の出力 path は大文字小文字を区別して検証し、Windows の規則を流用しない。

## Flatpak とその他の package

Flatpak は runtime/version/SDK、launch command、finish-args、ordered modules、追加 native root properties を持つ。module は native object または staged JSON/YAML fragment である。inline module は name、外部 fragment は path を合成時の identity とする。[flatpak-builder reference](https://docs.flatpak.org/en/latest/flatpak-builder-command-reference.html)

Command は既存 executable ID の参照か、native module がインストールする wrapper の相対コマンド名を区別する。すべての native build output を偽の Cargo executable 宣言へ変換する必要はない。native command の存在と実行可否は Flatpak backend が確認する。

`properties` は build-options、sdk-extensions、base、cleanup、extensions 等の native JSON を保持する。`id`/`app-id`、runtime、runtime-version、sdk、command、finish-args、modules は型付きフィールドが所有し、properties への二重指定を拒否する。任意 property の native schema までは解釈しない。runtime/version/SDK はこのモデルでは明示的に要求する。

finish-args は順序と意味を持つ native option であり、両側に指定がある場合は同じ配列を要求する。異なる permission policy を自動で混ぜない。module は既存順を保って追加し、同じ name/path の異なる定義を拒否する。deb/rpm/AppImage 等は format と staged recipe を保持し、recipe の内部を構造的に合成するとは主張しない。

## 合成・XML 編集・検証

Windows/Linux の `merge_from` は原子的であり、最後の項目で競合しても先行した変更を残さない。executable/package は ID、resource と metadata は destination、localized value は locale/codepage と key で識別する。同値の再宣言は許可し、scalar・opaque file・異なる native XML 全体の競合は拒否する。Flatpak の追加 JSON object は再帰的に合成し、配列の内容を勝手に統合しない。[合成実装](../crates/whisker-plugin/src/project/desktop/compose.rs)

`XmlElement::edit` は qualified name と identity 属性の path を用い、Upsert、属性の Set/Override/Remove、要素削除、明示的な子ノード置換を行う。複数一致や identity の変更は拒否し、失敗時は元の tree を保持する。子の追加順を保ち、native schema のための並べ替えや namespace 正規化は行わない。たとえば AppStream の text を変更するときは `ReplaceChildren` を明示する。[XML 編集 API](../crates/whisker-plugin/src/project/xml/edit.rs)

検証は参照、宣言された path の衝突、XML root、Desktop Actions、Flatpak の typed key と module identity 等を扱う。file existence、native schemas、署名、install/uninstall、MIME/icon database 更新、Windows registry、runtime activation、package 公開は実行しない。[validator](../crates/whisker-plugin/src/project/desktop/validate.rs)、[テスト](../crates/whisker-plugin/src/project/desktop/tests.rs)


## 標準生成・buildへの接続

`windows` と `linux` を生成先へ追加した。引数なしのCNGは6プラットフォームを生成する。`desktop` は従来どおり `macos` の別名を維持する。[application plugin](../crates/whisker-cng/src/desktop/application.rs) は標準Cargo Hostのmanifest／entry point、背景色、OS metadata、AppIcon、Cargoが選択したmoduleの登録を宣言する。共有 `name`／`bundle_id`／`version`／`build_number` を使用し、Windows versionの各成分・build numberにはu16の範囲を要求する。iconは256px以上の正方形PNGからWindowsのICO、Linuxのhicolor PNGを生成する。

Windows/Linuxの空application IDとLinuxの空app IDは、初期化途中の未指定値として `Merge` で埋める。確定済みの異なるIDは競合し、最終検証ではIDとapplication executableが必要になる。この合成契約の追加でproject protocolのIR schemaは **6** になった。protocol envelopeは1のままである。

[renderer](../crates/whisker-cng/src/desktop/project_render.rs) は staged files と `.whisker/desktop-build.json`（version 1）を生成する。planにはOS、Cargo target／selection、application、executables、配布destination→staged sourceを保存する。アプリのfeaturesは生成Cargo manifestのアプリ依存へ適用し、各 `RustBuild.features/default_features` はそのHost binaryのビルドへ適用する。機能pluginのdiscoveryも同じOS／feature選択を使う。stagingのみのファイルは配布されない。

Windowsではmanifest／icon／VERSIONINFO／追加RCを `.rc` と `build.rs` に下ろし、[embed-resource 3](https://docs.rs/embed-resource) の `compile_for` で指定binのリンクに渡す。日本語等のVERSIONINFOはUTF-16のwide stringとして出力する。生成packageには `embed-resource` build dependencyが入る。独自 `build.rs` との合成、root以外のCargo manifestへの埋め込み、prebuilt PEへのmetadata追加はエラーにする。MSVC用RC／linker、またはGNU用windres／linkerのセットアップは別途必要である。

LinuxではDesktop Entry／action group／型付き値、MIME／AppStream XML、D-Bus serviceを配布planへ登録する。Desktop Entryの文字列とlistは仕様のescapeを適用し、Execのnative quoting／field codeは保持する。D-Bus値はDesktop Entryのescapeへ変換せず、改行・制御文字等の未対応値を拒否する。これはinstallation prefix相対のツリーの作成までであり、実際のインストール、Desktop EntryのPATH解決、icon／MIME database更新は行わない。

```sh
whisker build windows --manifest-path examples/news/Cargo.toml --cargo-target x86_64-pc-windows-msvc
whisker build linux --manifest-path examples/news/Cargo.toml --cargo-target x86_64-unknown-linux-gnu
```

[builder](../crates/whisker-build/src/desktop.rs) はCargo executableとprebuilt helperを集め、`target/.whisker/<platform>/<app-package>/dist/release/` に配布物を生成する。必要なRust target・native linker／sysrootを用意すれば別OS上からも呼べる。配布先の衝突、入力のsymlink・欠落、予約名を検出し、入力／ビルド失敗で以前の配布物を削除しない。生成後のresource変更をfingerprintに含め、再生成では古いファイルを除去する。最終ディレクトリの公開は旧ディレクトリを置換するため、完全なatomic transactionではない。

`whisker-asset` はWindowsの実行ファイル横の `whisker_assets/`、Linuxの `share/<executable名>/whisker_assets/` を宣言する。Linuxのapplication destinationは直接 `bin/` 配下である必要がある。runtimeはcurrent_exeから配置を求め、起動時のcwdに依存しない。明示 `set_base` は優先される。

現時点でMSIX、Flatpak／deb／rpm等のpackage宣言は標準backendがエラーで拒否する。署名・Store配布、ネイティブ依存DLL／共有ライブラリの自動収集、Windows/Linuxの `whisker run desktop`／Hot Reload接続は未対応。CLI本体には既存のUnix依存TUIが残るため、Windows上のCLI全体の対応完了を意味しない。生成CargoプロジェクトとCNG／build経路の検証範囲を、Host GUIの実機検証と区別する。

検証は [renderer tests](../crates/whisker-cng/tests/desktop_project.rs)、[builder tests](../crates/whisker-build/src/desktop.rs)、[asset tests](../packages/whisker-asset/tests/plugin_e2e.rs)、[全6OSの実discovery／feature切替テスト](../crates/whisker-cng/tests/generation.rs) に置く。外部resource compiler／Windows linkerを使うテストは通常のテストから分離したignored testである。
