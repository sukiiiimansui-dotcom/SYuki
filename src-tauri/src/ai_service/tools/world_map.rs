//! 世界模拟 · 聊天工具（P3）：`get_my_location` / `get_nearby_facilities` / `move_to`
//!
//! ## 为什么照 `scene.rs` 写
//! 这三个工具走的是**主对话工具系统**（路 B，架构文档 §4.4 的结论）：
//! `tools/executor.rs::Tool` trait + `tools/mod.rs::built_in_registry` 注册一行，
//! 权限矩阵（`UserChat → scene_admin → all_tools:true`）自动放行，
//! 不需要动 `lib.rs` / capability / manifest 任何一处。
//!
//! ## 铁律：工具里**不许**发网络、不许跑 AI 绘制
//! `ToolExecutor` 默认给每个工具 **2 秒**超时（`tools/executor.rs:89`），
//! 而地图的任何一个网络动作（Overpass / wttr.in / 逆地理）都可能是几十秒。
//! 所以三个工具**只读** [`crate::world_map::state`] 里那份进程内缓存状态：
//!
//! | 工具 | 读什么 | 谁负责把数据放进去 |
//! |---|---|---|
//! | `get_my_location` | `scene` + `actors[current_role]` | 前端地图页 `world_map_update_runtime` |
//! | `get_nearby_facilities` | `facilities` + 角色/玩家坐标 | 同上（设施表是 `facilities.rs` 生成的） |
//! | `move_to` | 只**写**一条 `pending_move` 意图 | 前端读走后做动画，落位后再推 `actors` |
//!
//! 缓存是空的（世界模拟没开）时三个工具都**明确报错**、绝不编造位置：
//! 让模型知道"现在没有地图信息"，比塞给它一个假地址安全得多。
//!
//! ## 命名
//! 工具名 snake_case、全库唯一（`ToolRegistry::register` 重名会返回
//! `RegistryError::DuplicateName`，启动即失败，所以名字必须一次定死）。
//! 本模块的 `world_map` 与 `crate::world_map`（地图后端）**同名但不是一回事**，
//! 所以下面一律用 `crate::world_map::…` 全路径引用，避免看串。

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::ai_service::types::ToolDefinition;
use crate::world_map::summary;
use crate::world_map::state;

use super::executor::{Tool, ToolContext, ToolError, ToolResult};

/// `get_nearby_facilities` 默认返回几条 / 最多几条。
/// 上限卡死是给 system 提示与 LLM 上下文留余地：一屏列 100 个设施没有意义。
const NEARBY_DEFAULT_LIMIT: usize = 5;
const NEARBY_MAX_LIMIT: usize = 20;

/// 14 类设施的 key → 中文名（与 `facilities.rs::ALL_TYPES` 的 `zh` 字段同源）。
/// 放在工具描述里让模型知道能按什么过滤；对不上时按**原样**匹配 `type`/`group`，
/// 所以模型给中文名或任意 key 都不会炸，只是可能筛不出东西。
const TYPE_HINT: &str = "住宅=residential、商业=commercial、教育=education、医疗=medical、\
休闲娱乐=leisure、市政服务=civic、住宿=lodging、公交站=bus_stop、地铁站=subway、\
停车场=parking、加油站=gas、机场=airport、火车站=train_station、码头=pier";

/// 把 `type` 参数规整成一组小写字符串。
///
/// 参数在 schema 里声明为 **string**（不是数组）：`oneOf` / 联合类型在部分
/// 供应商的 function calling 实现里会被拒（各家对 JSON Schema 子集的支持不一样），
/// 而执行器又只按顶层 `type` 校验参数 —— 声明成数组就意味着模型传单个字符串会被
/// 直接判成非法参数。所以约定"多个类型用逗号分隔"，这里顺带把
/// `,` / `，` / `、` 都当分隔符，并且**仍然容忍**真数组（老调用方/上帝 agent 传的）。
fn type_filter(value: Option<&Value>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        let s = s.trim().to_lowercase();
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    };
    match value {
        Some(Value::String(s)) => {
            for part in s.split([',', '，', '、', '|']) {
                push(part);
            }
        }
        Some(Value::Array(a)) => {
            for v in a {
                if let Some(s) = v.as_str() {
                    for part in s.split([',', '，', '、', '|']) {
                        push(part);
                    }
                }
            }
        }
        _ => {}
    }
    out
}

