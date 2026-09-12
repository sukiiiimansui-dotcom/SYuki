//! 聊天工具的用户配置（与权限矩阵分离），持久化在 `data/tool_settings.toml`。
//!
//! 权限矩阵（`tool_permissions.toml`）决定"哪些工具允许下发给模型"，
//! 这里的配置决定"工具自身如何工作"（API Key、代理等）。
//! `SharedToolSettings` 在 AppState 与工具实例间共享，保存后立即生效。

use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::permissions::ToolPermissionConfig;

pub const SETTINGS_FILE_NAME: &str = "tool_settings.toml";

/// 工具分组 → 组内工具注册名。
/// 设置页按组开关，权限同步时组内工具一起放开/收回。
/// web_search 不在此列：它有独立的 enabled + 配置就绪判断。
pub const TOOL_GROUPS: &[(&str, &[&str])] = &[
    (
        "schedule",
        &[
            "schedule_get_all",
            "schedule_add_todo",
            "schedule_update_todo",
            "schedule_delete_todo",
        ],
    ),
    (
        "memory",
        &[
            "memory_get_current",
            "memory_get_notes",
            "memory_add_note",
            "memory_update_note",
            "memory_delete_note",
        ],
    ),
    ("character", &["character_list", "character_switch"]),
    // P3-4：**世界模拟专属**的三个地图工具并进 scene 组，跟「场景」开关同一个开关。
    //
    // 为什么不新建一个 `world_map` 组：设置页的工具组列表是前端**硬编码**的
    // （`src/api/services/tool-settings.ts` 的组名+i18n），后端新建一个组名，
    // 用户在那个页面上根本看不到它 → 组永远关着 → 工具还是拿不到。
    // 并进 scene 组是**零前端改动**就能让用户开得起来的唯一做法。
    //
    // 语义上也站得住：这三个工具干的事与 scene_list/scene_switch 是同一类 ——
    // 让 AI 知道「我现在在哪、周围有什么」（get_my_location / get_nearby_facilities）
    // 与改变「我处在什么情境」（move_to），都属于情境感知。
    //
    // ⚠️ 可达性的完整链路（少一环都拿不到工具，别只盯着这里）：
    //   设置页打开「场景」→ `ToolSettings::sync_to_permissions` 把这三个名字写进
    //   **default 角色组**并置 `enabled = true` → `permissions::allowed_tools()`
    //   场景组（UserChat → scene_admin，all_tools=true）与该角色组取交集才非空。
    //   默认角色组 `enabled = false` 时 `allowed_tools()` 直接返回空集（官方既有行为），
    //   所以「用户在设置页开一次场景组」是这三个工具唯一的默认可达路径。
    (
        "scene",
        &[
            "scene_list",
            "scene_switch",
            "get_my_location",
            "get_nearby_facilities",
            "move_to",
        ],
    ),
    ("status", &["status_get_current", "status_get_scene"]),
    ("clock", &["get_current_time"]),
    ("skills", &["list_skills", "read_skill"]),
    (
        "file_ops",
        &[
            "list_files",
            "read_file",
            "write_file",
            "delete_file",
            "edit_file",
            "search_files",
            "grep_files",
        ],
    ),
    ("command", &["execute_command"]),
];

/// 网页搜索工具配置。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct WebSearchSettings {
    /// 总开关：关闭时工具不下发给模型，执行也会被拒绝。
    pub enabled: bool,
    /// 为 true 时使用「模型 API 内置联网」：复用聊天模型的 API（Moonshot/Kimi），
    /// 由服务端执行 $web_search，无需单独的搜索 API Key；
    /// 为 false 时使用独立搜索端点 + api_key。
    pub use_builtin: bool,
    /// 独立端点模式的搜索服务提供商：
    /// "kimi"（Kimi Code 同款 /v1/search，body 为 text_query）
    /// "bocha"（BoCha 博查 https://api.bochaai.com/v1/web-search）
    /// 仅在 use_builtin = false 时生效。
    pub provider: String,
    /// API Key（Bearer 认证，仅 use_builtin = false 时需要）。
    pub api_key: String,
    /// 搜索端点（仅 use_builtin = false 时使用）。
    pub base_url: String,
    /// 是否通过本地 HTTP 代理（如 v2rayN）访问搜索端点。
    pub proxy_enabled: bool,
    /// 代理地址，v2rayN（sing-box）默认本地端口 10808。
    pub proxy_addr: String,
    /// 返回给模型的最大结果条数（仅独立端点模式）。
    pub max_results: usize,
    /// 为 true 时喂给模型的搜索结果不含网址/来源名，并指示模型
    /// 把信息自然融入回答，避免在对话中念出搜索结果列表。
    pub hide_search_results: bool,
}

impl Default for WebSearchSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            use_builtin: true,
            provider: "kimi".to_string(),
            api_key: String::new(),
            base_url: "https://api.kimi.com/coding/v1/search".to_string(),
            proxy_enabled: false,
            proxy_addr: "http://127.0.0.1:10808".to_string(),
            max_results: 8,
            hide_search_results: false,
        }
    }
}

