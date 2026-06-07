//! tmux 操作のヘルパー。

use std::path::Path;
use std::process::Command;

use crate::error::Result;

/// tmux サブコマンドを実行する（出力は捨てる）。
fn tmux(args: &[&str]) -> Result<()> {
    let status = Command::new("tmux")
        .args(args)
        .status()
        .map_err(|e| format!("tmux の起動に失敗: {e}"))?;
    if !status.success() {
        return Err(format!("tmux {} に失敗", args.join(" ")).into());
    }
    Ok(())
}

/// tmux サブコマンドを実行し stdout を返す。
fn tmux_out(args: &[&str]) -> Result<String> {
    let out = Command::new("tmux")
        .args(args)
        .output()
        .map_err(|e| format!("tmux の起動に失敗: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// セッションが存在するか。
pub fn has_session(session: &str) -> bool {
    Command::new("tmux")
        .args(["has-session", "-t", session])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 新規セッションを detached で作る（最初のウィンドウ付き）。
pub fn new_session(session: &str, cwd: &Path, window_name: Option<&str>) -> Result<()> {
    let cwd = cwd.to_string_lossy();
    let mut args = vec!["new-session", "-d", "-s", session, "-c", &cwd];
    if let Some(name) = window_name {
        args.push("-n");
        args.push(name);
    }
    tmux(&args)
}

/// 既存セッションに新規ウィンドウを足す。
pub fn new_window(session: &str, cwd: &Path, window_name: Option<&str>) -> Result<()> {
    let cwd = cwd.to_string_lossy();
    let target = session.to_string();
    let mut args = vec!["new-window", "-t", &target, "-c", &cwd];
    if let Some(name) = window_name {
        args.push("-n");
        args.push(name);
    }
    tmux(&args)
}

/// 指定ウィンドウを分割して新規ペインを作る。
pub fn split_window(session: &str, window: &str, cwd: &Path) -> Result<()> {
    let cwd = cwd.to_string_lossy();
    let target = format!("{session}:{window}");
    tmux(&["split-window", "-t", &target, "-c", &cwd])
}

/// ウィンドウのレイアウトを選択する。
pub fn select_layout(session: &str, window: &str, layout: &str) -> Result<()> {
    let target = format!("{session}:{window}");
    tmux(&["select-layout", "-t", &target, layout])
}

/// 指定ウィンドウのアクティブペインにコマンドを送り Enter を押す。
///
/// ペイン番号で指定すると `pane-base-index` の設定差で壊れるため、
/// 分割直後にアクティブになるペインへ送る前提で運用する。
pub fn send_keys(session: &str, window: &str, keys: &str) -> Result<()> {
    let target = format!("{session}:{window}");
    tmux(&["send-keys", "-t", &target, keys, "Enter"])
}

/// セッションの各ペインで実行中のコマンド一覧。
pub fn pane_commands(session: &str) -> Vec<String> {
    tmux_out(&["list-panes", "-t", session, "-F", "#{pane_current_command}"])
        .map(|s| s.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default()
}

/// セッションを削除する。
pub fn kill_session(session: &str) -> Result<()> {
    tmux(&["kill-session", "-t", session])
}
