# Bluesky example — 実装メモ

whisker の実地評価のための Bluesky クライアント。本家アプリに機能を近づけながら、
**whisker で素直に書ける範囲**で実装する（内部コードのハック・過度な拡張はしない）。
このメモは「実装済み機能」「スキップした機能と理由」「開発者体験(DX)の気づき」を記録する。

## 実装済み機能

- OAuth ログイン（atrium-oauth、埋め込み WebView、loopback redirect）
- セッション永続化（`whisker-secure-store` = iOS Keychain / Android Tink+Keystore に DPoP セッションを保存、起動時に復元）
- Following タイムライン表示（`getTimeline`、Lynx `<list>` 仮想化）
- 投稿カード（アバター・著者・本文・エンゲージメント数、lucide アイコン）
- 全画面 safe-area 対応、ハンドル入力の hygiene（auto_capitalize/autocorrect/spell_check off）
- （Phase 0〜）タブナビゲーション …（以降フェーズごとに追記）
- 検索（Phase 4）: `searchActors`（ユーザー）/`searchPosts`（投稿）をセグメント切替で表示。
  検索フィールド + タブを固定ヘッダー、結果は仮想化 `list`。ユーザー行タップ→プロフィール遷移。

## スキップした機能と理由

| 機能 | 理由 |
|---|---|
| プッシュ通知 | ユーザー指示でスキップ。whisker に push 通知モジュールが無い（APNs/FCM 連携が要る）。 |
| 画像・動画の投稿 | whisker に画像ピッカー / カメラ / フォトライブラリのネイティブモジュールが無い（`whisker-image`/`whisker-video` は**表示**専用）。ピッカー無しでは投稿フローが組めない。 |

（フェーズを進める中でスキップ判断したものを随時追記）

## DX の気づき（whisker 評価メモ）

実装しながら気づいた「やりづらかった点・足りない機能・不揃いな API」を記録する。

- **keep-alive な `Switch` ブランチ画面はホットパッチで再レンダリングされない＝編集の確認に
  cold rebuild が要る**（重要・DX）: タブは root 直下の keep-alive `Switch`（全ブランチを起動時に
  マウントして display トグル）。そのため Search/Notifications/Profile タブのような**起動時に
  マウント済みの画面を編集しても、tier-1 ホットパッチでは画面に反映されない**（マウント済み
  インスタンスは古いコードのまま）。home タブのように push で新規マウントされる画面や、遷移で
  入り直す画面は反映されるが、keep-alive タブ直下の画面は**アプリ再起動（＝on-disk バイナリを
  作り直す cold rebuild）が必要**。`whisker run` は tier-1 が通る限り cold rebuild しないので、
  確認したいときは「アプリを terminate → ソースを touch」で `no client → Tier 2` を踏ませて
  cold rebuild + 再 launch させた（simctl terminate/launch だけでは on-disk バイナリが古いままで
  パッチも再適用されない点に注意）。Search 画面の反復はこれで毎回フルビルドになり遅かった。
  → keep-alive ブランチの画面にもホットリロードの再レンダリングが届くと DX が大きく改善する。
- **テキスト入力の自動検証は「合成タップは Lynx に届かないが、ネイティブ UITextField への
  ホストキーボード入力は届く」**: serve-sim のタップでフィールドにフォーカスを当てた後、
  `osascript`（System Events keystroke / key code 36=Return）で文字列と Return を送ると、
  whisker-input（iOS は本物の `UITextField`）は普通に受け取れた。Lynx の合成 gesture が
  届きにくいのとは別で、フォーカス済みネイティブ入力欄はホストのハードウェアキーボード入力を
  受理する。検索クエリ投入の検証に使えた。
- **`Input`（whisker-input のカスタムモジュール view）は親いっぱいに stretch しない＝
  `width: 100%` を明示しないと中身幅に潰れる**: login 画面は root に `align_items: Stretch` を
  置いていたので Input が全幅になっていたが、検索画面で Input をパディング付きラッパー view に
  入れたら**幅が中身（空プレースホルダ）まで縮んだ**。Input の `style` 文字列に `width: 100%;`
  を足して解決。custom module view は flex の cross-axis stretch を受けない（もしくは intrinsic
  サイズを主張する）ようなので、明示幅が安全。

