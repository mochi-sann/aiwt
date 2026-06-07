//! 設定の読み込みとマージ。
//!
//! 優先順位（低→高）:
//!   1. ビルトインのデフォルト
//!   2. グローバル設定 `~/.config/aiwt/config.toml`
//!   3. プロジェクト設定 `<repo_root>/.aiwt.toml`
//!   4. 環境変数 (`AIWT_*`)
//!   5. CLI フラグ
//!
//! 高優先のレイヤーで指定された値だけが下のレイヤーを上書きする（未指定は据え置き）。

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::Result;

/// TOML から読む生の設定。全フィールドが省略可能で、マージ用に使う。
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawConfig {
    /// AI 起動コマンド（windows 未指定時のデフォルト pane に使われる）。
    pub ai_command: Option<String>,
    /// worktree 作成時に AI を自動起動するか。
    pub auto_start_ai: Option<bool>,
    /// ベースブランチ。未指定なら自動検出（origin/HEAD → main → master → 現在ブランチ）。
    pub base_branch: Option<String>,
    /// worktree 置き場。未指定なら `<repo親>/worktrees`。
    /// 相対パスは repo_root 基準、絶対パス・`~` 展開に対応。
    pub worktree_dir: Option<String>,
    /// tmux セッション名のプレフィックス。
    pub session_prefix: Option<String>,
    /// worktree 内に作るコンテキストファイル名。
    pub context_file: Option<String>,
    /// tmux のウィンドウ/ペイン構成。未指定ならデフォルト（1 window・1 pane）。
    pub windows: Option<Vec<WindowConfig>>,
}

/// tmux の 1 ウィンドウ分の構成。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowConfig {
    /// ウィンドウ名（省略時は連番）。
    #[serde(default)]
    pub name: Option<String>,
    /// `tmux select-layout` のレイアウト名（例: main-vertical, tiled）。
    #[serde(default)]
    pub layout: Option<String>,
    /// 各ペインで実行するコマンド。1 要素 = 1 ペイン。空ならシェルのまま。
    #[serde(default)]
    pub panes: Vec<String>,
}

impl RawConfig {
    /// TOML ファイルを読む。存在しなければ空設定。
    fn from_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("設定ファイルを読めません {}: {e}", path.display()))?;
        let cfg: RawConfig = toml::from_str(&text)
            .map_err(|e| format!("設定ファイルの解析に失敗 {}: {e}", path.display()))?;
        Ok(cfg)
    }

    /// `other` の指定値で self を上書きする（other 優先）。
    fn overlay(&mut self, other: RawConfig) {
        if other.ai_command.is_some() {
            self.ai_command = other.ai_command;
        }
        if other.auto_start_ai.is_some() {
            self.auto_start_ai = other.auto_start_ai;
        }
        if other.base_branch.is_some() {
            self.base_branch = other.base_branch;
        }
        if other.worktree_dir.is_some() {
            self.worktree_dir = other.worktree_dir;
        }
        if other.session_prefix.is_some() {
            self.session_prefix = other.session_prefix;
        }
        if other.context_file.is_some() {
            self.context_file = other.context_file;
        }
        if other.windows.is_some() {
            self.windows = other.windows;
        }
    }
}

/// CLI から渡される上書き値。
#[derive(Debug, Default, Clone)]
pub struct Overrides {
    pub ai_command: Option<String>,
    pub base_branch: Option<String>,
    pub worktree_dir: Option<String>,
    /// 明示的に指定された設定ファイルパス（プロジェクト設定の代わりに使う）。
    pub config_path: Option<PathBuf>,
}

/// 解決済みの設定。
#[derive(Debug, Clone)]
pub struct Config {
    pub ai_command: String,
    pub auto_start_ai: bool,
    pub base_branch: Option<String>,
    pub worktree_dir: Option<String>,
    pub session_prefix: String,
    pub context_file: String,
    pub windows: Option<Vec<WindowConfig>>,
}

