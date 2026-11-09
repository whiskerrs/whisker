# Whisker 公式 Agent Skills 設計案

ステータス: 提案（未実装）。`docs/` は「現在の設計」だけを置く場所なので、この案はここに置き、合意後に issue / PR へ移す。

## 1. 目的と前提

- 目的: Whisker アプリ・モジュールを書く開発者のコーディングエージェント（Claude Code / Codex / Cursor など）が、Whisker 固有の書き方と落とし穴に沿ってコードを書けるようにする。
- Whisker は学習データにほぼ存在しない。エージェントは放っておくと Dioxus / Leptos / React Native の書き方で推測する。skill の価値の大半は「推測するな、これを読め」と「ドキュメントに載っていない落とし穴」にある。
- 参考にした事例: nsh（`longbridge/nsh`、同じ Rust のネイティブ UI フレームワーク）を主なモデルにし、Expo（ルーター型 skill と plugin marketplace）と Supabase（「changelog を先に確認しろ」）から一部を取り入れる。

## 2. 配布方式

**本体リポジトリのルートに `skills/` を置き、配布経路を2つ用意する。**

```
skills/
  whisker/
    SKILL.md
    references/...
  whisker-module-authoring/
    SKILL.md
    references/...
.claude-plugin/
  marketplace.json      # plugin "whisker" → skills: ["./skills/whisker", "./skills/whisker-module-authoring"]
```

| 経路 | コマンド | 対象 |
|---|---|---|
| skills CLI | `npx skills add whiskerrs/whisker` | エージェントを問わない（約80種類） |
| Claude Code plugin | `/plugin marketplace add whiskerrs/whisker` → `/plugin install whisker` | Claude Code ユーザー。更新が楽 |

- skills CLI は `.claude-plugin/marketplace.json` に書かれた skill も見つけるので、同じフォルダを2つの経路で共有できる。
- 別リポジトリ（`whiskerrs/skills`）にはしない。Next.js が本体へ戻した理由と同じく、API の変更と skill の更新を同じ PR で揃えるため。
- README に「Skills for AI coding agents」の節を追加する（nsh と同じ形）。

### 既存の `.agents/skills/release-whisker` との衝突

