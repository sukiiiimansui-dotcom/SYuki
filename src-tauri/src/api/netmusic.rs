//! 网易云 API（前端功能页调用）。
//! 依赖 `crate::ai_service::netmusic_service` 的核心实现。

use crate::ai_service::netmusic_service::{self, NetMusicSong};

/// 网易云搜索歌曲。
#[tauri::command]
pub async fn netmusic_search(keyword: String, limit: Option<usize>) -> Result<Vec<NetMusicSong>, String> {
    if keyword.trim().is_empty() {
        return Err("搜索关键词不能为空".into());
    }
    let client = netmusic_service::build_client()?;
    Ok(netmusic_service::search_songs(&client, keyword.trim(), limit.unwrap_or(8)).await)
}

/// 心情/时段推荐歌曲（mood 为空则按默认流行）。
#[tauri::command]
pub async fn netmusic_recommend(mood: Option<String>, limit: Option<usize>) -> Result<Vec<NetMusicSong>, String> {
    let client = netmusic_service::build_client()?;
    let mood = netmusic_service::normalize_mood(&mood.unwrap_or_default());
    Ok(netmusic_service::recommend(&client, &mood, limit.unwrap_or(10)).await)
}

/// 外链播放地址。
#[tauri::command]
pub fn netmusic_url(song_id: u64) -> Result<String, String> {
    Ok(netmusic_service::playing_url(song_id))
}
