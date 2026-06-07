# aiwt 仕様書

`aiwt`（AI WorkTree）の現行仕様。tmux + AI + git worktree を統合管理する CLI。

## 1. コマンド一覧

| コマンド | 説明 |
| --- | --- |
| `aiwt new <branch> [task...]` | worktree 作成 + tmux セッションを設定構成で起動。コンテキストファイルを書き出す |
| `aiwt ls`（別名 `list`） | worktree・tmux・AI 稼働状況の一覧 |
| `aiwt rm [branch] [-y/--yes]` | tmux セッション・worktree・(任意で) ブランチを削除。`-y` で確認スキップ。branch 省略時は fzf 選択 |
| `aiwt sync [branch] [-r/--reverse]` | worktree ↔ root のブランチ同期。branch 省略時は fzf 選択 |
| `aiwt prune [--dry-run] [-y/--yes]` | マージ済み / gone ブランチの worktree を一括掃除 |
| `aiwt completions <shell>` | 静的なシェル補完スクリプトを出力（bash/zsh/fish/powershell/elvish） |
| `aiwt config` | 解決済み設定の表示（確認用） |

`rm` / `sync` は branch を省略すると、worktree_dir 配下の worktree 一覧を **fzf**
で選択する（fzf 未インストール時はエラー）。

### グローバルフラグ（全サブコマンド共通・環境変数連動）

| フラグ | 環境変数 | 説明 |
| --- | --- | --- |
| `--ai-command` | `AIWT_AI_COMMAND` | AI 起動コマンドを上書き |
| `--base` | `AIWT_BASE_BRANCH` | ベースブランチを上書き |
| `--worktree-dir` | `AIWT_WORKTREE_DIR` | worktree 置き場を上書き |
| `--config` | `AIWT_CONFIG` | プロジェクト設定の代わりに使う設定ファイル |

## 2. 設定の優先順位（低 → 高）

1. ビルトインのデフォルト
2. グローバル `~/.config/aiwt/config.toml`（`XDG_CONFIG_HOME` 対応）
3. プロジェクト `<repo>/.aiwt.toml`（`--config` 指定時はそのファイル）
4. 環境変数 `AIWT_*`
5. CLI フラグ

- 高優先レイヤーで指定した値だけが下位を上書きする（未指定は据え置き）。
- `windows` は部分マージせず「指定があれば丸ごと置き換え」。

## 3. 設定キー

| キー | 既定 | 説明 |
| --- | --- | --- |
| `ai_command` | `claude {context_arg}` | AI 起動コマンド。`windows` 未指定時のデフォルト pane で使用 |
| `auto_start_ai` | `true` | `windows` 未指定時に AI を起動するか（false なら空シェル） |
| `base_branch` | 自動検出 | 検出順: `origin/HEAD` → `main` → `master` → 現在ブランチ |
| `worktree_dir` | `<repo親>/worktrees` | 相対 = repo 基準、絶対パス・先頭 `~` 可 |
| `session_prefix` | `""` | tmux セッション名の接頭辞 |
| `context_file` | `.aiwt-task.md` | worktree 内に書くコンテキストファイル名 |
| `windows` | 1 window・1 pane | tmux のウィンドウ/ペイン構成（§4） |

環境変数は上記に加え `AIWT_SESSION_PREFIX` / `AIWT_AUTO_START_AI`（`1`/`true`/`yes` で真）も対応。

## 4. tmux パネル構成（`[[windows]]`）

```toml
[[windows]]
name = "code"            # 省略時 main / w1 / w2 ...
layout = "main-vertical" # tmux select-layout 名（省略可）
panes = ["claude {context_arg}", "nvim .", ""]  # 1 要素 = 1 ペイン、"" は空シェル
```

- 先頭ウィンドウは `new-session`、以降は `new-window` で追加。
- 各ペインは順に split しつつ、**直後にアクティブになるペインへ** `send-keys` する
  （`pane-base-index` の設定差に依存しない）。
- 全ペイン生成後に `layout` を適用。
- `panes` が空のウィンドウはシェル 1 枚のまま。

## 5. プレースホルダ

