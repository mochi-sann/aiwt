# aiwt — AI WorkTree

tmux + AI + git worktree を統合管理する汎用 CLI。

ブランチごとに git worktree を切り、専用の tmux セッションを立て、その中で AI
（Claude Code など）やエディタ・dev サーバーを設定どおりに起動するまでを 1
コマンドで行います。元は `wt-new` / `wt-ls` / `wt-rm` / `wt-sync` という 4 本の
bash スクリプトでしたが、それらを単一バイナリのサブコマンドに統合し、
**AI 起動コマンド・ベースブランチ・worktree 置き場・tmux パネル構成** を
設定ファイルで管理できるようにしたものです。

## インストール

```sh
# リポジトリ直下で
cargo build --release
cp target/release/aiwt ~/.local/bin/   # PATH の通った場所へ
# または
cargo install --path .
```

## 使い方

```sh
aiwt new <branch> [task...]      # worktree + tmux セッションを作成して起動
aiwt ls                          # worktree とセッションの一覧
aiwt rm [branch] [-y]            # worktree / tmux / (任意で) ブランチを削除
aiwt sync [branch] [--reverse]   # worktree と root のブランチを同期
aiwt prune [--dry-run] [-y]      # マージ済み/gone ブランチの worktree を一括掃除
aiwt completions <shell>         # シェル補完を出力（zsh など）
aiwt config                      # 解決済みの設定を表示（確認用）
```

- `new` … ブランチが無ければベースブランチから作成し、worktree を追加。tmux
  セッションを設定の構成で起動し、worktree 内にコンテキストファイル
  （ベースブランチからのコミット・差分・タスク）を書き出します。
- `sync` … 既定は **worktree → root**（root にチェックアウトして確認。使用中なら
  `review/<session>` を作成）。`--reverse` で **root → worktree** にマージ。
- `rm` … `-y` でブランチ削除の確認をスキップ。
- `prune` … ベースブランチにマージ済み、または upstream が削除された（gone）
  ブランチの worktree・セッション・ブランチをまとめて削除。`--dry-run` で対象だけ確認。
- `rm` / `sync` は **branch を省略すると fzf** で worktree を選択できます。

### zsh 補完

**動的補完（推奨）** — `rm` / `sync` で実際の worktree ブランチ、`new` で既存
ローカルブランチを補完時に提示します。`~/.zshrc` に以下を追加:

```sh
source <(COMPLETE=zsh aiwt)
```

bash / fish も同様（`COMPLETE=bash` / `COMPLETE=fish`）。

**静的補完** — サブコマンドのみ（ブランチ名の動的提示なし）。事前生成したい場合:

```sh
aiwt completions zsh > ~/.zfunc/_aiwt   # fpath に通したディレクトリへ
# ~/.zshrc に fpath=(~/.zfunc $fpath); autoload -U compinit && compinit
```

## 設定

優先順位（低 → 高）:

1. グローバル `~/.config/aiwt/config.toml`
2. プロジェクト `<repo>/.aiwt.toml`
3. 環境変数 `AIWT_AI_COMMAND` / `AIWT_BASE_BRANCH` / `AIWT_WORKTREE_DIR` /
   `AIWT_SESSION_PREFIX` / `AIWT_AUTO_START_AI`
4. CLI フラグ `--ai-command` / `--base` / `--worktree-dir` / `--config`

全項目は [`config.example.toml`](./config.example.toml) を参照。主な項目:

| キー | 既定 | 説明 |
| --- | --- | --- |
| `ai_command` | `claude {context_arg}` | AI 起動コマンド。`aider` や `codex` 等に差し替え可 |
| `auto_start_ai` | `true` | `windows` 未指定時に AI を自動起動するか |
| `base_branch` | 自動検出 | origin/HEAD → main → master → 現在ブランチ |
| `worktree_dir` | `<repo親>/worktrees` | 相対は repo 基準、絶対パス・`~` 可 |
| `session_prefix` | `""` | tmux セッション名の接頭辞 |
| `context_file` | `.aiwt-task.md` | worktree 内に書くコンテキストファイル名 |
| `windows` | 1 window・1 pane | tmux のウィンドウ/ペイン構成 |

### tmux パネル構成

`windows` でウィンドウ・ペイン・レイアウト・各ペインのコマンドを定義できます。

```toml
[[windows]]
name = "code"
layout = "main-vertical"
panes = [
  "claude {context_arg}",   # pane 0: AI
  "nvim .",                 # pane 1: エディタ
  "",                       # pane 2: 空シェル
]

[[windows]]
name = "dev"
panes = ["pnpm dev"]
```

各ペインのコマンド内で使えるプレースホルダ:

| プレースホルダ | 展開先 |
| --- | --- |
| `{branch}` | ブランチ名 |
| `{session}` | tmux セッション名 |
| `{worktree}` | worktree の絶対パス |
| `{repo_root}` | 元リポジトリのルート |
| `{base_branch}` | ベースブランチ |
| `{context_file}` | コンテキストファイルの絶対パス |
| `{context_arg}` | コンテキストがあれば `"$(cat <file>)"`、なければ空 |
| `{task}` | `new` に渡したタスク文字列 |

## 旧スクリプトからの移行

| 旧 | 新 |
| --- | --- |
| `wt-new <b> [task]` | `aiwt new <b> [task]` |
| `wt-ls` | `aiwt ls` |
| `wt-rm <b>` | `aiwt rm <b>` |
| `wt-sync [-r] <b>` | `aiwt sync [--reverse] <b>` |

旧版で `claude`・`main`・`../worktrees` に固定だった部分が、すべて設定・環境変数・
フラグで変更できるようになっています。シェルエイリアス（`alias wt-new='aiwt new'`
など）を張れば従来のコマンド名のまま使えます。