impl Config {
    /// 全レイヤーをマージして設定を解決する。
    pub fn load(repo_root: &Path, overrides: &Overrides) -> Result<Self> {
        let mut raw = RawConfig::default();

        // 2. グローバル設定
        if let Some(global) = global_config_path() {
            raw.overlay(RawConfig::from_file(&global)?);
        }

        // 3. プロジェクト設定（--config 指定があればそちらを優先して使う）
        if let Some(explicit) = &overrides.config_path {
            raw.overlay(RawConfig::from_file(explicit)?);
        } else {
            raw.overlay(RawConfig::from_file(&repo_root.join(".aiwt.toml"))?);
        }

        // 4. 環境変数
        raw.overlay(env_config());

        // 5. CLI フラグ
        if overrides.ai_command.is_some() {
            raw.ai_command = overrides.ai_command.clone();
        }
        if overrides.base_branch.is_some() {
            raw.base_branch = overrides.base_branch.clone();
        }
        if overrides.worktree_dir.is_some() {
            raw.worktree_dir = overrides.worktree_dir.clone();
        }

        Ok(Config {
            ai_command: raw
                .ai_command
                .unwrap_or_else(|| default_ai_command().to_string()),
            auto_start_ai: raw.auto_start_ai.unwrap_or(true),
            base_branch: raw.base_branch,
            worktree_dir: raw.worktree_dir,
            session_prefix: raw.session_prefix.unwrap_or_default(),
            context_file: raw
                .context_file
                .unwrap_or_else(|| ".aiwt-task.md".to_string()),
            windows: raw.windows,
        })
    }

    /// 実際に使う tmux ウィンドウ構成を返す。
    /// 設定に windows があればそれを、なければデフォルト構成を組み立てる。
    pub fn resolved_windows(&self) -> Vec<WindowConfig> {
        if let Some(windows) = &self.windows {
            return windows.clone();
        }
        // デフォルト: 1 window・1 pane。auto_start_ai なら AI を起動。
        let panes = if self.auto_start_ai {
            vec![self.ai_command.clone()]
        } else {
            vec![]
        };
        vec![WindowConfig {
            name: Some("main".to_string()),
            layout: None,
            panes,
        }]
    }
}

fn default_ai_command() -> &'static str {
    // {context_arg} はコンテキストファイルがあれば `"$(cat <file>)"` に展開される。
    "claude {context_arg}"
}

/// `~/.config/aiwt/config.toml`（XDG_CONFIG_HOME 優先）のパス。
fn global_config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("aiwt").join("config.toml"))
}

/// 環境変数からの設定。
fn env_config() -> RawConfig {
    let mut raw = RawConfig::default();
    if let Ok(v) = std::env::var("AIWT_AI_COMMAND") {
        raw.ai_command = Some(v);
    }
    if let Ok(v) = std::env::var("AIWT_BASE_BRANCH") {
        raw.base_branch = Some(v);
    }
    if let Ok(v) = std::env::var("AIWT_WORKTREE_DIR") {
        raw.worktree_dir = Some(v);
    }
    if let Ok(v) = std::env::var("AIWT_SESSION_PREFIX") {
        raw.session_prefix = Some(v);
    }
    if let Ok(v) = std::env::var("AIWT_AUTO_START_AI") {
        raw.auto_start_ai = Some(matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes"));
    }
    raw
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(ai: &str, auto: bool, windows: Option<Vec<WindowConfig>>) -> Config {
        Config {
            ai_command: ai.into(),
            auto_start_ai: auto,
            base_branch: None,
            worktree_dir: None,
            session_prefix: String::new(),
            context_file: ".aiwt-task.md".into(),
            windows,
        }
    }

    #[test]
    fn overlay_replaces_only_specified_fields() {
        let mut base = RawConfig {
            ai_command: Some("claude".into()),
            base_branch: Some("main".into()),
            ..Default::default()
        };
        base.overlay(RawConfig {
            ai_command: Some("aider".into()),
            ..Default::default()
        });
        assert_eq!(base.ai_command.as_deref(), Some("aider")); // 上書き
        assert_eq!(base.base_branch.as_deref(), Some("main")); // 据え置き
    }

    #[test]
    fn overlay_none_keeps_lower_layer() {
        let mut base = RawConfig {
            worktree_dir: Some("../wt".into()),
            ..Default::default()
        };
        base.overlay(RawConfig::default());
        assert_eq!(base.worktree_dir.as_deref(), Some("../wt"));
    }

    #[test]
    fn default_windows_start_ai_when_enabled() {
        let c = cfg("claude {context_arg}", true, None);
        let windows = c.resolved_windows();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].panes, vec!["claude {context_arg}".to_string()]);
    }

    #[test]
    fn default_windows_empty_when_ai_disabled() {
        let c = cfg("claude", false, None);
        let windows = c.resolved_windows();
        assert_eq!(windows.len(), 1);
        assert!(windows[0].panes.is_empty());
    }

    #[test]
    fn explicit_windows_take_precedence() {
        let win = WindowConfig {
            name: Some("dev".into()),
            layout: None,
            panes: vec!["pnpm dev".into()],
        };
        let c = cfg("claude", true, Some(vec![win]));
        let windows = c.resolved_windows();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].name.as_deref(), Some("dev"));
    }
}
