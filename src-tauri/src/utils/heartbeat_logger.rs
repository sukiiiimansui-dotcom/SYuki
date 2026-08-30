//! 心跳 / 主动对话专用日志
//!
//! 单独记录 AI 的心跳 / 主动（想念 · 日程 · 主动投递 · 意图暂存等）事件，
//! 写到 `data/log/heartbeat/heartbeat_YYYYMMDD.log`。
//! 与通用应用日志（`data/log/app/`）分离，便于单独查看“AI 是否在想念 / 主动搭话”。
//!
//! 开关跟随应用日志开关（`log_enable`）。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use chrono::Local;

/// 全局开关（跟随应用日志开关）。
static ENABLED: AtomicBool = AtomicBool::new(true);

/// 心跳日志输出目录。
static LOG_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();

/// 初始化心跳日志模块。
///
/// 在 `data_dir/log/heartbeat/` 下创建目录。`enable` 为 `false` 时仍会创建目录
/// 结构，但不写入日志。
pub fn init(data_dir: &Path, enable: bool) {
    let log_dir = data_dir.join("log").join("heartbeat");
    let _ = fs::create_dir_all(&log_dir);
    LOG_DIR.set(log_dir).ok();
    ENABLED.store(enable, Ordering::Release);
}

/// 记录一条心跳 / 主动事件（追加到当日日志文件）。
///
/// `kind` 为事件类型（`deliver` / `miss_away` / `intent_flush` 等），`message` 为说明。
pub fn log_event(kind: &str, message: &str) {
    if !ENABLED.load(Ordering::Acquire) {
        return;
    }
    let Some(dir) = LOG_DIR.get() else { return };

    let day = Local::now().format("%Y%m%d");
    let path = dir.join(format!("heartbeat_{day}.log"));
    let line = format!(
        "[{}] [{}] {}\n",
        Local::now().format("%Y-%m-%d %H:%M:%S"),
        kind,
        message,
    );

    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        if let Err(e) = f.write_all(line.as_bytes()) {
            tracing::warn!("写入心跳日志失败: {e}");
        }
    }
}
