//! aiwt バイナリのエンドツーエンド結合テスト。
//!
//! 各テストは隔離した一時 git リポジトリと、専用の `TMUX_TMPDIR`（tmux サーバを
//! ユーザーの実環境から隔離）の上で実バイナリを実行する。tmux 未インストール環境では
//! tmux を要するテストはスキップする。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// ビルド済み aiwt バイナリのパス（cargo がテスト時に注入）。
fn aiwt() -> &'static str {
    env!("CARGO_BIN_EXE_aiwt")
}

fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 指定 cwd で git を実行する（テストのセットアップ用）。
fn git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .status()
        .expect("git の実行に失敗");
    assert!(status.success(), "git {args:?} に失敗");
}

/// 隔離された一時リポジトリ。Drop で tmux サーバを後始末する。
struct TestRepo {
    dir: tempfile::TempDir,
    prefix: String,
}

impl TestRepo {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(dir.path().join("tmuxtmp")).unwrap();

        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@example.com"]);
        git(&repo, &["config", "user.name", "tester"]);
        fs::write(repo.join("a.txt"), "hello").unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-qm", "init"]);

        TestRepo {
            dir,
            prefix: "aiwttest-".to_string(),
        }
    }

    fn repo(&self) -> PathBuf {
        self.dir.path().join("repo")
    }

    fn wt_base(&self) -> PathBuf {
        self.dir.path().join("wts")
    }

    fn tmux_tmpdir(&self) -> PathBuf {
        self.dir.path().join("tmuxtmp")
    }

    /// この branch に対応する worktree パス。
    fn worktree(&self, branch: &str) -> PathBuf {
        let session = format!("{}{}", self.prefix, branch.replace('/', "-"));
        self.wt_base().join(session)
    }

    /// aiwt を隔離環境変数つきで実行する。
    fn run(&self, args: &[&str]) -> Output {
        Command::new(aiwt())
            .current_dir(self.repo())
            .args(args)
            .env("AIWT_WORKTREE_DIR", self.wt_base())
            .env("AIWT_SESSION_PREFIX", &self.prefix)
            .env("AIWT_AI_COMMAND", "echo hi") // claude を起動させない
            .env("TMUX_TMPDIR", self.tmux_tmpdir())
            .output()
            .expect("aiwt の実行に失敗")
    }

    fn git(&self, cwd: &Path, args: &[&str]) {
        git(cwd, args);
    }

    fn branch_exists(&self, branch: &str) -> bool {
        let out = Command::new("git")
            .current_dir(self.repo())
            .args(["branch", "--list", branch])
            .output()
            .unwrap();
        !String::from_utf8_lossy(&out.stdout).trim().is_empty()
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        // 隔離した tmux サーバを落とす（テンポラリ削除より前に実行される）。
        let _ = Command::new("tmux")
            .env("TMUX_TMPDIR", self.tmux_tmpdir())
            .arg("kill-server")
            .output();
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

// ───────────────────────── repo 非依存 ─────────────────────────

#[test]
fn completions_runs_without_repo() {
    let out = Command::new(aiwt())
        .args(["completions", "bash"])
        .output()
        .expect("実行失敗");
    assert!(out.status.success());
    assert!(stdout(&out).contains("aiwt"));
}

#[test]
fn config_reports_overrides() {
    let t = TestRepo::new();
    let out = t.run(&["config"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = stdout(&out);
    assert!(
        s.contains("echo hi"),
        "ai_command の上書きが反映されていない:\n{s}"
    );
}

// ───────────────────────── tmux 必須 ─────────────────────────

#[test]
fn new_creates_worktree_and_ls_lists_it() {
    if !tmux_available() {
        eprintln!("tmux 不在のためスキップ");
        return;
    }
    let t = TestRepo::new();

    let out = t.run(&["new", "feat/x"]);
    assert!(
        out.status.success(),
        "new 失敗: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(t.worktree("feat/x").exists(), "worktree が作られていない");
    // コンテキストファイルが書かれている
    assert!(t.worktree("feat/x").join(".aiwt-task.md").exists());

    let ls = t.run(&["ls"]);
    assert!(
        stdout(&ls).contains("feat/x"),
        "ls に branch が出ない:\n{}",
        stdout(&ls)
    );
}

#[test]
fn new_with_task_writes_context_file() {
    if !tmux_available() {
        eprintln!("tmux 不在のためスキップ");
        return;
    }
    let t = TestRepo::new();
    t.run(&["new", "feat/task", "これはタスク説明"]);
    let ctx = fs::read_to_string(t.worktree("feat/task").join(".aiwt-task.md")).unwrap();
    assert!(
        ctx.contains("これはタスク説明"),
        "タスクが書かれていない:\n{ctx}"
    );
    assert!(ctx.contains("# worktree: feat/task"));
}

#[test]
fn rm_removes_worktree_and_branch() {
    if !tmux_available() {
        eprintln!("tmux 不在のためスキップ");
        return;
    }
    let t = TestRepo::new();
    t.run(&["new", "feat/y"]);
    assert!(t.worktree("feat/y").exists());

    let out = t.run(&["rm", "feat/y", "-y"]);
    assert!(
        out.status.success(),
        "rm 失敗: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!t.worktree("feat/y").exists(), "worktree が消えていない");
    assert!(!t.branch_exists("feat/y"), "branch が消えていない");
}

#[test]
fn prune_removes_merged_and_keeps_active() {
    if !tmux_available() {
        eprintln!("tmux 不在のためスキップ");
        return;
    }
    let t = TestRepo::new();

    // マージ済みになる branch
    t.run(&["new", "feat/merged"]);
    t.git(&t.repo(), &["merge", "--no-edit", "-q", "feat/merged"]);

    // 未マージのコミットを持つ active branch
    t.run(&["new", "feat/active"]);
    let active = t.worktree("feat/active");
    fs::write(active.join("b.txt"), "x").unwrap();
    t.git(&active, &["add", "-A"]);
    t.git(&active, &["commit", "-qm", "wip"]);

    let dry = t.run(&["prune", "--dry-run"]);
    let s = stdout(&dry);
    assert!(s.contains("feat/merged"), "dry-run に対象が出ない:\n{s}");
    assert!(s.contains("dry-run"));
    assert!(
        t.worktree("feat/merged").exists(),
        "dry-run で消えてはいけない"
    );

    let out = t.run(&["prune", "-y"]);
    assert!(out.status.success());
    assert!(
        !t.worktree("feat/merged").exists(),
        "merged が掃除されていない"
    );
    assert!(
        t.worktree("feat/active").exists(),
        "active を消してはいけない"
    );

    t.run(&["rm", "feat/active", "-y"]);
}

#[test]
fn new_reuses_existing_worktree() {
    if !tmux_available() {
        eprintln!("tmux 不在のためスキップ");
        return;
    }
    let t = TestRepo::new();
    t.run(&["new", "feat/dup"]);
    let out = t.run(&["new", "feat/dup"]);
    assert!(out.status.success());
    assert!(
        stdout(&out).contains("再利用"),
        "再利用メッセージが出ない:\n{}",
        stdout(&out)
    );

    t.run(&["rm", "feat/dup", "-y"]);
}
