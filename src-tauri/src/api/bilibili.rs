//! B站学习 API（前端功能页调用）。
//! 依赖 `crate::ai_service::bilibili_service` 的核心实现。

use crate::ai_service::bilibili_service::{
    self, BiliLearnResult, BiliSearchItem, BiliVideo,
};

/// B站热榜 bvid 列表。
#[tauri::command]
pub async fn bili_hot(limit: Option<usize>) -> Result<Vec<String>, String> {
    let client = bilibili_service::build_client()?;
    Ok(bilibili_service::get_hot_bvids(&client, limit.unwrap_or(10)).await)
}

/// B站视频搜索。
#[tauri::command]
pub async fn bili_search(query: String, limit: Option<usize>) -> Result<Vec<BiliSearchItem>, String> {
    if query.trim().is_empty() {
        return Err("搜索关键词不能为空".into());
    }
    let client = bilibili_service::build_client()?;
    Ok(bilibili_service::search_videos(&client, query.trim(), limit.unwrap_or(8)).await)
}

/// 学习一个视频（信息 + 弹幕 + 评论 → 学习库）。
#[tauri::command]
pub async fn bili_learn(bvid: String) -> Result<BiliLearnResult, String> {
    let bvid = bvid.trim().to_string();
    if bvid.is_empty() {
        return Err("bvid 不能为空".into());
    }
    let client = bilibili_service::build_client()?;
    Ok(bilibili_service::learn_video(&client, &super::data_dir(), &bvid).await)
}

/// 查询学习库。
#[tauri::command]
pub fn bili_knowledge(q: Option<String>, limit: Option<usize>) -> Result<Vec<BiliVideo>, String> {
    let data_dir = super::data_dir();
    let q = q.unwrap_or_default();
    Ok(bilibili_service::search_knowledge(&data_dir, &q, limit.unwrap_or(20)))
}

/// 供对话注入：最近学习的弹幕文化（小 token）。
#[tauri::command]
pub fn bili_recent_context(limit: Option<usize>) -> Result<Vec<BiliVideo>, String> {
    let data_dir = super::data_dir();
    Ok(bilibili_service::search_knowledge(&data_dir, "", limit.unwrap_or(3)))
}
