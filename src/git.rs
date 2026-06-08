//! git / worktree 操作のヘルパー。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::Result;

/// 任意ディレクトリで git を実行し、stdout（trim 済み）を返す。
pub fn git(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|e| format!("git の起動に失敗: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git {} に失敗: {}", args.join(" "), stderr.trim()).into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// git を実行し、成否だけを返す（失敗を許容したい場面用）。
pub fn git_ok(cwd: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// git を実行し、stdout/stderr を継承して進捗を表示する。
pub fn git_inherit(cwd: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .status()
        .map_err(|e| format!("git の起動に失敗: {e}"))?;
    if !status.success() {
        return Err(format!("git {} に失敗", args.join(" ")).into());
    }
    Ok(())
}

/// カレントディレクトリを含むリポジトリのルートを返す。
pub fn repo_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&cwd)
        .output()
        .map_err(|e| format!("git の起動に失敗: {e}"))?;
    if !out.status.success() {
        return Err("git リポジトリの中で実行してください".into());
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    ))
}

/// ローカルブランチが存在するか。
pub fn branch_exists(repo: &Path, branch: &str) -> bool {
    git_ok(
        repo,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
}

/// 現在チェックアウト中のブランチ名（detached なら空文字）。
pub fn current_branch(repo: &Path) -> Result<String> {
    git(repo, &["branch", "--show-current"])
}

/// ワーキングツリーに未コミットの変更があるか（staged 含む）。
pub fn has_uncommitted_changes(repo: &Path) -> bool {
    !git_ok(repo, &["diff", "--quiet"]) || !git_ok(repo, &["diff", "--cached", "--quiet"])
}

/// `git status --short` の出力。
pub fn status_short(repo: &Path) -> Result<String> {
    git(repo, &["status", "--short"])
}

/// `git worktree list --porcelain` の出力を (path, branch) に解析する。
fn parse_worktree_porcelain(out: &str) -> Vec<(PathBuf, String)> {
    let mut result = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut branch = String::new();
    for line in out.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            // 直前のエントリを確定
            if let Some(prev) = path.take() {
                result.push((prev, std::mem::take(&mut branch)));
            }
            path = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch ") {
            // refs/heads/foo → foo
            branch = b.strip_prefix("refs/heads/").unwrap_or(b).to_string();
        } else if line == "detached" {
            branch = "(detached)".to_string();
        }
    }
    if let Some(prev) = path.take() {
        result.push((prev, branch));
    }
    result
}

/// worktree 一覧を (path, branch) で返す。
pub fn worktree_list(repo: &Path) -> Result<Vec<(PathBuf, String)>> {
    let out = git(repo, &["worktree", "list", "--porcelain"])?;
    Ok(parse_worktree_porcelain(&out))
}

/// 指定パスが worktree として登録済みか。
pub fn worktree_exists(repo: &Path, wt_path: &Path) -> Result<bool> {
    Ok(worktree_list(repo)?.iter().any(|(p, _)| p == wt_path))
}

/// `base` にマージ済みのローカルブランチ一覧。
pub fn merged_branches(repo: &Path, base: &str) -> Result<Vec<String>> {
    let out = git(
        repo,
        &["branch", "--merged", base, "--format", "%(refname:short)"],
    )?;
    Ok(out
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// `%(refname:short)|%(upstream:track)` 形式から gone なブランチ名を抽出する。
fn parse_gone_branches(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|line| {
            let (name, track) = line.split_once('|')?;
            track.contains("gone").then(|| name.to_string())
        })
        .collect()
}

/// upstream が削除された（gone）ローカルブランチ一覧。
pub fn gone_branches(repo: &Path) -> Result<Vec<String>> {
    let out = git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname:short)|%(upstream:track)",
            "refs/heads/",
        ],
    )?;
    Ok(parse_gone_branches(&out))
}

/// ベースブランチを検出する。
/// origin/HEAD → main → master → 現在ブランチ の順。
pub fn detect_base_branch(repo: &Path) -> Result<String> {
    // origin/HEAD のシンボリック参照（例: refs/remotes/origin/main）
    if let Ok(sym) = git(
        repo,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
    ) && let Some(name) = sym.strip_prefix("refs/remotes/origin/")
        && !name.is_empty()
    {
        return Ok(name.to_string());
    }
    for candidate in ["main", "master"] {
        if branch_exists(repo, candidate) {
            return Ok(candidate.to_string());
        }
    }
    let current = current_branch(repo)?;
    if current.is_empty() {
        return Err("ベースブランチを検出できません。設定 base_branch を指定してください".into());
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_worktree_porcelain_basic() {
        let out = "\
worktree /repo
HEAD abc123
branch refs/heads/main

worktree /wt/feat-x
HEAD def456
branch refs/heads/feature/x
";
        let parsed = parse_worktree_porcelain(out);
        assert_eq!(
            parsed,
            vec![
                (PathBuf::from("/repo"), "main".to_string()),
                (PathBuf::from("/wt/feat-x"), "feature/x".to_string()),
            ]
        );
    }

    #[test]
    fn parse_worktree_porcelain_detached() {
        let out = "\
worktree /repo
HEAD abc123
branch refs/heads/main

worktree /wt/loose
HEAD def456
detached
";
        let parsed = parse_worktree_porcelain(out);
        assert_eq!(
            parsed[1],
            (PathBuf::from("/wt/loose"), "(detached)".to_string())
        );
    }

    #[test]
    fn parse_worktree_porcelain_empty() {
        assert!(parse_worktree_porcelain("").is_empty());
    }

    #[test]
    fn parse_gone_branches_filters_only_gone() {
        let out = "\
main|
feature/x|[ahead 1]
old/feat|[gone]
another|[behind 2, gone]
";
        let gone = parse_gone_branches(out);
        assert_eq!(gone, vec!["old/feat".to_string(), "another".to_string()]);
    }

    #[test]
    fn parse_gone_branches_handles_empty() {
        assert!(parse_gone_branches("").is_empty());
    }
}
