//! aiwt — AI WorkTree
//!
//! tmux + AI + git worktree を統合管理する汎用 CLI。
//! 既存の wt-new / wt-ls / wt-rm / wt-sync を単一バイナリのサブコマンドに統合し、
//! AI 起動コマンドや tmux パネル構成を設定ファイルで管理できるようにする。

mod commands;
mod complete;
mod config;
mod error;
mod fzf;
mod git;
mod template;
mod tmux;

use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use clap_complete::engine::ArgValueCandidates;
use clap_complete::env::CompleteEnv;

use config::{Config, Overrides};
use git::repo_root;

#[derive(Parser)]
#[command(name = "aiwt", version, about = "tmux + AI + git worktree 統合管理ツール")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// AI 起動コマンドを上書き（例: "aider {context_arg}"）。
    #[arg(long, global = true, env = "AIWT_AI_COMMAND")]
    ai_command: Option<String>,

    /// ベースブランチを上書き（未指定なら自動検出）。
    #[arg(long = "base", global = true, env = "AIWT_BASE_BRANCH")]
    base_branch: Option<String>,

    /// worktree 置き場を上書き。
    #[arg(long, global = true, env = "AIWT_WORKTREE_DIR")]
    worktree_dir: Option<String>,

    /// プロジェクト設定の代わりに使う設定ファイル。
    #[arg(long, global = true, env = "AIWT_CONFIG")]
    config: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// worktree + tmux セッションを作成し、設定の構成で起動する。
    New {
        /// ブランチ名（補完は既存ローカルブランチ）。
        #[arg(add = ArgValueCandidates::new(complete::local_branches))]
        branch: String,
        /// AI に渡すタスク説明（複数語可）。
        #[arg(trailing_var_arg = true)]
        task: Vec<String>,
    },
    /// worktree とセッションの一覧を表示する。
    #[command(alias = "list")]
    Ls,
    /// worktree・tmux セッション・（任意で）ブランチを削除する。
    Rm {
        /// ブランチ名（省略時は fzf で選択。補完は worktree のブランチ）。
        #[arg(add = ArgValueCandidates::new(complete::worktree_branches))]
        branch: Option<String>,
        /// ブランチ削除の確認をスキップする。
        #[arg(long, short)]
        yes: bool,
    },
    /// worktree と root の間でブランチを同期する。
    Sync {
        /// ブランチ名（省略時は fzf で選択。補完は worktree のブランチ）。
        #[arg(add = ArgValueCandidates::new(complete::worktree_branches))]
        branch: Option<String>,
        /// root → worktree 方向にマージする。
        #[arg(long, short)]
        reverse: bool,
    },
    /// マージ済み / gone のブランチを持つ worktree を一括掃除する。
    Prune {
        /// 削除せず対象だけ表示する。
        #[arg(long)]
        dry_run: bool,
        /// 確認をスキップする。
        #[arg(long, short)]
        yes: bool,
    },
    /// シェル補完スクリプトを出力する（例: aiwt completions zsh）。
    Completions {
        /// 対象シェル（bash / zsh / fish / powershell / elvish）。
        shell: Shell,
    },
    /// 解決済みの設定を表示する。
    Config,
}

fn run(cli: Cli) -> error::Result<()> {
    // リポジトリに依存しないコマンドを先に処理する。
    if let Command::Completions { shell } = &cli.command {
        let mut cmd = Cli::command();
        clap_complete::generate(*shell, &mut cmd, "aiwt", &mut io::stdout());
        return Ok(());
    }

    let overrides = Overrides {
        ai_command: cli.ai_command,
        base_branch: cli.base_branch,
        worktree_dir: cli.worktree_dir,
        config_path: cli.config,
    };

    // 設定はリポジトリルート基準で解決する。
    let root = repo_root()?;
    let config = Config::load(&root, &overrides)?;

    match cli.command {
        Command::New { branch, task } => commands::new(&config, &branch, &task.join(" ")),
        Command::Ls => commands::ls(&config),
        Command::Rm { branch, yes } => commands::rm(&config, branch, yes),
        Command::Sync { branch, reverse } => commands::sync(&config, branch, reverse),
        Command::Prune { dry_run, yes } => commands::prune(&config, dry_run, yes),
        Command::Config => commands::show_config(&config),
        Command::Completions { .. } => unreachable!("先に処理済み"),
    }
}

fn main() -> ExitCode {
    // 動的補完: `COMPLETE=zsh aiwt` のように呼ばれた場合はここで補完を出力して終了する。
    CompleteEnv::with_factory(Cli::command).complete();

    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("エラー: {e}");
            ExitCode::FAILURE
        }
    }
}
