# Web の宣言的プロジェクトモデル

`whisker_plugin::project::WebProjectIr` は静的 document、Wasm build、配布ファイル、PWA metadata、配信時の HTTP header を保持する。[型定義](../crates/whisker-plugin/src/project/web.rs) は標準生成とbuildコマンドへ接続されている。HTTP headerの適用は未対応で、宣言された場合は生成時にエラーにする。[全体の位置づけ](cng-design.md)

## 出力先と URL

`document` は HTML の出力 path、`generated_artifacts` は backend-defined role ごとの JS/Wasm 等の出力 path を持つ。resource、生成 Manifest と併せて、同じファイルへの書き込みや directory とその子の重複を拒否する。generated artifact の role と実際の出力処理の対応は backend の契約であり、未知の role を無視してよいという意味ではない。

`ProjectFiles` の staging と、最終配布先への `resources` は別である。staging しただけの Service Worker が配布物に入るとは判断しない。worker script は generated artifact か、配布 resource のファイル／directory 以下として宣言されている必要がある。directory 内の実ファイルの存在は backend が確認する。

`base_path` は CNG の配布 URL prefix であり、HTML の `<base>` 要素とは別である。モデルは `/`・`/my-app/` のような origin-relative directory を採用し、unreserved ASCII segment のみを許す。query、fragment、authority、dot segment、percent escape を含む base prefix はこの型の範囲外である。`output_url` は配布ファイル名の UTF-8 byte を percent-encode するため、名前に含まれる空白・`#`・`?`・`%` が URL の区切りに変化しない。[HTML URL resolution](https://html.spec.whatwg.org/multipage/urls-and-fetching.html#resolving-urls)

Manifest の `start_url`・icon URL や HTML 属性の URL は native URL reference のまま保持する。すべてを自動的に deployment prefix 相対へ変換することはない。それぞれの解決規則は renderer とブラウザーが扱う。[HTML base](https://html.spec.whatwg.org/multipage/semantics.html#the-base-element)、[Web App Manifest](https://www.w3.org/TR/appmanifest/)

## 静的 HTML

title/lang はモデル自身が所有する。追加の html 属性、head、applicationが宣言する `body` と、その前後の `body_before` / `body_after` を保持できる。head/body は安定した ID 付きの element 配列で、異なる ID の同じタグをまとめず、宣言順を保つ。runtime DOM と Whisker の画面ロジックはこのモデルに含めない。

属性は boolean attribute を区別し、子ノードは element/text の順序を保持する。検証は tag/attribute 名の基本形式、case-colliding 属性、void element の子、script/style 内の element、raw-text の閉じタグ列を確認する。複数 title、複数 base、body や入れ子への base 配置も拒否する。これは HTML 全体の content model 検証ではない。[HTML syntax](https://html.spec.whatwg.org/multipage/syntax.html)、[script content](https://html.spec.whatwg.org/multipage/scripting.html#restrictions-for-contents-of-script-elements)

Text は論理内容であり、escaping は renderer の責務である。script/style を通常の HTML text と同じように escape してよいわけではない。inline JS/CSS の意味を変更せず安全に serialize できない入力は、renderer が拒否するか外部ファイルを使う。

## Web Manifest と Service Worker

Web Manifest は任意の native JSON properties を保持する。icons、shortcuts、share_target 等や将来の拡張キーを部分的な Rust enum に閉じ込めない。`path` と `crossorigin` は生成する manifest link の契約でもあり、Manifest がある場合に別の `rel=manifest` を head へ追加することは拒否する。ブラウザーの installability 条件を IR が勝手に必須化することはない。[Manifest link](https://html.spec.whatwg.org/multipage/links.html#link-type-manifest)

Service Worker は安定した ID ごとに複数登録を宣言できる。script は配布 path、kind は classic/module、`update_via_cache` は imports/all/none を保持する。scope を省略した場合は script directory を使用し、明示する場合は origin-relative directory URL とする。同じ scope の二重登録は構造エラーである。[Service Workers specification](https://www.w3.org/TR/service-workers/)

Scope が script directory より広くても一律に拒否しない。`Service-Worker-Allowed` header が必要になる場合があるため、配信環境で判断する。secure context、script MIME、実際のレスポンス、push/cache/routing の処理はブラウザー・hosting・worker 実装の責務である。

## HTTP response requirements

`response_headers` は安定した ID を持つ順序付き rule で、全出力・特定ファイル・directory 以下を対象にできる。header 名は lowercase token、値は改行等の control character を含まないものとし、複数 rule が一致する場合は後の rule が同じ header を上書きする。hosting backend は未対応の宣言を明示的に拒否する契約である。

この設定は、CSP の HTTP-only directive、COOP/COEP、Service-Worker-Allowed、Wasm の content type 等を宣言するために使える。HTML meta だけでは同じ表現力にならない。[CSP meta の制限](https://www.w3.org/TR/CSP3/#meta-element)、[COOP/COEP](https://web.dev/articles/coop-coep)、[Wasm streaming](https://webassembly.github.io/spec/web-api/index.html#streaming-modules)

DNS・TLS・redirect/rewrite・SPA fallback・任意 server rule language を実行／再現するモデルではない。ファイルを生成することと HTTP server へ設定を適用することは別である。

## 合成と検証

`merge_from` は原子的に適用する。HTML と response rule は ID、resources は destination、worker は ID、generated artifacts は role で合成する。同値は一つにし、同じ identity の異なる値はエラーにする。Manifest JSON は object を再帰的に合成し、scalar/array は同値のみ許可する。array の identity や優先順位を意味解析して自動結合することはない。置換は呼び出し側が明示的に行う。[合成実装](../crates/whisker-plugin/src/project/web/compose.rs)

構造検証は出力先、URL prefix、静的 HTML の一部、worker の宣言上の配布・scope、HTTP header の基本形式を扱う。HTML/Manifest schema の完全検証、JavaScript の実行、HTTP の実配信、ブラウザーごとの対応は含まない。[validator](../crates/whisker-plugin/src/project/web/validate.rs)、[テスト](../crates/whisker-plugin/src/project/web/tests.rs)

## 標準生成と build への接続

Webも空の `WebProjectIr` から始め、[application plugin](../crates/whisker-cng/src/web/application.rs) を先頭にした `ProjectEngine` で合成する。`wasm` と `document` の `None`、title/lang/base_pathの空文字は合成途中の未指定値であり、`Merge` で埋める。最終検証ではwasm/documentと有効なbase_pathが必要になる。異なる宣言済みの値は競合とし、上書きは理由付き `Replace` で行う。`body` の追加と未指定値の合成規則を含むIR schemaはversion 6である。

`whisker-web-application` は `WebInputs` から、Cargo Hostのmanifest/source、Wasm build、HTMLの既定metadata・背景・favicon・mount・起動scriptを宣言する。rendererがアプリの設定を後から再適用することはない。Hostは起動時のdocument titleを読み取るため、pluginで変更した静的titleも維持する。faviconは既存の `Config.web.favicon` を使う。

Cargo依存グラフにある `protocol = "project"` のpluginを、同じfeature選択で発見・実行する。旧mobile protocolはWeb contextを持たないため、従来どおりWebでは実行せず、そのpluginの設定も適用しない。mobile専用の組み込み設定もWebでは寄与しない。未知のproject plugin設定は拒否する。macOSもproject protocolによる合成経路を使う。[plugin合成](cng-plugin-composition.md)

[renderer](../crates/whisker-cng/src/web/project_render.rs) は構造検証後にstaged filesを読み、完成したHTML、Web Manifest、Service Worker登録script、配布計画 `.whisker/web-build.json` を生成する。stageされたファイルの内容とmodeもfingerprintに含める。出力衝突や入力の欠落は生成先を整理する前に確認する。再生成時は既存のdistを保ち、次のbuildが作り直す。

[Web builder](../crates/whisker-build/src/web.rs) は配布計画からCargo manifest・library target・feature選択を読み、wasm-bindgenの成果物と、明示的に配布対象となったファイルをdistへ配置する。`ProjectFiles` への追加だけでは公開せず、`resources` で配布先を宣言する。directory resourceは実ファイルへ展開し、子ファイルも欠落・重複を検証する。Manifestは自動的に配布対象になる。Service Workerは登録前に配布物に実在することを確認する。古い計画や計画のないgen/webは再生成が必要である。

現在のbackendの制約は次のとおり。

- build/runとHot Reloadの契約に合わせ、documentは `index.html`、generated artifactは `js = whisker_app.js` と `wasm = whisker_app_bg.wasm` の2つに限定する。別のroleやpathはエラーにする。`snippets/` はwasm-bindgen用に予約する。
- 標準generatorではgenerated package名とbase_pathをアプリ設定と一致させる。base_pathは `Config.web` で設定する。独自の変更をIRだけに行い、起動scriptやdev serverのprefixと食い違う構成は拒否する。
- `response_headers` はhosting adapterが未接続なのでエラーにする。Service Workerの広いscope、CSPやCOOP/COEP等が必要な場合、その配信設定まで自動で成立するとは扱わない。
- Service Workerはstatic resourceとして配布されたscriptを扱う。Wasm/JS生成成果物をworker用にビルドする専用経路はない。
- HTMLの通常text・属性をescapeし、script/style等のraw textは本文を保持する。閉じタグ列やscript内のHTMLコメント構文は拒否し、外部scriptへ分離する。textarea等のtext-only要素に子elementは置けない。html/head/bodyの入れ子、plaintext、noscript、SVG/MathMLの専用contextは未対応として拒否する。HTML全体のcontent modelを検証するものではない。
- 生成計画を先に検証するが、ファイル書き込み全体のtransactionではない。配信設定・DNS/TLS・デプロイ操作は行わない。

[生成と配布計画の回帰テスト](../crates/whisker-cng/tests/web_project.rs) と、[build側の配布テスト](../crates/whisker-build/src/web.rs) でこの契約を確認する。

[whisker-asset](../packages/whisker-asset/README.md) はproject protocolへ移行済みで、`AppFile` とWebの `resources` を宣言する。`project_plugin::<WhiskerAsset>` で宣言したdirectoryの相対path、または個別fileのbasenameを配布先にする。Webの `resolve()` はpluginがheadへ追加したdeployment prefixを参照するので、現在のrouteに依存しない。native側の配置先はAndroidの `assets/whisker`、iOSの `whisker_assets` を維持する。

newsのWebリリースビルドで、宣言した13ファイルの配布内容が元ファイルと一致することを確認した。ブラウザーでは一覧と記事画面への遷移後に写真の読み込みを確認している。
