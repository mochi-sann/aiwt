//! サブコマンドの実装。

use std::collections::HashSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::error::Result;
use crate::template::Vars;
use crate::{fzf, git, tmux};

/// ブランチ名から tmux セッション名を作る（`/` → `-`、プレフィックス付与）。
fn session_name(prefix: &str, branch: &str) -> String {
    format!("{prefix}{}", branch.replace('/', "-"))
}

/// 先頭の `~` を HOME に展開する。
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

/// worktree 置き場のベースディレクトリを解決する。
/// 設定なし → `<repo親>/worktrees`。設定あり → 絶対パスはそのまま、相対は repo_root 基準。
fn resolve_worktree_base(repo_root: &Path, config: &Config) -> Result<PathBuf> {
    match &config.worktree_dir {
        Some(dir) => {
            let expanded = expand_tilde(dir);
            if expanded.is_absolute() {
                Ok(expanded)
            } else {
                Ok(repo_root.join(expanded))
            }
        }
        None => repo_root
            .parent()
            .map(|p| p.join("worktrees"))
            .ok_or_else(|| "repo の親ディレクトリを解決できません".into()),
    }
}

/// ベースブランチを解決する（設定 → 自動検出）。
fn resolve_base_branch(repo_root: &Path, config: &Config) -> Result<String> {
    match &config.base_branch {
        Some(b) => Ok(b.clone()),
        None => git::detect_base_branch(repo_root),
    }
}