各 pane コマンド内で展開される。

| プレースホルダ | 展開先 |
| --- | --- |
| `{branch}` | ブランチ名 |
| `{session}` | tmux セッション名 |
| `{worktree}` | worktree の絶対パス |
| `{repo_root}` | 元リポジトリのルート |
| `{base_branch}` | ベースブランチ |
| `{context_file}` | コンテキストファイルの絶対パス |
| `{context_arg}` | コンテキスト有なら `"$(cat <context_file>)"`、無ければ空 |
| `{task}` | `new` に渡したタスク文字列 |

## 6. 各コマンドの挙動詳細

### new
- セッション名 = `session_prefix` + `branch` の `/`→`-` 置換。
- worktree が既存 → 再利用 / ブランチが既存 → `worktree add` / それ以外 → ベースブランチから新規作成。
- コンテキストファイルに「ベースブランチからのコミット（`log base..HEAD --oneline`）・
  差分（`diff --stat base...HEAD`）・タスク」を出力。
- tmux セッションが既存ならセッション構築をスキップ。

### ls
- `worktree list` のうち `worktree_dir` 配下のみ表示。
- TMUX 稼働の有無、`ai_command` の先頭バイナリがペインで実行中かを判定して表示。

### rm
- tmux 削除 → `worktree remove --force` → ブランチは確認の上 `branch -D`（`-y` でスキップ）。

### sync（既定: worktree → root）
- root に当該ブランチを `checkout`。使用中なら `review/<session>` を作成。
- worktree に未コミット変更があれば警告し確認。

### sync --reverse（root → worktree）
- root の現在ブランチを worktree へ `merge --no-edit`。
- 競合時は解決手順を提示して非ゼロ終了。
- root に未コミット変更があれば警告し確認。

### prune
- 掃除対象 = worktree_dir 配下の worktree のうち、ブランチが
  **ベースブランチにマージ済み**（`git branch --merged <base>`）または
  **upstream が削除済み（gone）**（`%(upstream:track)` が `gone`）のもの。
- ベースブランチ自身は対象外。
- 対象一覧と理由（`merged` / `gone`）を表示。`--dry-run` なら削除せず終了。
- 確認後（`-y` でスキップ）、各対象を worktree+セッション+ブランチごと削除。

### completions（静的）
- `clap_complete` でシェル補完を stdout に出力。リポジトリ外でも動作。
- サブコマンド・フラグのみ補完（ブランチ名の動的提示はなし）。
- 例: `aiwt completions zsh > ~/.zfunc/_aiwt`。

### 動的補完（dynamic suggest）
- `clap_complete` の `unstable-dynamic`（`CompleteEnv`）で実装。
- 登録: `source <(COMPLETE=zsh aiwt)`（bash/fish も `COMPLETE=<shell>`）。
- 補完時に**実際のリポジトリ状態**から候補を生成:
  - `rm` / `sync` の branch → worktree_dir 配下の worktree ブランチ（`complete::worktree_branches`）
  - `new` の branch → 既存ローカルブランチ全件（`complete::local_branches`）
- 候補生成はエラー時に空へフォールバックし、補完を壊さない。
- 実体は `COMPLETE` 環境変数が設定されたときのみ動作（通常実行には影響しない）。

## 7. 実装・配置

- 言語: Rust（edition 2024）。依存 = clap(derive,env) / clap_complete / serde / toml。
- ソース: `main.rs`（CLI）/ `config.rs` / `git.rs` / `tmux.rs` / `template.rs` /
  `fzf.rs` / `complete.rs`（動的補完候補）/ `commands.rs`。
- テスト: `cargo test`（プレースホルダ展開・設定マージ・session_name・expand_tilde 等 11 件）。
- 旧 `wt-*` bash スクリプト 4 本は chezmoi から削除し aiwt に一本化。
- グローバル設定サンプルは chezmoi `dot_config/aiwt/config.toml`。
- インストール: `cargo build --release && cp target/release/aiwt ~/.local/bin/`。
- zsh 補完: `aiwt completions zsh > ~/.zfunc/_aiwt`（`fpath` に追加）。
