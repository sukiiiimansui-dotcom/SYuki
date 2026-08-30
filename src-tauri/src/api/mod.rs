pub mod achievement;
pub mod adventure;
pub mod ambient;
pub mod asset;
pub mod background;
pub mod bilibili;
pub mod character;
pub mod chat;
pub mod font;
pub mod game;
pub mod locale;
pub mod memory;
pub mod music;
pub mod netmusic;
pub mod pet;
// 插件命令由 RustPython 驱动，移动端（Android/iOS）不编译
#[cfg(desktop)]
pub mod plugins;
pub mod save;
pub mod scene;
pub mod schedule;
pub mod screenshot;
pub mod script;
pub mod script_editor;
pub mod settings;
pub mod tool_settings;
pub mod workshop;

use std::path::PathBuf;

use tauri::Manager;
use crate::AppState;

// ========== 共享路径辅助函数 ==========

pub(crate) fn data_dir() -> PathBuf {
    crate::init::static_copy::get_data_dir().clone()
}

pub(crate) fn game_data_dir() -> PathBuf {
    data_dir().join("game_data")
}

pub(crate) fn characters_dir() -> PathBuf {
    game_data_dir().join("characters")
}

pub(crate) fn backgrounds_dir() -> PathBuf {
    game_data_dir().join("backgrounds")
}

pub(crate) fn music_dir() -> PathBuf {
    game_data_dir().join("musics")
}

pub(crate) fn ambient_dir() -> PathBuf {
    game_data_dir().join("ambients")
}

pub(crate) fn voice_dir() -> PathBuf {
    data_dir().join("voice")
}

pub(crate) fn fonts_dir() -> PathBuf {
    data_dir().join("fonts")
}


// ========== 主动对话系统指令 ==========

/// 前端通知后端当前是否具备主动对话投放条件。
/// 仅在最终布尔值翻转时调用。
#[tauri::command]
pub async fn proactive_set_can_deliver(
    app: tauri::AppHandle,
    can_deliver: bool,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    if let Some(ref ps) = state.proactive_system {
        ps.lock().await.set_can_deliver(can_deliver);
    }
    Ok(())
}

/// 前端心跳上报：用户有交互时刷新"最近活跃"时间，用于唤醒离开想念/心跳触发。
#[tauri::command]
pub async fn proactive_mark_active(app: tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    if let Some(ref ps) = state.proactive_system {
        ps.lock().await.mark_user_active();
    }
    Ok(())
}

/// 读取主动对话系统的运行时状态快照（含已投放的主动历史），供前端可视化。
#[tauri::command]
pub async fn get_proactive_status(
    app: tauri::AppHandle,
) -> Result<crate::ai_service::proactive_system::types::ProactiveStatusSnapshot, String> {
    let state = app.state::<AppState>();
    match &state.proactive_system {
        Some(ps) => Ok(ps.lock().await.status_snapshot().await),
        None => Err("主动对话系统未运行！".into()),
    }
}
pub mod role_archive;
