//! コマンド文字列中のプレースホルダ展開。
//!
//! 対応プレースホルダ:
//!   {branch}       ブランチ名
//!   {session}      tmux セッション名
//!   {worktree}     worktree の絶対パス
//!   {repo_root}    元リポジトリのルート
//!   {base_branch}  ベースブランチ
//!   {context_file} コンテキストファイルの絶対パス
//!   {context_arg}  コンテキストがあれば `"$(cat <file>)"`、なければ空文字
//!   {task}         タスク文字列

/// プレースホルダの値一式。
pub struct Vars {
    pub branch: String,
    pub session: String,
    pub worktree: String,
    pub repo_root: String,
    pub base_branch: String,
    pub context_file: String,
    /// コンテキストファイルを実際に書き出したか（context_arg の出し分けに使う）。
    pub has_context: bool,
    pub task: String,
}

impl Vars {
    /// テンプレート文字列を展開する。
    pub fn render(&self, template: &str) -> String {
        let context_arg = if self.has_context {
            format!("\"$(cat {})\"", self.context_file)
        } else {
            String::new()
        };
        template
            .replace("{branch}", &self.branch)
            .replace("{session}", &self.session)
            .replace("{worktree}", &self.worktree)
            .replace("{repo_root}", &self.repo_root)
            .replace("{base_branch}", &self.base_branch)
            .replace("{context_file}", &self.context_file)
            .replace("{context_arg}", &context_arg)
            .replace("{task}", &self.task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(has_context: bool) -> Vars {
        Vars {
            branch: "feat/x".into(),
            session: "feat-x".into(),
            worktree: "/wt/feat-x".into(),
            repo_root: "/repo".into(),
            base_branch: "main".into(),
            context_file: "/wt/feat-x/.aiwt-task.md".into(),
            has_context,
            task: "やること".into(),
        }
    }

    #[test]
    fn expands_basic_placeholders() {
        let v = sample(true);
        assert_eq!(v.render("git checkout {branch}"), "git checkout feat/x");
        assert_eq!(v.render("cd {worktree}"), "cd /wt/feat-x");
        assert_eq!(v.render("base={base_branch}"), "base=main");
    }

    #[test]
    fn context_arg_present_wraps_cat() {
        let v = sample(true);
        assert_eq!(
            v.render("claude {context_arg}"),
            "claude \"$(cat /wt/feat-x/.aiwt-task.md)\""
        );
    }

    #[test]
    fn context_arg_absent_is_empty() {
        let v = sample(false);
        assert_eq!(v.render("claude {context_arg}"), "claude ");
    }

    #[test]
    fn unknown_placeholder_is_left_as_is() {
        let v = sample(true);
        assert_eq!(v.render("echo {unknown}"), "echo {unknown}");
    }
}