skills CLI は `skills/` のほかに `.agents/skills/` も探す（[vercel-labs/skills の creating-skills.md](https://github.com/vercel-labs/skills/blob/main/docs/creating-skills.md)）。対策をしないと、アプリ開発者が `npx skills add whiskerrs/whisker` を実行したときにリリース手順の skill まで入ってしまう。

→ `release-whisker` の frontmatter に `metadata: { internal: true }` を付ける。CLI の通常の一覧には出なくなり、コントリビューターは今のまま `.agents/skills/` から使える。

## 3. skill の構成（2つに絞る）

Expo のように20個以上に分けず、nsh のように **入口の skill と references** の形にする。分けるのは「読み手と作業の種類が違う」場合だけにする。

### 3.1 `whisker`（アプリ開発全般・ルーター）

**description 案**（発火条件と除外条件を明記する）:

> Build, run, debug and ship Whisker apps (Rust UI on native iOS/Android views). Use when the project has a `whisker.rs` at the crate root, a `whisker` dependency in `Cargo.toml`, a `#[whisker::main]` function, or `render!` / `css!` / `#[component]` from whisker; or when the user asks to create a Whisker app. Do NOT use for Dioxus, Leptos, Tauri or plain Lynx projects even though the syntax looks similar.

**SKILL.md 本文（150行程度を目安に）**:

1. **最初にやること**
   - `Cargo.toml` / `Cargo.lock` の `whisker` のバージョンを確認する。skill はリポジトリの HEAD から入るので、利用者のバージョンと食い違うことがある。古いバージョンなら、該当 API の Rustdoc（docs.rs のそのバージョン）を優先させる。
   - 「Dioxus や React の知識や学習データから API を推測しない。references と docs.rs を読む」と明記する。
2. **核心のルール（数行）**: component の本体はいつ走るか、signal を読んだ binding だけが更新されること、render 時に解決して closure に move すること、など。どの作業でも前提になるものだけを書く。
3. **作業 → 読むページの表**（ルーター）

   | やりたいこと | 読む |
   |---|---|
   | 画面・component を書く | `references/components.md` |
   | 状態・非同期・context | `references/reactivity.md` |
   | スタイル・レイアウト | `references/styling-and-layout.md` |
   | リスト・条件分岐 | `references/lists-and-control-flow.md` |
   | 画面遷移 | `references/routing.md` |
   | 既存モジュールを使う（image, input, keyboard ...） | `references/using-modules.md` |
   | 実行・hot reload・ビルド・ストア提出 | `references/tooling.md` |
   | アプリ全体の見本 | `references/recipe.md` |
   | モジュールを自作する | `whisker-module-authoring` skill |

4. **検証ループ**（実際に実行できるものだけ書く）
   - `cargo check` → `whisker fmt` → `whisker run ios|android` でシミュレーター上で確認する。
   - Whisker にはまだヘッドレスでアプリをテストする仕組みがない（nsh の `nsh test` に当たるもの）。そのため「画面の確認はシミュレーターで行い、確認できなかったことは報告する」と書く。ヘッドレステストができたら、ここを書き換える。
5. **hot reload の制約**: どの変更が hot patch で反映され、どの変更がフル再起動を必要とするか（Props の型、static、root での `provide_context` など）。エージェントが「反映されない」原因を誤って調べ回るのを防ぐ。

**references の書き方のルール**:

- 各ページは「正しい書き方（コンパイルが通るコード）」→「よくある間違い（エラーと直し方）」の順に書く。
- 概念の説明は書かずに whisker.rs/docs へリンクする。skill に置くのは、エージェントが手を動かすのに必要な最小限の内容と落とし穴だけにする。

### 3.2 `whisker-module-authoring`（モジュールの自作）

- 対象: `whisker new-module`、Rust / Swift / Kotlin の3言語、`[package.metadata.whisker.module.platforms]`、イベント送信、AsyncFunction、view-bound module。
- アプリ開発とは触るファイルも言語も違うので、別の skill にする。情報源は `docs/module-api-design.md` と website の authoring-a-module / authoring-a-plugin。
- description: 「`whisker new-module` を使うとき、または `Package.swift` / `build.gradle.kts` を持つ whisker module crate を編集するとき」。

### 入れないもの

- リリース作業、Lynx fork、SPM のピン合わせ、hot reload の内部実装など、コントリビューター向けの内容 → これまでどおり `docs/` と `.agents/skills/` に置く。

## 4. 情報源と同期

### 4.1 ドキュメントの正本の問題（nsh との違い）

nsh はサイトのドキュメントと `skills/` が同じリポジトリにあるので、「ガイドを1文字も違わずコピーし、テストで一致を確認する」ことができる。Whisker はユーザー向けのドキュメントが別リポジトリ（`whiskerrs/website`、44ページ）にあるので、同じやり方はそのままでは使えない。

| 案 | 内容 | 評価 |
|---|---|---|
| **A（推奨）** | references は skill 用に独自に書く。概念は whisker.rs へのリンクで済ませる | skill の価値は落とし穴とレシピなので、サイトとの重複は小さい。同期の仕組みが要らない |
| B | whisker の CI で website リポジトリを取得し、コピーとの差分を検査する | リポジトリをまたぐ CI は壊れやすい |
| C | ユーザー向けドキュメントを本体リポジトリへ移す | 効果は最大だが、skill の範囲を超える判断になる |

### 4.2 落とし穴の採用基準

これまでの開発で見つかった落とし穴のうち、次の3つを**すべて**満たすものだけを references に入れる。

1. アプリ・モジュールの作者がふつうに書いていて踏むもの（内部実装の話ではない）
2. 現在の main で再現するもの（修正済みのものは入れない）
3. 回避策が公開 API で書けるもの

修正済みになったら references から消す。採用前に、その落とし穴がまだ再現するかを1件ずつ確認する。

### 4.3 xtask で自動チェックする

`xtask/src/doc_links.rs` と同じ場所に、次のテストを追加する。

- `skills/**/SKILL.md` と references の相対リンクが存在すること（既存の `documentation_links_name_files_that_exist` が git 管理下の `.md` をすべて検査するので、追加の作業なしでカバーされる）
- frontmatter が正しいこと（`name` がフォルダ名と一致する、`description` が1024文字以内）
- SKILL.md のルーター表が、`references/` のファイルをすべて参照していること
- `references/recipe.md` が見本の example の全ファイルと一致すること（nsh の `tsx-recipe.md` と同じ方式）。example 側は CI でビルドされるので、レシピのコードは常にコンパイルが通る

## 5. レシピ（アプリ全体の見本）

nsh は、CI でビルドとテストが通る小さな tasks アプリを丸ごと貼っている。Whisker の既存の example はどれもそのままでは向かない。

- `examples/chat`: 多くのモジュールに分かれていて大きい
- `examples/news` / `bluesky`: perf 計測や外部 API の事情を抱えている

候補:

- **(a)** 新しく `examples/tasks` を作る（推奨）: 一覧、追加、詳細画面への遷移、永続化（local-store）を200〜400行程度で書く。これをレシピの正本にする。
- (b) `whisker new` が生成するテンプレート: 小さすぎて、リストや遷移の見本にならない。

## 6. 進め方

1. **土台**: `release-whisker` に `internal: true` を付ける。`skills/whisker/SKILL.md`（ルーター、核心のルール、検証ループ）と `tooling.md` を書く。README に節を追加する。
2. **references**: 落とし穴を採用基準で選び直し、1件ずつ再現を確認してから書く。
3. **レシピと xtask のチェック**: `examples/tasks` を作り、同期テストを追加する。
4. **module skill**: `whisker-module-authoring` を書く。
5. **配布**: `.claude-plugin/marketplace.json` を追加する。skills.sh に掲載されていることを確認する。
6. **評価（軽量）**: 代表的なタスク（カウンターアプリを作る、リスト画面を追加する、画面遷移を追加する、モジュールを1つ作る）を、skill あり・なしで実行してビルドが通るかを比べる。Expo のような CI への組み込みは後で検討する。

## 7. 関連する別作業（skill の外）

- **website に `llms.txt` と各ページの `.md` 版を出す**: SKILL.md から「references で足りなければ `https://whisker.rs/docs/....md` を取得しろ」と書けるようになる（nsh の design-guides skill と同じ方式）。
- **`whisker new` が AGENTS.md / CLAUDE.md を生成する**: 生成されたプロジェクトに skill のインストール方法とバージョン情報を書いておく（Next.js 16.3 以降が AGENTS.md を自動生成するのと同じ考え方）。
- **ヘッドレスでアプリをテストする仕組み**: 検証ループを強くするには、これが一番効く。

## 8. 決めてほしいこと

1. ドキュメントの正本: 4.1 の A（skill 用に独自に書く）でよいか
2. レシピ: 新しく `examples/tasks` を作ってよいか
3. `whisker-module-authoring` を最初から作るか、`whisker` を出した後にするか
4. `whisker new` で AGENTS.md を生成するのを、今回の作業に含めるか