/// 一条设施是否命中过滤条件。
///
/// 认四种写法：`type` key（`subway`）、`group`（`life` / `transport`）、
/// 中文名（`type_zh`：`地铁站`），以及名字里的子串（`咖啡`）。
/// 前三者精确匹配、最后一个模糊匹配 —— 模型嘴里的"咖啡馆"未必等于生成器起的
/// "星洲咖啡"，模糊一点才用得上。
fn facility_matches(facility: &Value, filters: &[String]) -> bool {
    if filters.is_empty() {
        return true;
    }
    let type_key = summary::str_of(facility, "type").unwrap_or_default().to_lowercase();
    let group = summary::str_of(facility, "group").unwrap_or_default().to_lowercase();
    let zh = summary::str_of(facility, "type_zh").unwrap_or_default();
    let name = summary::str_of(facility, "name").unwrap_or_default();
    filters.iter().any(|f| {
        type_key == *f || group == *f || zh == *f || (!name.is_empty() && name.contains(f.as_str()))
    })
}

/// 当前该把谁当成"我"：优先参数，其次 `current_role`（前端推的正在说话的角色）。
fn resolve_role(runtime_role: &Option<String>, argument: Option<&Value>) -> Option<String> {
    argument
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| runtime_role.clone())
}

/// `get_my_location` —— 角色此刻在地图的哪个位置。
pub struct GetMyLocation;