impl WebSearchSettings {
    /// 配置是否达到可下发给模型的就绪状态。
    pub fn is_ready(&self) -> bool {
        self.enabled && (self.use_builtin || !self.api_key.trim().is_empty())
    }
}

/// 工具配置根。
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ToolSettings {
    pub web_search: WebSearchSettings,
    /// 分组开关：组名（见 `TOOL_GROUPS`）→ 是否启用，缺省关闭。
    pub groups: std::collections::HashMap<String, bool>,
    /// 命令执行：免审批直接运行 shell（危险，仅在信任当前角色/模型时开启）。
    pub command_auto_approve: bool,
    /// 命令执行：识别到删除操作时免审批继续执行（危险；缺省 false）。
    pub command_delete_auto_approve: bool,
    /// 删除文件：免审批直接删除（危险；缺省 false，旧配置升级后仍会弹窗）。
    pub file_delete_auto_approve: bool,
    /// 文件操作：允许访问文件沙箱（默认 data/）之外的任意路径。
    pub file_ops_allow_any_path: bool,
}

impl ToolSettings {
    /// 把用户配置同步到权限矩阵的 default 角色组。
    pub fn sync_to_permissions(&self, permissions: &mut ToolPermissionConfig) {
        permissions.set_tool_allowed_for_default_group("web_search", self.web_search.is_ready());
        for (group, tools) in TOOL_GROUPS {
            let enabled = self.groups.get(*group).copied().unwrap_or(false);
            for tool in *tools {
                permissions.set_tool_allowed_for_default_group(tool, enabled);
            }
        }
    }
}

impl ToolSettings {
    /// 加载配置；文件不存在时写入一份默认配置。
    pub fn load_or_create(data_dir: &Path) -> Result<Self> {
        let path = data_dir.join(SETTINGS_FILE_NAME);
        if path.exists() {
            let text = fs::read_to_string(&path)
                .with_context(|| format!("读取工具配置失败: {}", path.display()))?;
            return toml::from_str(&text)
                .with_context(|| format!("解析工具配置失败: {}", path.display()));
        }
        let settings = Self::default();
        settings.save(data_dir)?;
        Ok(settings)
    }

    /// 原子写入 `data/tool_settings.toml`。
    pub fn save(&self, data_dir: &Path) -> Result<()> {
        let path = data_dir.join(SETTINGS_FILE_NAME);
        let text = toml::to_string_pretty(self).context("序列化工具配置失败")?;
        super::atomic_replace(&path, text.as_bytes())
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("保存工具配置失败: {}", path.display()))?;
        Ok(())
    }
}

/// 在线程间共享、可热更新的工具配置句柄。
#[derive(Clone)]
pub struct SharedToolSettings(Arc<RwLock<ToolSettings>>);

impl SharedToolSettings {
    pub fn new(settings: ToolSettings) -> Self {
        Self(Arc::new(RwLock::new(settings)))
    }

    /// 读取当前配置快照。
    pub fn get(&self) -> ToolSettings {
        self.0.read().expect("工具配置锁已中毒").clone()
    }

    /// 整体替换配置，立即对所有工具生效。
    pub fn update(&self, settings: ToolSettings) {
        *self.0.write().expect("工具配置锁已中毒") = settings;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_settings_keep_delete_confirmation_enabled() {
        let legacy = r#"
command_auto_approve = false
file_ops_allow_any_path = false
"#;
        let settings: ToolSettings = toml::from_str(legacy).unwrap();
        assert!(!settings.command_delete_auto_approve);
        assert!(!settings.file_delete_auto_approve);
    }

    #[test]
    fn save_can_replace_existing_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = ToolSettings::default();
        settings.save(dir.path()).unwrap();
        settings.command_auto_approve = true;
        settings.save(dir.path()).unwrap();

        let loaded = ToolSettings::load_or_create(dir.path()).unwrap();
        assert!(loaded.command_auto_approve);
    }

    // ══════════════════════════════════════════════════════════════
    //  P3-4：世界模拟的三个地图工具必须**默认可达**
    //
    //  可达性不是本文件一个 pin 说了算的：设置页开关 → `sync_to_permissions`
    //  → 权限矩阵「场景组 × 角色组」交集。所以这里跑的是**整条链路**：
    //  用真的 `ToolPermissionConfig` 算 `allowed_tools()`，而不是只查数组里有没有名字。
    // ══════════════════════════════════════════════════════════════

    use crate::ai_service::message_system::generator::GeneratorSource;
    use crate::ai_service::tools::permissions::{
        GroupPermission, ToolPermissionConfig, DEFAULT_ROLE_GROUP,
    };
    use std::collections::HashSet;

    /// 三个世界模拟工具（顺序固定，断言失败时好看）
    const MAP_TOOLS: [&str; 3] = ["get_my_location", "get_nearby_facilities", "move_to"];

    /// 全量工具注册名（`allowed_tools` 的 `all_names` 入参）
    fn all_tool_names() -> HashSet<String> {
        TOOL_GROUPS
            .iter()
            .flat_map(|(_, tools)| tools.iter().map(|t| t.to_string()))
            .chain(std::iter::once("web_search".to_string()))
            .collect()
    }

