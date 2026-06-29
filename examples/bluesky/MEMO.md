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
