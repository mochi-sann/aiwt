//! fzf によるインタラクティブ選択。

use std::io::Write;
use std::process::{Command, Stdio};

use crate::error::Result;

/// 候補を fzf に渡して 1 件選ばせる。
/// 選択された行（trim 済み）を返す。キャンセル時は `Ok(None)`。
pub fn select(items: &[String], prompt: &str) -> Result<Option<String>> {
    let mut child = Command::new("fzf")
        .args(["--prompt", prompt, "--height", "40%", "--reverse"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("fzf を起動できません（インストール済みですか）: {e}"))?;

    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or("fzf の標準入力を取得できません")?;
        stdin.write_all(items.join("\n").as_bytes())?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        // 非ゼロ終了（Esc / Ctrl-C でのキャンセルなど）
        return Ok(None);
    }
    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if selected.is_empty() {
        Ok(None)
    } else {
        Ok(Some(selected))
    }
}