- **dev ループでの認証セッションの扱い**: `whisker run ios` の cold rebuild は upgrade install
  （アンインストールしない）なので、**Keychain のセッションは rebuild を跨いで保持される**
  （一度ログインすれば作業中は復元され続ける）。当初ログイン画面に戻ったのはセッション
  期限切れ/未ログインが原因で、再インストールが消したわけではなかった。
  - ただし**操作（タップ）の自動検証は難しい**: シミュレータへの合成タップ/キーストロークが
    Lynx の gesture に届きにくく（Brave が最前面を保持する等の環境要因もあり）、いいね・遷移
    などの対話はスクリーンショットだけでは確認しづらい。レンダリングは目視できるが、操作系は
    実機の手動確認に頼ることになる。dev ループに「要素を tap/scroll する」テスト用フックがあると
    自動検証が大きく楽になる。
- **タブは「root レイアウト直下に `Switch`」にしないと切替が重く感じる**: 当初は
  `Stack { Route("", TabsLayout){ Switch{...} } , login, ... }` と root を `Stack` で包み、
  その index レイアウトに `Switch` をネストしていた。これだとタブ切替にアニメーションが付いて
  見えた。whisker-router の example どおり **root を `Route(component: TabsLayout){ Switch{...} }`**
  （Stack で包まない）にすると、タブ切替は `Switch` の display トグル（瞬時）、タブ内 push は
  `Stack` のアニメ、と期待通りに分離できた。login/compose は Switch のブランチ / タブ内 Stack に配置。
  認証状態は keep-alive でもログイン直後にタイムラインが再取得されるよう、共有 `RwSignal<bool>`
  (`AuthState`) を root で `provide_context` して扱う。
  → DX: 「タブ＝瞬時、push＝アニメ」を出すための入れ子の作り方が直感的に分かりにくく、example 必須。
- **`list` は更新時に key で並べ替えない＝増分追加・先頭挿入・並べ替えで表示順が壊れる**（重要・要改善）:
  React / SolidJS / SwiftUI の keyed list は `render` が返す配列順に毎回 reconcile するので、
  「望む順で配列を返す」だけで無限スクロール（末尾追加）も pull更新（先頭挿入）も途中挿入も正しく動く。
  whisker の `list` は **更新時に既存アイテムを並べ替えず、新規 key を末尾に `append_child` するだけ**。
  そのため `each()` が返す順序と表示順が**更新をまたぐと乖離**する。実証：プロフィールでヘッダーを
  先に 1 件出し、後から feed の投稿が来ると**ヘッダーが末尾に押し出された**。エラーも出ず静かに壊れる。
  → 実用上の制約は「**リストの中身は 1 回の更新で確定させ、増分追加/並べ替えをするな**」。これは
  keyed list の常識（React 等）に反し、**無限スクロール・プル更新・途中挿入が軒並み危険**。
  ルールというより `list` の reconcile 不足。`list` が key で並べ替えるべき。
