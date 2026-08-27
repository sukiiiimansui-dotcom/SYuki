//! 网易云 AI 工具（function-calling 可调）。
//! 让 AI 能：搜歌、按心情推歌。

use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use crate::ai_service::netmusic_service::{self, NetMusicSong};
use crate::ai_service::types::ToolDefinition;

/// 把歌曲列表推给前端全局播放器（AI 发歌 → 前端自动播放）。
/// 在搜索/推荐工具返回歌曲后触发，让网易云音乐能"不打扰游戏"地后台播放。
fn emit_play_to_frontend(app: &tauri::AppHandle, songs: &[NetMusicSong]) {
    // 选择要播的歌：优先第一首；若已在播同一首则不重复触发。
    let Some(first) = songs.first() else { return };
    use tauri::Emitter;
    let payload = serde_json::json!({
        "title": first.title,
        "artist": first.artist,
        "album": first.album,
        "url": first.url,
        "cover": first.cover,
        "duration": first.duration,
    });
    if let Err(e) = app.emit("netmusic:play", payload) {
        tracing::warn!("emit netmusic:play 失败: {e}");
    }
}

use super::executor::{Tool, ToolContext, ToolError, ToolResult};

const TIMEOUT: Duration = Duration::from_secs(25);

/// 歌曲 → 模型友好文本。
fn format_songs(songs: &[NetMusicSong]) -> String {
    if songs.is_empty() {
        return "没有找到相关歌曲。".into();
    }
    let mut out = String::new();
    for s in songs {
        let dur = if s.duration > 0 {
            format!("{}:{:02}", s.duration / 60, s.duration % 60)
        } else {
            "—".to_string()
        };
        out.push_str(&format!(
            "《{}》 - {} ({} | {}s)\n  链接: {}\n",
            s.title,
            s.artist,
            s.album,
            dur,
            s.url
        ));
    }
    out
}

/// 网易云搜索工具。
pub struct NetMusicSearchTool;

impl NetMusicSearchTool {
    pub fn new() -> Self {
        Self
    }
}

/// 网易云心情推荐工具。
pub struct NetMusicRecommendTool;

impl NetMusicRecommendTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for NetMusicSearchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "netmusic_search",
            "搜索网易云歌曲（返回歌名/歌手/专辑/时长/播放链接）。当用户想听某首歌、找音乐，或要求点歌时使用。",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "keyword": { "type": "string", "description": "搜索关键词（歌名/歌手）" },
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

    async fn execute(&self, context: &ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let keyword = args
            .get("keyword")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ToolError::InvalidArguments("缺少必填参数 keyword".into()))?;
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(8) as usize;
        let client = netmusic_service::build_client().map_err(ToolError::Execution)?;
        let songs = netmusic_service::search_songs(&client, keyword, limit.max(1)).await;
        let text = format_songs(&songs);
        if let Ok(app) = context.require_app() {
            emit_play_to_frontend(&app, &songs);
        }
        Ok(serde_json::json!({ "ok": true, "count": songs.len(), "text": text, "songs": songs }))
    }
}

#[async_trait]
impl Tool for NetMusicRecommendTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "netmusic_recommend",
            "按心情推荐网易云歌曲（happy/sad/angry/sleepy/calm/nostalgic）。当用户想听适配当前心情、氛围的歌，或要求推歌时使用。",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "mood": { "type": "string", "description": "心情关键词，可选 happy/sad/angry/sleepy/calm/nostalgic" },
                    "limit": { "type": "integer", "description": "返回数量，默认 10" }
                },
                "required": [],
                "additionalProperties": false
            }),
        )
    }

    fn timeout_hint(&self) -> Option<Duration> {
        Some(TIMEOUT)
    }

    async fn execute(&self, context: &ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let mood = args
            .get("mood")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize;
        let client = netmusic_service::build_client().map_err(ToolError::Execution)?;
        let songs = netmusic_service::recommend(&client, &mood, limit.max(1)).await;
        let text = format_songs(&songs);
        if let Ok(app) = context.require_app() {
            emit_play_to_frontend(&app, &songs);
        }
        Ok(serde_json::json!({ "ok": true, "count": songs.len(), "text": text, "songs": songs }))
    }
}
