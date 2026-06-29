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

## スキップした機能と理由

| 機能 | 理由 |
|---|---|
| プッシュ通知 | ユーザー指示でスキップ。whisker に push 通知モジュールが無い（APNs/FCM 連携が要る）。 |
| 画像・動画の投稿 | whisker に画像ピッカー / カメラ / フォトライブラリのネイティブモジュールが無い（`whisker-image`/`whisker-video` は**表示**専用）。ピッカー無しでは投稿フローが組めない。 |

（フェーズを進める中でスキップ判断したものを随時追記）

## DX の気づき（whisker 評価メモ）

実装しながら気づいた「やりづらかった点・足りない機能・不揃いな API」を記録する。

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
- **仮想化 `list` にヘッダースロットが無い → ヘッダーは「混在アイテム」で先頭に入れる**:
  長いスクロールは必ず仮想化 `list` を使う方針（`scroll_view`+`ForEach` だと全マウントで重い）。
  ただし whisker の `list` は `each`/`key`/`children` の 3 kwarg だけで **body もヘッダー item も sticky も
  取らない**（list-item は内部生成）。そこで **行を enum（`Header(Profile,…) | Post(FeedPost)`）にして
  `each` の先頭に Header を 1 件積む**ことで、ヘッダーも仮想化リストの一部としてスクロールさせた。
  Header は固定 key（`"::header"`）にしておくと、feed 更新時に新規 Post だけが差分追加され、
  既にマウント済みのヘッダーは再描画/空白化しない（先の「同一 Show children 問題」もこれで回避）。
  異なる要素構造のアイテムを 1 つの `list` に混ぜても decoupled-native list は per-item subtree なので問題なし。
  - **ただし `list` にアイテムを増分追加すると並び順が壊れる**（重要）: ヘッダーを先に 1 件入れ、
    後から feed の投稿を append すると、**ヘッダーが末尾に押し出される**（新規アイテムは
    `append_child` で後から入るが、レイアウトが各() の index 順／item-key 順を保たない）。
    回避：**prof と feed の両方が settle してから `list` を 1 回の diff で `[header, …posts]` と
    一括マウント**する（ホームのタイムラインが全件一括で正しく並ぶのと同じ）。`Show(when: 両方settled)`
    でラップし、それまではローディング表示。増分（count 0→1→N）を避けるのがポイント。
  → DX: 「ヘッダー付きの長いリスト」は頻出。`list` に header/footer/sticky スロットがあれば enum 化＋
  一括マウント調整なしで書ける。増分追加時の並び順保証も欲しい。
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