#[async_trait]
impl Tool for GetMyLocation {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "get_my_location",
            "查看你自己此刻在地图上的位置：所在场景路径（市·区·小区）与所在设施，\
以及你是什么时候到这里的。只读取已经同步好的地图状态，不会联网。",
            json!({
                "type": "object",
                "properties": {
                    "role": {
                        "type": "string",
                        "description": "要查的角色名；不填就查当前正在说话的角色"
                    }
                },
                "required": [],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(
        &self,
        _context: &ToolContext,
        arguments: Value,
    ) -> Result<ToolResult, ToolError> {
        let Some(obj) = arguments.as_object() else {
            return Err(ToolError::InvalidArguments(
                "get_my_location 参数必须是 JSON object".into(),
            ));
        };
        let runtime = state::shared();
        let guard = runtime.read().await;

        let role = resolve_role(&guard.current_role, obj.get("role"));
        let scene = guard.scene.as_ref().unwrap_or(&Value::Null);
        let area = summary::scene_label(scene);
        let actor = role
            .as_deref()
            .map(|r| summary::actor_pos(&guard.actors, r))
            .unwrap_or_default();
        let place = actor
            .place
            .clone()
            .or_else(|| summary::str_of(scene, "place"))
            .or_else(|| guard.me.as_ref().and_then(|m| summary::me_label(m)));

        if area.is_none() && place.is_none() {
            return Err(ToolError::Execution(
                "现在没有地图位置信息：世界模拟还没打开，或者地图页还没把当前场景同步过来".into(),
            ));
        }

        // 契约形状固定成 {area, place, since}（三键），缺的给 null，
        // 别自创字段 —— 前端与注入都按这三个键读。
        Ok(json!({
            "area": area,
            "place": place,
            "since": actor.since,
        }))
    }
}

/// `get_nearby_facilities` —— 附近有什么。
pub struct GetNearbyFacilities;

#[async_trait]
impl Tool for GetNearbyFacilities {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "get_nearby_facilities",
            "查看你附近的生活/交通设施，返回名称、类型与直线距离（米），按距离从近到远。\
可按类型过滤（type）、限制半径（radius_m）与条数（limit）。\
只读取已经同步好的地图状态，不会联网。",
            json!({
                "type": "object",
                "properties": {
                    "type": {
                        "type": "string",
                        "description": format!("设施类型过滤，多个用逗号分隔（例如 \"subway\" 或 \"commercial,leisure\"）。{TYPE_HINT}")
                    },
                    "radius_m": {
                        "type": "number",
                        "description": "只返回这个半径（米）以内的设施，例如 500"
                    },
                    "limit": {
                        "type": "integer",
                        "description": format!("最多返回几条，默认 {NEARBY_DEFAULT_LIMIT}，上限 {NEARBY_MAX_LIMIT}")
                    },
                    "role": {
                        "type": "string",
                        "description": "以谁的位置为中心；不填就用当前正在说话的角色（再退到玩家位置）"
                    }
                },
                "required": [],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(
        &self,
        _context: &ToolContext,
        arguments: Value,
    ) -> Result<ToolResult, ToolError> {
        let Some(obj) = arguments.as_object() else {
            return Err(ToolError::InvalidArguments(
                "get_nearby_facilities 参数必须是 JSON object".into(),
            ));
        };
        let filters = type_filter(obj.get("type"));
        let radius = obj.get("radius_m").and_then(Value::as_f64);
        let limit = obj
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| (n as usize).clamp(1, NEARBY_MAX_LIMIT))
            .unwrap_or(NEARBY_DEFAULT_LIMIT);

        let runtime = state::shared();
        let guard = runtime.read().await;

        let role = resolve_role(&guard.current_role, obj.get("role"));
        let actor = role
            .as_deref()
            .map(|r| summary::actor_pos(&guard.actors, r))
            .unwrap_or_default();

        // 中心点：角色格点 → 玩家格点 → 玩家经纬度（三种都认，能算哪个用哪个）
        let actor_xy = (actor.x, actor.y);
        let me_grid = guard
            .me
            .as_ref()
            .map(summary::cell_xy)
            .unwrap_or((None, None));
        let has_actor_grid = actor_xy.0.is_some() && actor_xy.1.is_some();
        let has_me_grid = me_grid.0.is_some() && me_grid.1.is_some();
        let origin = if has_actor_grid { actor_xy } else { me_grid };
        let me_geo = guard
            .me
            .as_ref()
            .map(summary::lng_lat)
            .unwrap_or((None, None));

        // 格点路线：设施本来就是按格摆的，距离最准
        let mut items: Vec<(String, String, f64)> = Vec::new();
        if has_actor_grid || has_me_grid {
            let exclude = if has_actor_grid {
                actor.place.as_deref()
            } else {
                None
            };
            // 先按类型筛出候选、再交给 `summary::nearby` 排序，
            // 最后才 `truncate(limit)` —— 顺序反了会变成"先截断再过滤"，
            // 漏掉稍远但符合类型的那几个（模型问"最近的医院"就会得到空）。
            let selected: Vec<Value> = guard
                .facilities
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|f| facility_matches(f, &filters))
                .collect();
            let pool = summary::nearby(
                &Value::Array(selected),
                origin,
                exclude,
                guard.cell_m,
                usize::MAX,
            );
            for item in pool {
                if radius.is_some_and(|r| item.dist_m > r) {
                    continue;
                }
                items.push((item.name, item.kind, item.dist_m));
                if items.len() >= limit {
                    break;
                }
            }
        } else if me_geo.0.is_some() {
            // 退化路线：只有玩家经纬度时按经纬度算（设施也带 lat/lng 才行）
            let list = guard.facilities.as_array().cloned().unwrap_or_default();
            for f in list {
                if !facility_matches(&f, &filters) {
                    continue;
                }
                let Some(d) = summary::geo_distance_m(me_geo, summary::lng_lat(&f)) else {
                    continue;
                };
                if radius.is_some_and(|r| d > r) {
                    continue;
                }
                items.push((
                    summary::str_of(&f, "name").unwrap_or_default(),
                    summary::str_of(&f, "type").unwrap_or_default(),
                    d,
                ));
            }
            items.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
            items.truncate(limit);
        }

        if items.is_empty() {
            return Err(ToolError::Execution(
                "算不出附近的设施：地图页还没把设施表和你当前的位置同步过来（或者这个范围内确实什么都没有）".into(),
            ));
        }

        Ok(json!({
            "items": items
                .into_iter()
                .map(|(name, kind, dist_m)| json!({
                    "name": name,
                    "type": kind,
                    "dist_m": dist_m.round() as i64,
                }))
                .collect::<Vec<_>>()
        }))
    }
}

/// `move_to` —— 移动到某地（**只记录意图**，动画由前端做）。
pub struct MoveTo;

#[async_trait]
impl Tool for MoveTo {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "move_to",
            "决定去某个地方（例如「咖啡馆」「地铁站」「回家」）。\
这里只登记你的移动意图，地图上真正走过去的过程由前端按现实速度播放；\
调用后不要自己描述\"已经到了\"，到没到要看地图状态。",
            json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "目的地：附近设施的名字，或者\"家\"\"公司\"这类地点"
                    },
                    "kind": {
                        "type": "string",
                        "description": "出行方式（可选）：walk/bike/bus/subway/taxi/car/train/plane/ferry"
                    }
                },
                "required": ["target"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(
        &self,
        _context: &ToolContext,
        arguments: Value,
    ) -> Result<ToolResult, ToolError> {
        let Some(obj) = arguments.as_object() else {
            return Err(ToolError::InvalidArguments(
                "move_to 参数必须是 JSON object".into(),
            ));
        };
        let target = obj
            .get("target")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ToolError::InvalidArguments("move_to 需要非空的 target".into()))?;
        let kind = obj
            .get("kind")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());

