use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 用户当前的状态分类。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserState {
    IDLE,
    BROWSING,
    WORK,
    GAME,
    CASUAL,
}

impl UserState {
    pub fn as_str(&self) -> &'static str {
        match self {
            UserState::IDLE => "IDLE",
            UserState::BROWSING => "BROWSING",
            UserState::WORK => "WORK",
            UserState::GAME => "GAME",
            UserState::CASUAL => "CASUAL",
        }
    }
}

/// 系统感知外部环境/用户行为后的汇总结果。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PerceptionResult {
    pub state: UserState,
    pub description: String,
    pub interest_modifier: i32,
    pub visual_change_detected: bool,
    pub current_screen_text: String,
}

// ==========================================
// 日程与待办配置结构 (schedules.json 映射)
// ==========================================

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleItem {
    pub name: String,
    pub time: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleGroup {
    pub title: String,
    pub description: String,
    pub items: Vec<ScheduleItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    pub id: i64,
    pub text: String,
    pub priority: i32,
    pub completed: bool,
    pub deadline: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TodoGroup {
    pub title: String,
    pub description: Option<String>,
    pub todos: Vec<TodoItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImportantDay {
    pub id: String,
    pub date: String,
    pub title: String,
    pub desc: Option<String>,
    pub cycle: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserScheduleSettings {
    pub schedule_groups: Option<HashMap<String, ScheduleGroup>>,
    pub todo_groups: Option<HashMap<String, TodoGroup>>,
    pub important_days: Option<Vec<ImportantDay>>,
}

// ==========================================
// 主动对话意图暂存（"小本本"）
// ==========================================

use std::time::Instant;

/// 意图类型，带 TTL 和投放优先级。
/// `Ord` 派生顺序 = 投放优先级（高值优先投放）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntentType {
    Topic = 0,        // 闲聊 — 最低优先级
    Screen = 1,       // 屏幕感知 — 时效短（2min TTL）
    Todo = 2,         // 待办提醒
    ImportantDay = 3, // 重要日子
    Miss = 5,         // 用户离开后 AI 想念/主动搭话 — 优先级仅次于日程闹钟
    Alarm = 4,        // 日程闹钟 — 最高优先级，长 TTL（不应过期）
}

impl IntentType {
    /// 小写标签，用于前端展示。
    pub fn key(self) -> &'static str {
        match self {
            Self::Topic => "topic",
            Self::Screen => "screen",
            Self::Todo => "todo",
            Self::ImportantDay => "important_day",
            Self::Miss => "miss",
            Self::Alarm => "alarm",
        }
    }

    /// 意图存活时间（秒）。超时自动作废，不再投放。
    pub fn ttl_secs(self) -> u64 {
        match self {
            Self::Topic => 900,
            Self::Screen => 120,
            Self::Todo => 600,
            Self::ImportantDay => 600,
            Self::Miss => 1800,  // 30 分钟，确保想念不会被轻易丢弃
            Self::Alarm => 1800, // 30 分钟，确保不会被轻易丢弃
        }
    }
}

/// 暂存的主动对话意图。prompt 已完整生成，不可变。
#[derive(Clone, Debug)]
pub struct PendingIntent {
    /// 已格式化的系统旁白（PromptRole::System.build_prompt 的结果）
    pub prompt: String,
    pub intent_type: IntentType,
    /// 生成时间，用于 TTL 过期判断
    pub triggered_at: Instant,
}

// ==========================================
// 主动对话状态快照（供前端可视化）
// ==========================================

/// 一条已投放的主动对话事件（内存历史，重启丢失；保留最近若干条）。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ProactiveEvent {
    /// 触发时间（unix 毫秒）。
    pub ts_ms: u64,
    /// 意图类型的小写标签：miss / alarm / todo / important_day / screen / topic。
    pub kind: String,
    /// 触发 prompt 摘要（截断）。
    pub preview: String,
}

/// 暂存意图的快照。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PendingIntentSnapshot {
    pub kind: String,
    pub waited_secs: u64,
}

/// 主动系统运行时状态快照（读命令返回给前端）。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ProactiveStatusSnapshot {
    /// 主动系统总开关。
    pub enabled: bool,
    /// 后台 30s 轮询是否在跑。
    pub running: bool,
    /// 前端上报：当前是否适合投放。
    pub can_deliver: bool,
    /// 距最近一次用户交互的秒数。
    pub last_interaction_ago_secs: u64,
    /// 本轮离开期间已想念次数。
    pub away_delivered_count: i32,
    pub away_max_times: i32,
    pub away_timeout_secs: u32,
    /// 兴趣累积值 / 上限。
    pub interest: f64,
    pub interest_cap: f64,
    pub proactive_times: i32,
    pub max_proactive_count: i32,
    /// 感知到的用户状态（IDLE/BROWSING/WORK/GAME/CASUAL）。
    pub state: String,
    pub description: String,
    /// 暂存的待投放意图队列。
    pub pending_intents: Vec<PendingIntentSnapshot>,
    /// 已投放的主动对话历史。
    pub history: Vec<ProactiveEvent>,
}