- **`list` の「背の高い不均一な先頭アイテム」はスクロール時の仮想化リサイクルで潰れる＝ヘッダーは
  list に入れず、固定の兄弟ノードにする**（重要・whisker/Lynx の根本制約）: `list` は `each`/`key`/
  `children` の 3 kwarg だけで header/section/sticky スロットは無い。当初 **行を enum（`Header | Post`）に
  して `each` の先頭に Header を置き**、一括マウント（prof+feed が揃ってから `[Header, …Posts]` を 1 diff）
  すれば順序・高さが安定する、と考えた。**が、これは「コンテンツが画面に収まる間」だけ成立する**。
  Lynx の decoupled native list は**コンテンツが画面を超えてスクロールが始まるとセルをリサイクル**し、
  その際に背の高い先頭セル（ヘッダー）を**再計測して潰す**。Rust/whisker 側は `set_update_list_info(count)`
  で**件数しか渡しておらず**（高さ設定は一切無い）、高さは各セルの実測 intrinsic。Lynx 内部の行高
  見積もりと大きく外れる先頭の巨大セルが、リサイクル時に縮められる。
  → **最終設計（採用）**: ヘッダーを list の外に出し、**固定ヘッダー（`flex-shrink: 0`）+ フィードだけの
  仮想化 `list(flex:1)`** を 2 つの独立した兄弟ノードにする。`profile_view` は
  `view(column){ Show(prof){ profile_header } Show(feed){ post_list } }`。これで list の制約を完全に回避でき、
  ヘッダーは潰れず、フィードは仮想化される（serve-sim でスクロール検証済み：ヘッダー固定・潰れない）。
  - トレードオフ：本家はスクロールでヘッダーも一緒に流れて消えるが、この設計ではヘッダーが常時上部に
    固定される（UX が本家と異なる）。`list` に正式な header/section スロット、または「先頭の不均一セルを
    リサイクル対象から外す」hook があれば本家どおり書ける（要望）。ユーザー判断で「固定ヘッダー +
    仮想化 list」を選択（潰れない・仮想化ありを優先）。
  - **追加調査（「これは whisker のバグか？」の切り分け）**:
    - **コード**: `list` ビルダー（`crates/whisker/src/lib.rs` `__h()`）は全アイテムを実体としてツリーに
      materialize し、各 `<list-item>` に **位置ベースの `item-key="w_{index}"`** を振り、`count` だけを
      `set_update_list_info` で Lynx に渡す。**`reuse-identifier`/item-type も per-item 高さも sticky/header
      スロットも露出していない**。リサイクル・レイアウトは完全に Lynx の decoupled native list 側。
      → whisker の Rust ロジックが明確に誤っている類のバグではなく、**list バインディングの機能欠落
      （不均一高さセルを安定させる制御を出していない）+ Lynx 側の仮想化の癖**、という位置づけ。
    - **再現**: serve-sim の合成ジェスチャ（ゆっくりドラッグも高速フリングも、スクロール往復も）では
      **「縦に潰れる」現象は再現できなかった**。ヘッダーを list の item 0 に戻し、intrinsic 高さ／固定高さ
      px(460) の両方で試したが、いずれもスクロール後もヘッダーは満寸で表示された。ユーザーは実機の
      手操作で潰れを観測しており、合成タップが Lynx gesture に完全には一致しない既知の制約（上記の
      別項）が再現できない一因と思われる。
    - **再現できた別異常**: item 0 にヘッダーを置くと、**ヘッダーのセル高さが中身より大きく**なり
      （カウント行と最初の投稿の間に余白が出る＝セル高 ≠ コンテンツ高）。潰れとは逆向きだが、
      「Lynx が不均一な先頭セルの高さを正しく measure できていない」ことの傍証。
    - **結論**: 「whisker 単体のバグ」と断定はできない。Lynx native list が不均一高さの先頭セルを
      正しく扱えない（過大／過小の measure）制約が主因で、whisker はそれを回避する API を露出して
      いない。固定ヘッダー分離が現状の正解。深掘りするなら Lynx fork 側の list セル sizing
      （`componentAtIndex`/`OnComponentFinished` 周辺）と、`reuse-identifier`/per-item-height の
      バインディング追加が候補。
- **`list` のセルは「クロス軸いっぱい」ではなく「コンテンツ幅」に縮む＝各アイテム root に
  `width: 100%` を明示しないと幅が不揃いになる**（重要・直感に反する）: 通常の flex/SwiftUI の
  縦リストはアイテムがクロス軸（横幅）いっぱいに stretch するが、whisker の仮想化 `<list>` は
  **各セルを中身の固有幅に shrink-wrap する**。そのため `width: 100%` を持つバナー画像は
  「セル幅（＝カウント行などの最長テキスト幅）の 100%」にしか広がらず、ヘッダーが画面幅の
  ~67% で止まって見えた。投稿行も**テキスト量ごとにセル幅が変わり**、区切り線（border-bottom）の
  右端が行ごとにバラバラになる。エラーは出ず、内容が一見揃って見えるので気づきにくい。
  → 対処：**list に入れる各アイテム（`post_card`）の root view に `width: percent(100)` をピン留め**する。
  これで投稿行も区切り線も画面幅いっぱいに揃った（serve-sim で確認）。`list` がセルをクロス軸 stretch
  すれば不要な作業。`profile_header` は list 外の固定ヘッダーになったが、列クロス軸の stretch に依存せず
  バナーを全幅化するため同様に `width: percent(100)` を付けてある。
  - 補足（hot-reload）: 既にマウント済みの仮想化リストのセルは、tier-1 hot patch でスタイルを
    変えても**再レンダリングされない**（home タイムラインは新規マウントで反映されたが、表示中の
    プロフィール list は変わらなかった）。画面を一度離れて再マウントすると反映された。
