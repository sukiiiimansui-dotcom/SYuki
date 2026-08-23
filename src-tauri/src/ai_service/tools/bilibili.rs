//! B站学习 AI 工具（function-calling 可调）。
//! 让 AI 能：搜 B站视频、学习一个视频、查询已学知识库。

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use crate::ai_service::bilibili_service::{self, BiliSearchItem, BiliVideo};
use crate::ai_service::types::ToolDefinition;

use super::executor::{Tool, ToolContext, ToolError, ToolResult};

const TIMEOUT: Duration = Duration::from_secs(25);

/// 站点视频搜索工具。
pub struct BiliSearchTool;

/// 学习单个视频信息工具（信息+弹幕+评论→学习库）。
pub struct BiliLearnTool {
    data_dir: PathBuf,
}

/// 查询已学知识库工具。
pub struct BiliKnowledgeTool {
    data_dir: PathBuf,
}

impl BiliSearchTool {
    pub fn new() -> Self {
        Self
    }
}

impl BiliLearnTool {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }
}

impl BiliKnowledgeTool {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }
}

/// 搜索结果 → 模型友好文本。
fn format_search_items(items: &[BiliSearchItem]) -> String {
    if items.is_empty() {
        return "没有找到相关的 B站视频。".into();
    }
    let mut out = String::new();
    for it in items {
        out.push_str(&format!(
            "《{}》 UP: {} | 播放: {} | 赞: {}\n  简介: {}\n  https://www.bilibili.com/video/{}\n",
            it.title,
            it.up,
            it.play,
            it.like,
            if it.desc.is_empty() { "—".to_string() } else { it.desc.clone() },
            it.bvid
        ));
    }
    out
}

/// 学习库条目 → 模型友好文本。
fn format_videos(videos: &[BiliVideo]) -> String {
    if videos.is_empty() {
        return "学习库还是空的，可以让用户提供 bvid，或先用 bilibili_learn_video 学习热门视频。".into();
    }
    let mut out = String::new();
    for v in videos {
        out.push_str(&format!(
            "《{}》(UP: {}) 分区: {} | 弹幕梗: {} | 高赞评论: {} | 学到时间: {}\n",
            v.title,
            v.up,
            v.tname,
            if v.repeat_danmaku.is_empty() { "—".to_string() } else { v.repeat_danmaku.clone() },
            if v.top_comments.is_empty() { "—".to_string() } else { v.top_comments.clone() },
            v.learned_at
        ));
    }
    out
}

#[async_trait]
impl Tool for BiliSearchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "bilibili_search_videos",
            "搜索B站视频（返回标题/UP主/播放/点赞/简介）。当用户想找B站视频、学习某个主题、或要看某个内容时使用。",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "keyword": { "type": "string", "description": "搜索关键词" },
                    "limit": { "type": "integer", "description": "返回数量，默认 8" }
                },
                "required": ["keyword"],
                "additionalProperties": false
            }),
        )
    }

    fn timeout_hint(&self) -> Option<Duration> {
        Some(TIMEOUT)
    }

    async fn execute(&self, _: &ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let keyword = args
            .get("keyword")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ToolError::InvalidArguments("缺少必填参数 keyword".into()))?;
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(8) as usize;
        let client = bilibili_service::build_client().map_err(ToolError::Execution)?;
        let items = bilibili_service::search_videos(&client, keyword, limit.max(1)).await;
        let text = format_search_items(&items);
        Ok(serde_json::json!({
            "ok": true,
            "count": items.len(),
            "text": text,
            "items": items,
        }))
    }
}

#[async_trait]
impl Tool for BiliLearnTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "bilibili_learn_video",
            "学习一个B站视频（拉取视频信息、重复弹幕、高赞评论并存入学习库）。当用户给了一个 bvid/链接、或想学习某个视频的弹幕文化和大家看法时使用。",
            serde_json::json!({
                "type": "object",
                "properties": { "bvid": { "type": "string", "description": "视频 bvid，例 BV1xx411c7mD" } },
                "required": ["bvid"],
                "additionalProperties": false
            }),
        )
    }

    fn timeout_hint(&self) -> Option<Duration> {
        Some(TIMEOUT)
    }

    async fn execute(&self, _: &ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let bvid = args
            .get("bvid")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ToolError::InvalidArguments("缺少必填参数 bvid".into()))?;
        let client = bilibili_service::build_client().map_err(ToolError::Execution)?;
        let result = bilibili_service::learn_video(&client, &self.data_dir, bvid).await;
        if !result.ok {
            return Ok(serde_json::json!({
                "ok": false,
                "bvid": result.bvid,
                "text": "学习失败：视频不存在或网络错误。",
            }));
        }
        Ok(serde_json::json!({
            "ok": true,
            "bvid": result.bvid,
            "title": result.title,
            "up": result.up,
            "danmaku": result.danmaku,
            "repeat": result.repeat,
            "comments": result.comments,
            "text": format!("已学习《{}》({})：获取 {} 条弹幕、{} 条高频弹幕、{} 条高赞评论。", result.title, result.up, result.danmaku, result.repeat, result.comments),
        }))
    }
}

#[async_trait]
impl Tool for BiliKnowledgeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "bilibili_search_knowledge",
            "查询已学B站知识库（通过关键词搜标题/UP主/弹幕梗）。当你已学过一个视频、或用户问起之前学过的内容时使用。",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "搜索关键词，可为空(返回最新)" },
                    "limit": { "type": "integer", "description": "返回数量，默认 20" }
                },
                "required": [],
                "additionalProperties": false
            }),
        )
    }

    fn timeout_hint(&self) -> Option<Duration> {
        Some(TIMEOUT)
    }

    async fn execute(&self, _: &ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
        let videos = bilibili_service::search_knowledge(&self.data_dir, &query, limit.max(1));
        let text = format_videos(&videos);
        Ok(serde_json::json!({
            "ok": true,
            "count": videos.len(),
            "text": text,
            "items": videos,
        }))
    }
}
