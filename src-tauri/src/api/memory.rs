use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::AppState;

/// 返回给前端的角色记忆库视图（MemoryBank）。
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RoleMemoryView {
    pub role_id: i32,
    pub role_name: String,
    /// 永久记忆是否开启（全局配置 `use_persistent_memory`）。
    pub memory_enabled: bool,
    pub schema_version: u32,
    /// 最近一次自动压缩更新的时间。
    pub updated_at: String,
    /// 短期上下文摘要（近期回顾 / 承接话题）。
    pub short_term: String,
    /// 长期经历编年史（关键事件）。
    pub long_term: String,
    /// 用户信息（ta 的画像：姓名/年龄/喜好/雷点）。
    pub user_info: String,
    /// 待办与契约清单（重要约定）。
    pub promises: String,
}

/// 读取指定角色的完整记忆库（MemoryBank），供前端记忆可视化面板展示。
#[tauri::command]
pub async fn get_role_memory_bank(
    app: AppHandle,
    role_id: i32,
) -> Result<RoleMemoryView, String> {
    let state = app.state::<AppState>();
    let svc = state.ai_service.lock().await;
    let gs = svc.game_status.lock().await;

    let role = gs
        .role_manager
        .get_loaded(role_id)
        .ok_or_else(|| format!("角色 {} 未加载（记忆库不可用）", role_id))?;

    let role_name = role.display_name.clone().unwrap_or_default();
    let memory_enabled = gs.role_manager.memory_enabled();

    // 快照优先取后台压缩引擎的实时缓存，否则回退到 role.memory_bank。
    let bank = gs
        .role_manager
        .get_role_memory_bank(role_id)
        .await
        .unwrap_or_else(|| role.memory_bank.clone());

    Ok(RoleMemoryView {
        role_id,
        role_name,
        memory_enabled,
        schema_version: bank.schema_version,
        updated_at: bank.meta.updated_at,
        short_term: bank.data.short_term,
        long_term: bank.data.long_term,
        user_info: bank.data.user_info,
        promises: bank.data.promises,
    })
}
