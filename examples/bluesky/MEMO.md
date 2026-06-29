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

- **iOS シミュレータは再インストールで Keychain を消す**: `whisker run ios` の cold rebuild は
  アプリを再インストールするため、`whisker-secure-store`（Keychain）に保存したセッションが
  毎回消え、起動するとログイン画面に戻る。永続化自体は正しく動く（同一インストールでの
  kill→再起動では復元される）。ただし**開発ループ中に認証必須の画面を検証するのが難しい**:
  - 回避策: 一度ログインしたら、以降は **hot-reload（tier-1 patch、再インストールしない）**で
    反復すればセッションは保持される。cold rebuild（ネイティブ変更時など）のたびに再ログインが要る。
  - whisker 側の改善余地: dev ループで「再インストールしてもアプリのデータ/Keychain を保持」する
    オプションがあると、認証系アプリの開発体験が大きく改善する。
- **at:// URI をルートパラメータに乗せにくい**（見込み）: ポスト URI は `at://did/coll/rkey` と
  スラッシュを含むため `Route("post/:uri")` に素直に入らない。percent-encode が要りそう。
  実装時に確認して追記する。
