//! `save.status` 里的场景快照（从 `game_status.rs` 拆出来的）。
//!
//! ## 为什么单独一个文件
//! 这个结构是**存档格式**本身：它的字段少一个、类型变一个，用户的存档就可能读不出来。
//! 原来它和 `GameStatus`（牵着 RoleManager / DatabaseConnection / AppState）挤在一个
//! 文件里，脱离整个工程根本编译不了 —— 于是"老存档还能不能读"只能靠肉眼看代码，
//! 而这条恰恰是最该有回归测试的地方。
//!
//! 拆出来之后它只依赖 serde + `world_map::bookmark`，可以在手机上
//! `rustc --test` 真跑（见 `~/chk/p6/shim.rs`）。`game_status.rs` 用
//! `pub use` 原样再导出，所有既有引用路径（`…::game_status::GameStatusSnapshot`）不变。
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::world_map::bookmark::WorldMapBookmark;

/// `GameStatus` 中需要持久化到 `save.status` JSON 的字段。
///
/// **加字段的规矩**：新字段一律 `#[serde(default)]`。老存档里没有这个键时
/// 必须能直接读出来（零迁移），新存档被老版本读到也只是多一个被忽略的键。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct GameStatusSnapshot {
    pub present_role_ids: Vec<i32>,
    pub current_role_id: Option<i32>,
    #[serde(default)]
    pub background: String,
    #[serde(default = "default_background_music")]
    pub background_music: String,
    #[serde(default = "default_background_effect")]
    pub background_effect: String,
    #[serde(default)]
    pub current_scene_id: Option<String>,
    #[serde(default)]
    pub global_variables: HashMap<String, Value>,
    #[serde(default)]
    pub completed_scripts: Vec<String>,
    pub last_dialog_time: Option<String>,
    #[serde(default = "default_true")]
    pub scene_awareness_enabled: bool,
    /// **世界模拟的地图上下文**（P3-1）：让「这张地图」跟着对话存档一起走。
    ///
    /// 存的是**书签**不是地图本体（`world_map::bookmark::WorldMapBookmark`：
    /// adcode 链路 / 小区名 / 小区 seed / 地图库 key）。地图本体在 maplib 与
    /// facilities 里，容量以 MB 计，塞进 `save.status` 只会把存档撑爆。
    ///
    /// `Option` + `#[serde(default)]` → **零迁移**：老存档没有这个键就是 `None`，
    /// 读档后地图保持空白（世界模拟本来就没开过）；新存档在老版本里被忽略。
    /// 注意这里**不能**写 `#[serde(default)] world_map: WorldMapBookmark`（非 Option）：
    /// 那样每份老存档读出来都会自带一个"空地图"，前端分不清"没开过"和"开了但空的"。
    #[serde(default)]
    pub world_map: Option<WorldMapBookmark>,
}

fn default_true() -> bool {
    true
}

fn default_background_music() -> String {
    "none".into()
}
fn default_background_effect() -> String {
    "none".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **老存档零迁移**：只有老字段的 JSON 必须能读出来，且新字段为 `None`。
    /// 这条挂了就意味着用户升级后读档直接失败。
    #[test]
    fn a_legacy_save_loads_without_the_world_map_key() {
        let legacy = r#"{
            "present_role_ids": [1, 2],
            "current_role_id": 1,
            "background": "classroom",
            "background_music": "none",
            "background_effect": "none",
            "current_scene_id": "s1",
            "global_variables": {"好感度": 3},
            "completed_scripts": ["intro"],
            "last_dialog_time": "2026-09-12T19:24:00+08:00",
            "scene_awareness_enabled": true
        }"#;
        let s: GameStatusSnapshot = serde_json::from_str(legacy).expect("老存档必须能读");
        assert!(s.world_map.is_none(), "老存档不该凭空长出一张地图");
        assert_eq!(s.present_role_ids, vec![1, 2]);
        assert_eq!(s.global_variables["好感度"], serde_json::json!(3));
    }

    /// 更老/更残缺的存档（连背景那几个键都没有）也要能读 —— 走各自的 default。
    #[test]
    fn a_minimal_save_falls_back_to_field_defaults() {
        // 只有 `present_role_ids` 是必填（它没有 serde(default)，老存档一直都在写它）；
        // 其余字段缺席时各走各的 default —— 包括新增的 `world_map`。
        let s: GameStatusSnapshot = serde_json::from_str(r#"{"present_role_ids":[]}"#).unwrap();
        assert_eq!(s.background_music, "none");
        assert_eq!(s.background_effect, "none");
        assert!(s.scene_awareness_enabled, "老存档缺这个键时按 true 处理");
        assert!(s.world_map.is_none());
        // `current_role_id` / `last_dialog_time` 是 Option：serde 对缺席的 Option 字段
        // 自动给 None（不需要 #[serde(default)]）—— 这一条也顺手钉住，
        // 免得以后有人把它们改成非 Option 又忘了加 default。
        assert_eq!(s.current_role_id, None);
        assert_eq!(s.last_dialog_time, None);
        let empty: GameStatusSnapshot = serde_json::from_str(r#"{"present_role_ids":[7]}"#).unwrap();
        assert_eq!(empty.present_role_ids, vec![7]);
        assert!(empty.world_map.is_none());
    }

    /// `world_map: null` / `{}` 都当"没有地图"，不能变成 `Some(空书签)` 之外的东西。
    #[test]
    fn explicit_null_or_empty_map_is_still_readable() {
        let null: GameStatusSnapshot =
            serde_json::from_str(r#"{"present_role_ids":[],"world_map":null}"#).unwrap();
        assert!(null.world_map.is_none());
        let empty: GameStatusSnapshot =
            serde_json::from_str(r#"{"present_role_ids":[],"world_map":{}}"#).unwrap();
        assert_eq!(empty.world_map.map(|b| b.is_empty()), Some(true));
    }

    /// 新存档往返：地图书签逐字段不丢（这正是"地图跟着存档走"要保证的事）。
    #[test]
    fn a_save_with_a_map_round_trips() {
        let bm = WorldMapBookmark {
            area: Some("广州市·越秀区·东山口".into()),
            adcodes: vec!["440000".into(), "440100".into(), "440104".into()],
            place: Some("东山口".into()),
            seed: Some(1234567),
            maplib_key: Some("layout:abc".into()),
            at: Some(1_800_000_000),
        };
        let mut snap = GameStatusSnapshot {
            present_role_ids: vec![1],
            current_role_id: Some(1),
            world_map: Some(bm.clone()),
            ..Default::default()
        };
        snap.background = "night_park".into();
        let text = serde_json::to_string(&snap).unwrap();
        let back: GameStatusSnapshot = serde_json::from_str(&text).unwrap();
        assert_eq!(back.world_map, Some(bm));
        assert_eq!(back.background, "night_park");
        // 键名是 snake_case（前端与老代码都按这个读）
        let raw: Value = serde_json::from_str(&text).unwrap();
        assert!(raw.get("world_map").is_some());
        assert_eq!(raw["world_map"]["adcodes"][2], serde_json::json!("440104"));
    }

    /// 存档里多出**未知**字段（将来版本写的）不能让读档失败。
    #[test]
    fn unknown_save_keys_are_ignored() {
        let s: GameStatusSnapshot = serde_json::from_str(
            r#"{"present_role_ids":[],"weather_at_save":"rain","world_map":{"area":"广州市"}}"#,
        )
        .unwrap();
        assert_eq!(s.world_map.unwrap().area.as_deref(), Some("广州市"));
    }
}
