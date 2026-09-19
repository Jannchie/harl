//! canonical 事件 v1：所有 harness 归一到同一形状，搜索、turn/step 视图都建在它上面。
//!
//! 形状取自 preferred-harness 的协议日志（session → turn → step → message / tool_call），
//! 因为它是显式记录这些边界的；其他 harness 往里映射，缺的边界用推断补上并在 `inferred` 里声明。
//!
//! 一行 raw 对应一个 Event（1:1），`prov` 指回那一行的 chunk；没映射的字段原样留在 `ext`。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    TurnStarted,
    TurnCompleted,
    StepStarted,
    StepCompleted,
    Message,
    ToolCall,
    ToolResult,
    /// 模型这一步实际收到的请求上下文：system prompt 片段、可用工具、模型……
    /// pfh 的 request_header；Claude Code 的 environment / model / instructions / prompt_snapshot 等 attachment
    RequestHeader,
    /// 不参与对话视图的一切：mode、summary、attachment……
    #[default]
    Meta,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    Tool,
}

/// turn / step 的身份：id 用来分组，n 是从 1 数的序号（有些 harness 不给）
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub id: String,
    pub n: Option<u32>,
}

/// 工具按功能归类，跨 harness 可比（`Bash` 和 `bash` 都是 shell）。映射表在 normalize/tools.rs。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    Shell,
    Read,
    Edit,
    Search,
    Web,
    /// 子 agent、后台任务
    Agent,
    /// 和用户交互：提问、交付文件
    User,
    Mcp,
    /// harness 自身的控制：skill、计划模式、todo……
    Meta,
    #[default]
    Other,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct Tool {
    pub call_id: String,
    pub name: String,
    /// 知道名字才有（Claude Code 的 tool_result 行没有名字）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<ToolCategory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// 各 harness 的 usage 键名不同，用 alias 一次收齐；没映射的键留在 `ext`
/// （Claude Code 的 `cache_creation`、`service_tier` 等）。alias 命中的键被吃掉，不会重复出现在 ext 里。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Usage {
    #[serde(alias = "inputTokens", alias = "input_tokens", skip_serializing_if = "Option::is_none")]
    pub input: Option<u64>,
    #[serde(alias = "outputTokens", alias = "output_tokens", skip_serializing_if = "Option::is_none")]
    pub output: Option<u64>,
    #[serde(alias = "cacheReadTokens", alias = "cache_read_input_tokens", skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<u64>,
    #[serde(alias = "cacheWriteTokens", alias = "cache_creation_input_tokens", skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<u64>,
    #[serde(flatten, skip_serializing_if = "Map::is_empty")]
    pub ext: Map<String, Value>,
}

/// 指向别的事件 / 会话的边。normalizer 只连边不合并，去重和折叠由投影决定。
#[derive(Serialize, Debug, Clone, Default)]
pub struct Link {
    /// spawned / notifies / delivered_from / replay_of / continues / compacted_from …
    pub rel: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// 这条边是推断的还是日志里显式给的
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub inferred: bool,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct Prov {
    /// raw 行的 chunk hash（blake3 hex）；行号就是 `seq`
    pub chunk: String,
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct Event {
    pub provider: &'static str,
    pub session: String,
    /// 父会话（subagent 时有）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session: Option<String>,
    /// 在源文件里的行号，从 0 数
    pub seq: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<DateTime<Utc>>,
    pub kind: Kind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn: Option<Scope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<Scope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    /// 归一化后的正文：user 输入、assistant 文本
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<Tool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// 哪些字段不是原文给的、是 normalizer 推断的（"turn"、"step"……）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inferred: Vec<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
    /// 原始对象里没被映射走的字段
    pub ext: Map<String, Value>,
    pub prov: Prov,
}
