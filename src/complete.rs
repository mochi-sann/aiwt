//! 補完時に動的に候補を生成する（clap_complete の unstable-dynamic）。
//!
//! `COMPLETE=zsh aiwt` 経由の動的補完で、ブランチ名を実際のリポジトリ状態から提示する。
//! いずれもエラー時は空候補にフォールバックし、補完を壊さない。

use clap_complete::engine::CompletionCandidate;

use crate::commands;
use crate::config::{Config, Overrides};
use crate::git;

/// rm / sync 用: worktree_dir 配下の worktree のブランチ名。
pub fn worktree_branches() -> Vec<CompletionCandidate> {
    let Ok(root) = git::repo_root() else {
        return Vec::new();
    };
    let Ok(config) = Config::load(&root, &Overrides::default()) else {
        return Vec::new();
    };
    commands::worktree_branch_names(&root, &config)
        .unwrap_or_default()
        .into_iter()
        .map(CompletionCandidate::new)
        .collect()
}

/// new 用: ローカルブランチ全件（既存ブランチの再利用を補助）。
pub fn local_branches() -> Vec<CompletionCandidate> {
    let Ok(root) = git::repo_root() else {
        return Vec::new();
    };
    let Ok(out) = git::git(
        &root,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads/"],
    ) else {
        return Vec::new();
    };
    out.lines()
        .filter(|l| !l.is_empty())
        .map(CompletionCandidate::new)
        .collect()
}