        // 谁在移动：`current_role`（前端推的当前发言人）。拿不到就记成匿名，
        // 前端可以按当前角色兜底 —— 总比不记强。
        let by = state::shared()
            .try_read()
            .ok()
            .and_then(|g| g.current_role.clone());

        if !state::record_move_intent(target, by.as_deref(), kind) {
            return Err(ToolError::InvalidArguments(
                "move_to 的目标地点不能为空".into(),
            ));
        }

        Ok(json!({"ok": true, "target": target}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 类型过滤：key / 中文名 / 组名 / 名字子串都能命中。
    #[test]
    fn type_filter_matches_keys_groups_and_names() {
        let f = json!({"name": "星洲咖啡馆", "type": "commercial", "type_zh": "商业", "group": "life"});
        assert!(facility_matches(&f, &["commercial".into()]));
        assert!(facility_matches(&f, &["商业".into()]));
        assert!(facility_matches(&f, &["life".into()]));
        assert!(facility_matches(&f, &["咖啡".into()]));
        assert!(!facility_matches(&f, &["subway".into()]));
        // 空过滤 = 全通过
        assert!(facility_matches(&f, &[]));
    }

    /// `type` 参数：逗号分隔、真数组、空白项都吃。
    #[test]
    fn type_argument_accepts_string_and_array() {
        assert_eq!(type_filter(Some(&json!("subway"))), vec!["subway".to_string()]);
        assert_eq!(
            type_filter(Some(&json!("commercial,leisure"))),
            vec!["commercial".to_string(), "leisure".to_string()]
        );
        assert_eq!(
            type_filter(Some(&json!("subway、bus_stop，地铁站"))),
            vec!["subway".to_string(), "bus_stop".to_string(), "地铁站".to_string()]
        );
        assert_eq!(
            type_filter(Some(&json!(["subway", "商业", " subway ", ""]))),
            vec!["subway".to_string(), "商业".to_string()]
        );
        assert!(type_filter(None).is_empty());
        assert!(type_filter(Some(&json!(3))).is_empty());
    }

    /// 角色解析：参数优先，其次运行时 current_role，都没有就是 None。
    #[test]
    fn role_resolution_prefers_argument() {
        let current = Some("小满".to_string());
        assert_eq!(resolve_role(&current, Some(&json!("阿离"))), Some("阿离".to_string()));
        assert_eq!(resolve_role(&current, None), Some("小满".to_string()));
        assert_eq!(resolve_role(&current, Some(&json!("  "))), Some("小满".to_string()));
        assert_eq!(resolve_role(&None, None), None);
    }

    /// 工具定义合法性与唯一性：名字 snake_case、参数是 object、
    /// `additionalProperties: false`（执行器会按这份 schema 校验参数）。
    #[test]
    fn definitions_are_well_formed() {
        let tools: Vec<(String, Value)> = vec![
            (GetMyLocation.definition().function.name, GetMyLocation.definition().function.parameters),
            (GetNearbyFacilities.definition().function.name, GetNearbyFacilities.definition().function.parameters),
            (MoveTo.definition().function.name, MoveTo.definition().function.parameters),
        ];
        let mut names: Vec<&str> = Vec::new();
        for (name, params) in &tools {
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "工具名必须是 snake_case: {name}"
            );
            assert_eq!(params["type"], json!("object"));
            assert_eq!(params["additionalProperties"], json!(false));
            assert!(!names.contains(&name.as_str()), "工具名重复: {name}");
            names.push(name.as_str());
        }
        assert_eq!(names, vec!["get_my_location", "get_nearby_facilities", "move_to"]);
        // move_to 的 target 必填 —— 执行器会替我们拦缺参调用
        assert_eq!(
            MoveTo.definition().function.parameters["required"],
            json!(["target"])
        );
    }
}
