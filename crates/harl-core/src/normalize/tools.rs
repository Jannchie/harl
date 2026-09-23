//! 工具名 → 类别。各 harness 的名字不同（`Bash` / `bash` / `powershell`，`Read` / `read_file`），
//! 按功能归到同一类，统计和 UI 才能跨 harness 比。名字大小写不敏感；`mcp__*` 一律是 mcp。
//! 名单来自对本机全部会话的统计（2026-09），没见过的归 other。

use crate::event::ToolCategory::{self, *};

const TABLE: &[(&str, ToolCategory)] = &[
    // shell
    ("bash", Shell),
    ("powershell", Shell),
    ("pwsh", Shell),
    ("run_code", Shell),
    // 读文件
    ("read", Read),
    ("read_file", Read),
    ("notebookread", Read),
    // 改文件
    ("edit", Edit),
    ("multiedit", Edit),
    ("write", Edit),
    ("notebookedit", Edit),
    ("edit_file", Edit),
    ("write_file", Edit),
    // 搜索
    ("grep", Search),
    ("glob", Search),
    ("lsp", Search),
    // 网络
    ("webfetch", Web),
    ("websearch", Web),
    ("web_fetch", Web),
    ("web_search", Web),
    // 子 agent 与后台任务
    ("agent", Agent),
    ("run_subagent", Agent),
    ("subagenthandback", Agent),
    ("sendmessage", Agent),
    ("listagents", Agent),
    ("workflow", Agent),
    ("taskoutput", Agent),
    ("taskstop", Agent),
    ("monitor", Agent),
    ("get_task_output", Agent),
    ("cancel_task", Agent),
    // 和用户交互
    ("askuserquestion", User),
    ("request_user_input", User),
    ("ask_user_question", User),
    ("senduserfile", User),
    ("artifact", User),
    ("pushnotification", User),
    // harness 自身的控制：skill、计划模式、worktree、todo、定时……
    ("skill", Meta),
    ("load_skill", Meta),
    ("toolsearch", Meta),
    ("enterplanmode", Meta),
    ("exitplanmode", Meta),
    ("enterworktree", Meta),
    ("exitworktree", Meta),
    ("todowrite", Meta),
    ("todo_write", Meta),
    ("set_workspace", Meta),
    ("bill_stats", Meta),
    ("croncreate", Meta),
    ("crondelete", Meta),
    ("cronlist", Meta),
    ("schedulewakeup", Meta),
    ("reportfindings", Meta),
    ("sendfeedback", Meta),
    ("create_goal", Meta),
    ("get_goal", Meta),
    ("update_goal", Meta),
];

pub fn categorize(name: &str) -> ToolCategory {
    if name.starts_with("mcp__") {
        return Mcp;
    }
    let lower = name.to_ascii_lowercase();
    TABLE.iter().find(|(n, _)| *n == lower).map(|(_, c)| *c).unwrap_or(Other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_harness_names_share_category() {
        assert_eq!(categorize("Bash"), Shell);
        assert_eq!(categorize("bash"), Shell);
        assert_eq!(categorize("PowerShell"), Shell);
        assert_eq!(categorize("Read"), Read);
        assert_eq!(categorize("read_file"), Read);
        assert_eq!(categorize("Agent"), Agent);
        assert_eq!(categorize("run_subagent"), Agent);
        assert_eq!(categorize("mcp__claude-in-chrome__navigate"), Mcp);
        assert_eq!(categorize("something_new"), Other);
    }
}