/// y/N の確認を取る（デフォルト No）。
fn confirm(prompt: &str) -> Result<bool> {
    print!("{prompt} [y/N] ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(input.trim(), "y" | "Y"))
}

/// worktree_dir 配下の worktree を (branch, session, path) で返す。
/// detached（branch が `(...)`）は除外する。
fn managed_worktrees(repo_root: &Path, config: &Config) -> Result<Vec<(String, String, PathBuf)>> {
    let wt_base = resolve_worktree_base(repo_root, config)?;
    let mut result = Vec::new();
    for (path, branch) in git::worktree_list(repo_root)? {
        if !path.starts_with(&wt_base) || branch.starts_with('(') {
            continue;
        }
        let session = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        result.push((branch, session, path));
    }
    Ok(result)
}

/// worktree_dir 配下の worktree のブランチ名一覧（補完候補用）。
pub fn worktree_branch_names(repo_root: &Path, config: &Config) -> Result<Vec<String>> {
    Ok(managed_worktrees(repo_root, config)?
        .into_iter()
        .map(|(branch, _, _)| branch)
        .collect())
}

/// branch 指定があればそれを、無ければ fzf で選ばせる。
fn resolve_branch(
    repo_root: &Path,
    config: &Config,
    branch: Option<String>,
    prompt: &str,
) -> Result<String> {
    if let Some(b) = branch {
        return Ok(b);
    }
    let worktrees = managed_worktrees(repo_root, config)?;
    if worktrees.is_empty() {
        return Err("対象の worktree がありません".into());
    }
    let items: Vec<String> = worktrees.into_iter().map(|(b, _, _)| b).collect();
    fzf::select(&items, prompt)?.ok_or_else(|| "選択がキャンセルされました".into())
}

/// worktree・tmux セッション・（条件次第で）ブランチを削除する共通処理。
///
/// - `assume_yes`: ブランチ削除の確認をスキップ
/// - `always_delete_branch`: 確認なしで常にブランチを削除（prune 用）
fn remove_worktree(
    repo_root: &Path,
    config: &Config,
    branch: &str,
    assume_yes: bool,
    always_delete_branch: bool,
) -> Result<()> {
    let session = session_name(&config.session_prefix, branch);
    let wt_base = resolve_worktree_base(repo_root, config)?;
    let wt_path = wt_base.join(&session);

    if tmux::has_session(&session) {
        tmux::kill_session(&session)?;
        println!("✓ tmuxセッション削除: {session}");
    } else {
        println!("  tmuxセッションなし: {session}");
    }

    if git::worktree_exists(repo_root, &wt_path)? {
        git::git_inherit(
            repo_root,
            &["worktree", "remove", "--force", &wt_path.to_string_lossy()],
        )?;
        println!("✓ worktree削除: {}", wt_path.display());
    } else {
        println!("  worktreeなし: {}", wt_path.display());
    }

    if git::branch_exists(repo_root, branch) {
        let proceed = always_delete_branch
            || assume_yes
            || confirm(&format!("ブランチ '{branch}' を削除しますか？"))?;
        if proceed {
            git::git_inherit(repo_root, &["branch", "-D", branch])?;
            println!("✓ ブランチ削除: {branch}");
        }
    }
    Ok(())
}

// ───────────────────────── new ─────────────────────────

pub fn new(config: &Config, branch: &str, task: &str) -> Result<()> {
    let repo_root = git::repo_root()?;
    let session = session_name(&config.session_prefix, branch);
    let base_branch = resolve_base_branch(&repo_root, config)?;
    let wt_base = resolve_worktree_base(&repo_root, config)?;
    let wt_path = wt_base.join(&session);

    std::fs::create_dir_all(&wt_base)?;

    // --- worktree の存在確認・作成 ---
    if git::worktree_exists(&repo_root, &wt_path)? {
        println!("既存のworktreeを再利用: {}", wt_path.display());
    } else if git::branch_exists(&repo_root, branch) {
        println!("ブランチが存在するのでworktreeを追加: {branch}");
        git::git_inherit(
            &repo_root,
            &["worktree", "add", &wt_path.to_string_lossy(), branch],
        )?;
    } else {
        println!("{base_branch} から新規ブランチを作成: {branch}");
        git::git_inherit(
            &repo_root,
            &[
                "worktree",
                "add",
                "-b",
                branch,
                &wt_path.to_string_lossy(),
                &base_branch,
            ],
        )?;
    }

    // --- コンテキストファイルを書き出す ---
    let context_path = wt_path.join(&config.context_file);
    write_context_file(&context_path, &wt_path, branch, &base_branch, task)?;

    // --- プレースホルダ変数 ---
    let vars = Vars {
        branch: branch.to_string(),
        session: session.clone(),
        worktree: wt_path.to_string_lossy().to_string(),
        repo_root: repo_root.to_string_lossy().to_string(),
        base_branch: base_branch.clone(),
        context_file: context_path.to_string_lossy().to_string(),
        has_context: true,
        task: task.to_string(),
    };

    // --- tmux セッション構築 ---
    if tmux::has_session(&session) {
        println!("tmuxセッションが既に存在します: {session}");
    } else {
        build_tmux_session(config, &session, &wt_path, &vars)?;
        println!("tmuxセッションを起動: {session}");
    }

    println!();
    println!("✓ '{session}' を起動しました");
    println!("  アタッチ: tmux attach -t {session}");
    Ok(())
}

/// コンテキストファイル（mainからのコミット・差分・タスク）を書き出す。
fn write_context_file(
    path: &Path,
    wt_path: &Path,
    branch: &str,
    base_branch: &str,
    task: &str,
) -> Result<()> {
    let commits = git::git(
        wt_path,
        &["log", &format!("{base_branch}..HEAD"), "--oneline"],
    )
    .unwrap_or_default();
    let diff_stat = git::git(
        wt_path,
        &["diff", "--stat", &format!("{base_branch}...HEAD")],
    )
    .unwrap_or_default();

    let mut out = String::new();
    out.push_str(&format!("# worktree: {branch}\n\n"));
    out.push_str(&format!("**パス:** {}\n\n", wt_path.display()));
    out.push_str(&format!("## {base_branch} からのコミット\n"));
    if commits.is_empty() {
        out.push_str("(コミットなし)\n");
    } else {
        out.push_str(&format!("```\n{commits}\n```\n"));
    }
    out.push_str("\n## 変更ファイル\n");
    if diff_stat.is_empty() {
        out.push_str("(差分なし)\n");
    } else {
        out.push_str(&format!("```\n{diff_stat}\n```\n"));
    }
    if !task.is_empty() {
        out.push_str(&format!("\n## タスク\n{task}\n"));
    }
    std::fs::write(path, out)?;
    Ok(())
}

/// 設定の windows 構成に従って tmux セッションを組み立てる。
fn build_tmux_session(config: &Config, session: &str, cwd: &Path, vars: &Vars) -> Result<()> {
    let windows = config.resolved_windows();
    for (i, win) in windows.iter().enumerate() {
        let win_name = win.name.clone().unwrap_or_else(|| {
            if i == 0 {
                "main".into()
            } else {
                format!("w{i}")
            }
        });

        if i == 0 {
            tmux::new_session(session, cwd, Some(&win_name))?;
        } else {
            tmux::new_window(session, cwd, Some(&win_name))?;
        }

        // ペインを順に作りつつ、各アクティブペインへコマンドを送る。
        for (p, raw_cmd) in win.panes.iter().enumerate() {
            if p > 0 {
                tmux::split_window(session, &win_name, cwd)?;
            }
            let cmd = vars.render(raw_cmd);
            if !cmd.trim().is_empty() {
                tmux::send_keys(session, &win_name, &cmd)?;
            }
        }

        if let Some(layout) = &win.layout {
            tmux::select_layout(session, &win_name, layout)?;
        }
    }
    Ok(())
}

// ───────────────────────── ls ─────────────────────────

pub fn ls(config: &Config) -> Result<()> {
    let repo_root = git::repo_root()?;
    let wt_base = resolve_worktree_base(&repo_root, config)?;

    println!(
        "{:<30} {:<35} {:<6} {:<7}",
        "SESSION", "BRANCH", "TMUX", "CLAUDE"
    );
    println!("{}", "-".repeat(80));

    let mut has_any = false;
    for (path, branch) in git::worktree_list(&repo_root)? {
        if !path.starts_with(&wt_base) {
            continue;
        }
        has_any = true;
        let session = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let (tmux_mark, ai_mark) = if tmux::has_session(&session) {
            let commands = tmux::pane_commands(&session);
            // AI コマンドの実行ファイル名を抽出して一致を見る。
            let ai_bin = config
                .ai_command
                .split_whitespace()
                .next()
                .unwrap_or("claude");
            let ai_running = commands.iter().any(|c| c == ai_bin);
            ("✓", if ai_running { "✓" } else { "—" })
        } else {
            ("—", "—")
        };

        println!("{session:<30} {branch:<35} {tmux_mark:<6} {ai_mark:<7}");
    }

    if !has_any {
        println!("(worktree が見つかりません)");
    }
    Ok(())
}

// ───────────────────────── rm ─────────────────────────

pub fn rm(config: &Config, branch: Option<String>, assume_yes: bool) -> Result<()> {
    let repo_root = git::repo_root()?;
    let branch = resolve_branch(&repo_root, config, branch, "削除する worktree> ")?;
    remove_worktree(&repo_root, config, &branch, assume_yes, false)
}

// ───────────────────────── prune ─────────────────────────

/// マージ済み / gone のブランチを持つ worktree を一括掃除する。
pub fn prune(config: &Config, dry_run: bool, assume_yes: bool) -> Result<()> {
    let repo_root = git::repo_root()?;
    let base = resolve_base_branch(&repo_root, config)?;

    let merged: HashSet<String> = git::merged_branches(&repo_root, &base)?
        .into_iter()
        .collect();
    let gone: HashSet<String> = git::gone_branches(&repo_root)?.into_iter().collect();

    let mut targets: Vec<(String, String)> = Vec::new();
    for (branch, _session, _path) in managed_worktrees(&repo_root, config)? {
        if branch == base {
            continue;
        }
        let mut reasons = Vec::new();
        if merged.contains(&branch) {
            reasons.push("merged");
        }
        if gone.contains(&branch) {
            reasons.push("gone");
        }
        if !reasons.is_empty() {
            targets.push((branch, reasons.join(",")));
        }
    }

    if targets.is_empty() {
        println!("掃除対象なし（merged / gone の worktree はありません）");
        return Ok(());
    }

    println!("掃除対象:");
    for (branch, reason) in &targets {
        println!("  {branch}  [{reason}]");
    }

    if dry_run {
        println!("\n(dry-run: 何も削除していません)");
        return Ok(());
    }
    if !assume_yes && !confirm("\n上記をすべて削除しますか？")? {
        return Ok(());
    }

    for (branch, _) in &targets {
        println!("\n── {branch} ──");
        remove_worktree(&repo_root, config, branch, true, true)?;
    }
    Ok(())
}

// ───────────────────────── sync ─────────────────────────

pub fn sync(config: &Config, branch: Option<String>, reverse: bool) -> Result<()> {
    let repo_root = git::repo_root()?;
    let branch = resolve_branch(&repo_root, config, branch, "同期する worktree> ")?;
    let session = session_name(&config.session_prefix, &branch);
    let wt_base = resolve_worktree_base(&repo_root, config)?;
    let wt_path = wt_base.join(&session);

    if !git::worktree_exists(&repo_root, &wt_path)? {
        return Err(format!("worktree が見つかりません: {}", wt_path.display()).into());
    }

    if reverse {
        sync_reverse(&repo_root, &wt_path)
    } else {
        sync_forward(&repo_root, &wt_path, &branch, &session)
    }
}

/// root の現在ブランチ → worktree にマージ。
fn sync_reverse(repo_root: &Path, wt_path: &Path) -> Result<()> {
    let root_branch = git::current_branch(repo_root)?;
    let wt_branch = git::current_branch(wt_path)?;

    if git::has_uncommitted_changes(repo_root) {
        println!("警告: root に未コミットの変更があります");
        println!("{}", git::status_short(repo_root)?);
        if !confirm("続けますか？")? {
            return Ok(());
        }
    }

    println!("root の現在のブランチ: {root_branch}");
    println!("{root_branch} → worktree ({wt_branch}) にマージします");

    match git::git_inherit(wt_path, &["merge", &root_branch, "--no-edit"]) {
        Ok(()) => {
            println!();
            println!("✓ マージ完了: {root_branch} → {wt_branch}");
            Ok(())
        }
        Err(_) => {
            println!();
            println!("⚠ マージに競合が発生しました。手動で解決してください:");
            println!("  cd {}", wt_path.display());
            Err("マージ競合".into())
        }
    }
}

/// worktree のブランチを root にチェックアウト（使用中なら review ブランチを作る）。
fn sync_forward(repo_root: &Path, wt_path: &Path, branch: &str, session: &str) -> Result<()> {
    if git::has_uncommitted_changes(wt_path) {
        println!("警告: worktree に未コミットの変更があります");
        println!("{}", git::status_short(wt_path)?);
        if !confirm("続けますか？")? {
            return Ok(());
        }
    }

    let root_branch = git::current_branch(repo_root)?;
    let review_branch = format!("review/{session}");
    println!("root の現在のブランチ: {root_branch}");

    let (checked_out, is_review) = if git::git_ok(repo_root, &["checkout", branch]) {
        (branch.to_string(), false)
    } else {
        println!(
            "⚠ {branch} は worktree で使用中のため、{review_branch} として確認用ブランチを作成します"
        );
        if git::branch_exists(repo_root, &review_branch) {
            git::git_inherit(repo_root, &["branch", "-D", &review_branch])?;
        }
        git::git_inherit(repo_root, &["checkout", "-b", &review_branch, branch])?;
        (review_branch.clone(), true)
    };

    println!();
    println!("✓ root: {root_branch} → {checked_out}");

    if is_review {
        println!();
        println!("確認後の片付け:");
        println!("  git -C {} checkout {root_branch}", repo_root.display());
        println!("  git -C {} branch -D {review_branch}", repo_root.display());
    } else {
        println!(
            "確認後は: git -C {} checkout {root_branch}",
            repo_root.display()
        );
    }
    Ok(())
}

// ───────────────────────── config ─────────────────────────

/// 解決済み設定を表示する（デバッグ用）。
pub fn show_config(config: &Config) -> Result<()> {
    println!("ai_command     = {}", config.ai_command);
    println!("auto_start_ai  = {}", config.auto_start_ai);
    println!(
        "base_branch    = {}",
        config.base_branch.as_deref().unwrap_or("(自動検出)")
    );
    println!(
        "worktree_dir   = {}",
        config
            .worktree_dir
            .as_deref()
            .unwrap_or("(<repo親>/worktrees)")
    );
    println!("session_prefix = {:?}", config.session_prefix);
    println!("context_file   = {}", config.context_file);
    println!();
    println!("tmux 構成:");
    for (i, win) in config.resolved_windows().iter().enumerate() {
        let name = win.name.clone().unwrap_or_else(|| format!("w{i}"));
        let layout = win.layout.as_deref().unwrap_or("(default)");
        println!("  [{i}] window={name} layout={layout}");
        if win.panes.is_empty() {
            println!("      (シェルのみ)");
        }
        for (p, cmd) in win.panes.iter().enumerate() {
            println!("      pane {p}: {cmd}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用に worktree_dir だけ差し替えた Config を作る。
    fn cfg_with_worktree_dir(dir: Option<&str>) -> Config {
        Config {
            ai_command: "claude".into(),
            auto_start_ai: true,
            base_branch: None,
            worktree_dir: dir.map(|s| s.to_string()),
            session_prefix: String::new(),
            context_file: ".aiwt-task.md".into(),
            windows: None,
        }
    }

    #[test]
    fn session_name_replaces_slashes_and_adds_prefix() {
        assert_eq!(session_name("", "feature/foo"), "feature-foo");
        assert_eq!(session_name("wt-", "a/b/c"), "wt-a-b-c");
        assert_eq!(session_name("", "main"), "main");
    }

    #[test]
    fn expand_tilde_expands_home() {
        // SAFETY: テストはシングルスレッドで環境変数を一時設定する。
        unsafe {
            std::env::set_var("HOME", "/home/u");
        }
        assert_eq!(expand_tilde("~/wt"), PathBuf::from("/home/u/wt"));
        assert_eq!(expand_tilde("/abs/path"), PathBuf::from("/abs/path"));
        assert_eq!(expand_tilde("rel/path"), PathBuf::from("rel/path"));
    }

    #[test]
    fn worktree_base_defaults_to_sibling_dir() {
        let repo = Path::new("/home/u/proj");
        let base = resolve_worktree_base(repo, &cfg_with_worktree_dir(None)).unwrap();
        assert_eq!(base, PathBuf::from("/home/u/worktrees"));
    }

    #[test]
    fn worktree_base_absolute_is_used_as_is() {
        let repo = Path::new("/home/u/proj");
        let base = resolve_worktree_base(repo, &cfg_with_worktree_dir(Some("/var/wt"))).unwrap();
        assert_eq!(base, PathBuf::from("/var/wt"));
    }

    #[test]
    fn worktree_base_relative_resolves_against_repo() {
        let repo = Path::new("/home/u/proj");
        let base = resolve_worktree_base(repo, &cfg_with_worktree_dir(Some("../wt"))).unwrap();
        assert_eq!(base, PathBuf::from("/home/u/proj/../wt"));
    }

    #[test]
    fn worktree_base_expands_tilde() {
        // SAFETY: テストはシングルスレッドで環境変数を一時設定する。
        unsafe {
            std::env::set_var("HOME", "/home/u");
        }
        let repo = Path::new("/home/u/proj");
        let base = resolve_worktree_base(repo, &cfg_with_worktree_dir(Some("~/wt"))).unwrap();
        assert_eq!(base, PathBuf::from("/home/u/wt"));
    }
}