    /// 用户把某个组打开后的权限矩阵。
    ///
    /// 从**真实默认矩阵**（`with_default_tools`）起步，再走一遍初始化时那条路：
    /// 角色落进 `default` 角色组（`enabled = false`）→ `sync_to_permissions`
    /// 按设置页开关写权限。这样算出来的就是用户真会拿到的那份。
    fn config_with_group(group: &str, on: bool) -> ToolPermissionConfig {
        let mut settings = ToolSettings::default();
        settings.groups.insert(group.to_string(), on);
        let mut perms = ToolPermissionConfig::with_default_tools(all_tool_names());
        perms
            .role_groups
            .entry(DEFAULT_ROLE_GROUP.to_string())
            .or_insert_with(GroupPermission::default)
            .roles
            .insert("小满".to_string());
        settings.sync_to_permissions(&mut perms);
        perms
    }

    /// 三个地图工具都在 scene 组里（前端设置页那个「场景」开关能带上它们）。
    #[test]
    fn map_tools_are_listed_under_the_scene_group() {
        let scene = TOOL_GROUPS
            .iter()
            .find(|(name, _)| *name == "scene")
            .map(|(_, tools)| *tools)
            .expect("scene 组必须存在");
        for tool in MAP_TOOLS {
            assert!(scene.contains(&tool), "{tool} 必须在 scene 组里: {scene:?}");
        }
        // 不许同时挂在别的组里：一个工具两个开关，用户关掉一个以为关干净了，其实没有
        for (name, tools) in TOOL_GROUPS {
            if *name == "scene" {
                continue;
            }
            for tool in MAP_TOOLS {
                assert!(!tools.contains(&tool), "{tool} 不该同时出现在 {name} 组");
            }
        }
    }

    /// 默认配置下三个工具拿不到 —— 这是**官方既有行为**（default 角色组 enabled = false），
    /// 记录下来，免得后人以为「装好就该能用」。
    #[test]
    fn default_settings_deny_every_tool_until_a_group_is_opened() {
        let perms = config_with_group("scene", false);
        let allowed = perms.allowed_tools(GeneratorSource::UserChat, Some("小满"), &all_tool_names());
        assert!(allowed.is_empty(), "默认角色组没启用时必须是空集: {allowed:?}");
        // 空配置（文件不存在时的那份）同样是空集
        let empty = ToolPermissionConfig::default();
        assert!(empty
            .allowed_tools(GeneratorSource::UserChat, Some("小满"), &all_tool_names())
            .is_empty());
    }

    /// 用户在设置页打开「场景」→ 三个地图工具真的下发给模型（整条链路）。
    #[test]
    fn opening_the_scene_group_makes_the_three_map_tools_reachable() {
        let perms = config_with_group("scene", true);
        let allowed = perms.allowed_tools(GeneratorSource::UserChat, Some("小满"), &all_tool_names());
        for tool in MAP_TOOLS {
            assert!(allowed.contains(tool), "打开场景组后 {tool} 必须可达: {allowed:?}");
        }
        // 同组的 scene_list/switch 当然也在
        assert!(allowed.contains("scene_list"));
        // 没开的组不许漏进来
        assert!(!allowed.contains("execute_command"), "没开的组不许漏: {allowed:?}");
        assert!(!allowed.contains("read_file"), "没开的组不许漏: {allowed:?}");
    }

    /// 「场景」关着时三个工具一个都不给（开关是双向的，不是只加不减）。
    #[test]
    fn closing_the_scene_group_takes_the_map_tools_away() {
        let perms = config_with_group("scene", false);
        let allowed = perms.allowed_tools(GeneratorSource::UserChat, Some("小满"), &all_tool_names());
        for tool in MAP_TOOLS {
            assert!(!allowed.contains(tool), "关掉场景组后 {tool} 不该可达: {allowed:?}");
        }
    }

    /// 开别的组不会顺带把地图工具放出来（它们只归 scene 组管）。
    #[test]
    fn other_groups_never_smuggle_the_map_tools_in() {
        for group in ["schedule", "memory", "character", "status", "clock", "skills", "file_ops", "command"] {
            let perms = config_with_group(group, true);
            let allowed =
                perms.allowed_tools(GeneratorSource::UserChat, Some("小满"), &all_tool_names());
            for tool in MAP_TOOLS {
                assert!(
                    !allowed.contains(tool),
                    "打开 {group} 组不该让 {tool} 可达: {allowed:?}"
                );
            }
        }
    }

    /// 主动搭话（Proactive → scene_normal）也走同一条路：地图工具在那边同样可达。
    #[test]
    fn proactive_source_also_sees_the_map_tools() {
        let perms = config_with_group("scene", true);
        let allowed = perms.allowed_tools(GeneratorSource::Proactive, Some("小满"), &all_tool_names());
        for tool in MAP_TOOLS {
            assert!(allowed.contains(tool), "Proactive 下 {tool} 也必须可达: {allowed:?}");
        }
    }
}