- **子コンポーネントの引数で `resource.get()` を直接読んでもリアクティブ依存が張られない**（重要）:
  `post_list(posts: feed.get().unwrap_or_default())` のように `render!` の**引数式**でリソースを読むと、
  初回 render の値（まだ None → 空）で子がマウントされ、**後から feed が解決しても子が再 render されず空のまま**
  だった（feed が初回 render に間に合った時だけ表示される、という不安定挙動）。home のタイムラインのように
  **`Show(when: move || feed.get().is_some()){ post_list(posts: feed.get()…) }` でゲート**すると、Show の
  `when` クロージャがリアクティブに feed を購読し、解決後にフルデータで1回マウントされて安定。
  → DX: 「`get()` を読めば依存が張られる」と思いがちだが、**リアクティブに追跡されるのは
  クロージャ（`Show`/`computed`/`effect`）の中で読んだ時だけ**。引数式での `get()` はスナップショット。
  非同期データを子に渡すときは `Show`/`computed` で包む必要があり、直感に反する。
- **同じ `Show` children 内で複数リソースを読むと、片方の更新が他方を巻き込んで再レンダリング**:
  プロフィール画面で `Show(when: prof.is_some()){ profile_header(prof.get()…) post_list(feed.get()…) }`
  と書いたら、**`feed` が解決した瞬間にヘッダーが空白化**した（最初は表示され、フィード到着で消える）。
  children ブロックは reactive クロージャで、中で `feed.get()` を読むと feed 更新時に children 全体
  （= profile_header も）が再評価され、その再評価フレームで一瞬デフォルト値になり潰れた。
  **post_list を `Show` の外の独立した兄弟ノードに出す**（ヘッダーは prof だけの `Show`、リストは feed だけ）
  と fine-grained に分離でき解決。併せて、仮想化 `<list>`（intrinsic 高が巨大）が隣の可変高ヘッダーを
  flex で潰さないよう、ヘッダーに `flex_shrink: 0` をピン留め。
  → DX: 「どの reactive read がどのノードの再レンダリングを誘発するか」が直感的に見えにくい。
  複数の非同期状態を1画面で混ぜるときは描画ノードを分けるのが安全。
- **`#[component]` の `Option<T>` 引数は「省略可能 prop」に特別扱いされる**: `following_uri: Option<String>`
  のような引数を定義すると、生成される builder の setter が `impl Into<T>`（= 内側の `String`）を取り
  省略時 `None`、という挙動になる。そのため**呼び出し側で `Option<String>` をそのまま渡せない**
  （`From<Option<String>>` for `String` が無い、というエラーになる）。回避として `following_uri: String`
  （空文字＝なし）の sentinel 方式にした。任意の `Option` 値を子に渡したいケースでは直感に反する。
  → 「省略可能 prop」と「Option 値を渡す prop」を区別できる仕組み（例: `#[prop(optional)]` 明示 vs
  生の型）があると分かりやすい。
- **小さいタップターゲットの自動検証**: 合成タップは「down→待機→up（`cliclick dd: w: du:`）」のホールド形なら
  FAB（画面上 ~16px）まで効くが、投稿ボタンのような極薄要素（~10px）は不安定。シミュレータ窓が
  349px 固定で全 UI が 0.29 倍に縮むのが主因（窓を任意サイズに広げられない）。
- **tier-1 hot-patch は新規クレート依存を拾えない**: `urlencoding` を新たに足した変更は
  tier-1 patch が `unlinked crate` で失敗し、tier-2 cold rebuild にフォールバックした（想定内
  だが、依存追加を伴う反復は毎回フルビルドになる点は DX メモとして記録）。
- **at:// URI をルートパラメータに乗せにくい**（見込み）: ポスト URI は `at://did/coll/rkey` と
  スラッシュを含むため `Route("post/:uri")` に素直に入らない。percent-encode が要りそう。
  実装時に確認して追記する。
