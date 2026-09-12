//! 地图存档书签（P3-1）：让「这张地图」跟着对话存档一起走。
//!
//! ## 为什么是一份"书签"而不是把整张地图塞进存档
//! 地图本体（SVG / 布局 JSON / 设施表）在 `maplib` 与 `facilities` 里，
//! 容量以 MB 计；存档里只需要记住**去哪张地图**就够了：
//!   · `area`        —— 注入与前端都在用的那串「广州市·越秀区·东山口」
//!   · `adcodes`     —— adcode 链路（行政区划渲染 / geo 缓存命中都靠它）
//!   · `place`       —— 小区名（注入里「你在：…·东山口」那一截）
//!   · `seed`        —— 小区布局的稳定种子（同一个小区重进必须长一个样）
//!   · `maplib_key`  —— 地图库里的 key（布局缓存 / 索引条目）
//!
//! ## 零迁移
//! 字段全部 `#[serde(default)]` + 在存档快照里是 `Option<…>`：
//! 老存档（`save.status` 里根本没有 `world_map` 键）读出来就是 `None`，
//! 不做任何迁移、不写任何回填。新存档读进老版本也就是多一个被忽略的键。
//!
//! ## 为什么这个文件不 import tauri
//! 它是**纯函数模块**（`Value` 进 `Value` 出），所以能在手机上脱离工程
//! `rustc --test` 真跑单测 —— 存档兼容性这种"错了就丢用户数据"的地方，
//! 不该只有"我看了一遍代码"这一种验证方式。
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// 存档里的地图上下文（`GameStatusSnapshot.world_map`）。
///
/// 所有字段都可缺省：老存档没有它 → `None`；只有 `{}` → 全部默认值
/// （`is_empty()` 为 true，等于没有地图）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldMapBookmark {
    /// 场景文案（`scene.area`，已经是人话：「广州市·越秀区·东山口」）
    pub area: Option<String>,
    /// adcode 链路（国·省·市·区县；前端目前推单个 `adcode`，这里统一成链路）
    pub adcodes: Vec<String>,
    /// 小区名（`scene.place`）
    pub place: Option<String>,
    /// 小区布局种子（稳定：同一个 area 每次导出都一样）
    pub seed: Option<u64>,
    /// 地图库 key（布局缓存 key / 索引条目 id）
    pub maplib_key: Option<String>,
    /// 记录时刻（unix 秒，排障用；不参与任何判定）
    pub at: Option<u64>,
}

impl WorldMapBookmark {
    /// 一条地图信息都没有（`{}` 反序列化出来的就是这种）。
    pub fn is_empty(&self) -> bool {
        self.area.is_none()
            && self.adcodes.is_empty()
            && self.place.is_none()
            && self.seed.is_none()
            && self.maplib_key.is_none()
    }

    /// 从 `MapRuntime::to_json` 的快照里抽一份书签（纯函数，不联网、不读盘）。
    ///
    /// 没有 `scene`（= 世界模拟没开）就返回 `None`：不在存档里写一个空的
    /// `world_map: {}`，免得下次读档看起来"像是有地图"。
    pub fn from_runtime(snapshot: &Value) -> Option<Self> {
        let mut bm = Self::from_scene(snapshot.get("scene").unwrap_or(&Value::Null))?;
        bm.at = snapshot
            .get("updated_at")
            .and_then(Value::as_u64)
            .filter(|t| *t > 0);
        Some(bm)
    }

    /// 从 `scene` 对象里抽书签：`area` 是唯一必需项。
    pub fn from_scene(scene: &Value) -> Option<Self> {
        if !scene.is_object() {
            return None;
        }
        let area = str_field(scene, "area");
        let adcodes = adcodes_of(scene);
        let place = str_field(scene, "place");
        let maplib_key = str_field(scene, "maplib_key").or_else(|| str_field(scene, "layout_key"));
        // 种子：前端推了就用它，没推就按 area 现推 —— 与 `bridge.rs` 生成设施表
        // 用的是**同一个** `area_seed()`，所以读档后同一个小区里还是那批店。
        let seed = scene
            .get("seed")
            .and_then(Value::as_u64)
            .or_else(|| area.as_deref().map(|a| area_seed(a) as u64));

        if area.is_none() && adcodes.is_empty() && place.is_none() && maplib_key.is_none() {
            return None;
        }
        Some(Self {
            area,
            adcodes,
            place,
            seed,
            maplib_key,
            at: None,
        })
    }

    /// 恢复地图用的 `world_map_update_runtime` patch（**纯函数**）。
    ///
    /// 只写 `scene` 一块：`apply_patch` 是字段级合并，推这一块不会碰
    /// 玩家位置 / 角色位置 / 天气 / 事件（那些由前端在一次会话里自己推）。
    /// 没有可恢复的内容时返回 `{}`（空 patch —— 调用方可以直接丢掉）。
    ///
    /// 为什么同时写 `adcode` 与 `adcodes`：前端 `wsRuntimePush.buildPatch`
    /// 认的是单个 `adcode`（字符串），而渲染链路要的是整条链；两个键一起给，
    /// 新老读法都能用上。
    pub fn restore_patch(&self) -> Value {
        let mut scene = Map::new();
        if let Some(area) = self.area.as_deref().filter(|s| !s.trim().is_empty()) {
            scene.insert("area".into(), json!(area));
        }
        if let Some(place) = self.place.as_deref().filter(|s| !s.trim().is_empty()) {
            scene.insert("place".into(), json!(place));
        }
        if !self.adcodes.is_empty() {
            scene.insert("adcodes".into(), json!(self.adcodes));
            if let Some(last) = self.adcodes.last() {
                scene.insert("adcode".into(), json!(last));
            }
        }
        if let Some(seed) = self.seed {
            scene.insert("seed".into(), json!(seed));
        }
        if let Some(key) = self.maplib_key.as_deref().filter(|s| !s.trim().is_empty()) {
            scene.insert("maplib_key".into(), json!(key));
        }
        if scene.is_empty() {
            return json!({});
        }
        json!({ "scene": Value::Object(scene) })
    }
}

/// 区域名 → 稳定种子（FNV-1a 64 位，取正）。
///
/// 用它而不是 `rand::random()`：设施表一旦随机，同一个小区每次生成都换一批店名与
/// 摆放，AI 记忆里的「楼下便利店」下一轮就解析不到了（`move::resolve_destination`
/// 是按名字查设施表的）。可复现比"每次都不一样"重要得多。
///
/// 原来私有在 `bridge.rs`；P3-1 要把它写进存档，所以搬到这里作为**唯一实现**
/// （`bridge.rs` 改为调用本函数）—— 存档里记的种子必须与生成设施表时用的是同一个，
/// 两份实现迟早会走偏。
pub fn area_seed(area: &str) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in area.trim().as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (h >> 1) as i64
}

/// 取一个非空字符串字段（去首尾空白；空串当没有）。
fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 归一化 adcode 链路：认 `adcodes: ["440000","440100"]`、`adcodes: "440000,440100"`、
/// 以及前端实际在推的单个 `adcode: "440100"`。
///
/// 只保留纯数字项（geo 缓存的键就是 6 位数字），去重且保持原顺序
/// —— 链路顺序是「省 → 市 → 区」，排序会把它打乱。
fn adcodes_of(scene: &Value) -> Vec<String> {
    let mut raw: Vec<String> = Vec::new();
    match scene.get("adcodes") {
        Some(Value::Array(arr)) => {
            for v in arr {
                match v {
                    Value::String(s) => raw.push(s.clone()),
                    Value::Number(n) => raw.push(n.to_string()),
                    _ => {}
                }
            }
        }
        Some(Value::String(s)) => raw.extend(s.split([',', '，', '·', ' ']).map(str::to_string)),
        _ => {}
    }
    if let Some(single) = str_field(scene, "adcode") {
        raw.push(single);
    }
    let mut out: Vec<String> = Vec::new();
    for item in raw {
        let ad = item.trim();
        // 只收 6 位纯数字：geo 缓存的文件名、`geo::parent_of` 的上级推算、
        // `offline::coverage` 的命中判定全是 6 位口径 —— 收下一个 4 位的「4401」
        // 只会在读档后变成一个谁都命中不了、又谁都删不掉的脏值。
        if ad.len() != 6 || !ad.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if !out.iter().any(|x| x == ad) {
            out.push(ad.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 老存档兼容：`world_map` 键整个不存在 → `None`（`Option` + `serde(default)`）。
    #[test]
    fn a_save_without_the_key_reads_as_none() {
        #[derive(Deserialize)]
        struct Holder {
            #[serde(default)]
            world_map: Option<WorldMapBookmark>,
        }
        let old: Holder = serde_json::from_str(r#"{"background":"classroom","current_role_id":3}"#)
            .expect("老存档必须能直接读，零迁移");
        assert!(old.world_map.is_none());
    }

    /// `{}` / `null` 都不能变成"有地图"。
    #[test]
    fn empty_bookmark_shapes_never_look_like_a_map() {
        let empty: WorldMapBookmark = serde_json::from_str("{}").unwrap();
        assert!(empty.is_empty());
        assert!(WorldMapBookmark::default().is_empty());
        assert_eq!(empty.restore_patch(), json!({}));
        assert!(WorldMapBookmark::from_scene(&Value::Null).is_none());
        assert!(WorldMapBookmark::from_scene(&json!({})).is_none());
        assert!(WorldMapBookmark::from_scene(&json!("广州市")).is_none());
    }

    /// 前端现在推的 `scene`（`{area, place, adcode}`）能抽全。
    #[test]
    fn bookmark_reads_what_the_frontend_actually_pushes() {
        let scene = json!({"area": "广州市·越秀区·东山口", "place": "东山口", "adcode": "440104"});
        let bm = WorldMapBookmark::from_scene(&scene).expect("有 area 就有书签");
        assert_eq!(bm.area.as_deref(), Some("广州市·越秀区·东山口"));
        assert_eq!(bm.place.as_deref(), Some("东山口"));
        assert_eq!(bm.adcodes, vec!["440104".to_string()]);
        // 种子与设施表用的是同一个函数（可复现）
        assert_eq!(bm.seed, Some(area_seed("广州市·越秀区·东山口") as u64));
        assert!(!bm.is_empty());
    }

    /// adcode 链路：数组 / 逗号串 / 单个键，三种都给；去重、剔脏、保序。
    #[test]
    fn adcode_chain_is_normalized_and_ordered() {
        let scene = json!({"area": "广州", "adcodes": ["440000", "440100", "440100", "x", 440104]});
        let bm = WorldMapBookmark::from_scene(&scene).unwrap();
        assert_eq!(bm.adcodes, vec!["440000", "440100", "440104"]);

        let csv = json!({"area": "广州", "adcodes": "440000,440100"});
        assert_eq!(
            WorldMapBookmark::from_scene(&csv).unwrap().adcodes,
            vec!["440000", "440100"]
        );

        // 数组 + 单个 adcode 并存时，连接在链尾（前端两个键都给是允许的）
        let both = json!({"area": "广州", "adcodes": ["440000"], "adcode": "440104"});
        assert_eq!(
            WorldMapBookmark::from_scene(&both).unwrap().adcodes,
            vec!["440000", "440104"]
        );
        // 脏 adcode 一律丢掉（geo 缓存的键是 6 位数字，别的都命中不了）
        let dirty = json!({"area": "广州", "adcode": " 4401 "});
        assert!(WorldMapBookmark::from_scene(&dirty).unwrap().adcodes.is_empty());
    }

    /// 存档往返：写出去的 JSON 读回来必须一模一样（含 seed / maplib_key）。
    #[test]
    fn bookmark_survives_a_save_round_trip() {
        let scene = json!({
            "area": "广州市·越秀区·东山口",
            "place": "东山口",
            "adcodes": ["440000", "440100"],
            "maplib_key": "layout:abc123",
        });
        let bm = WorldMapBookmark::from_scene(&scene).unwrap();
        let text = serde_json::to_string(&bm).unwrap();
        let back: WorldMapBookmark = serde_json::from_str(&text).unwrap();
        assert_eq!(back, bm);
        assert_eq!(back.maplib_key.as_deref(), Some("layout:abc123"));
    }

    /// 缺字段的老书签（只有 area）也要能读，不能因为少了 seed/at 就整条失败。
    #[test]
    fn partial_bookmark_json_fills_the_rest_with_defaults() {
        let partial: WorldMapBookmark = serde_json::from_str(r#"{"area":"广州市"}"#).unwrap();
        assert_eq!(partial.area.as_deref(), Some("广州市"));
        assert!(partial.adcodes.is_empty());
        assert_eq!(partial.seed, None);
        assert_eq!(partial.at, None);
        // 未知字段（将来版本多写的）不能把读档搞失败
        let unknown: WorldMapBookmark =
            serde_json::from_str(r#"{"area":"广州市","weather_at_save":1}"#).unwrap();
        assert_eq!(unknown.area.as_deref(), Some("广州市"));
    }

    /// 恢复 patch：只写 scene、只写有值的键；空的给空 patch。
    #[test]
    fn restore_patch_carries_only_what_was_saved() {
        let bm = WorldMapBookmark {
            area: Some("广州市·越秀区".into()),
            adcodes: vec!["440000".into(), "440100".into()],
            place: Some("东山口".into()),
            seed: Some(42),
            maplib_key: Some("k1".into()),
            at: None,
        };
        let patch = bm.restore_patch();
        assert_eq!(patch["scene"]["area"], json!("广州市·越秀区"));
        assert_eq!(patch["scene"]["place"], json!("东山口"));
        assert_eq!(patch["scene"]["adcodes"], json!(["440000", "440100"]));
        // 前端读的是单个 adcode → 取链路最后一级（最具体的那一级）
        assert_eq!(patch["scene"]["adcode"], json!("440100"));
        assert_eq!(patch["scene"]["seed"], json!(42));
        assert_eq!(patch["scene"]["maplib_key"], json!("k1"));
        // 不碰其它任何键（apply_patch 是字段级合并，多写的键会被原样存下来）
        assert_eq!(patch.as_object().unwrap().len(), 1);

        let bare = WorldMapBookmark {
            area: Some("  ".into()),
            ..Default::default()
        };
        assert_eq!(bare.restore_patch(), json!({}));
    }

    /// 从完整 runtime 快照抽：没有 scene 就没有书签。
    #[test]
    fn from_runtime_needs_a_scene() {
        assert!(WorldMapBookmark::from_runtime(&json!({"scene": null, "me": {"area": "广州"}})).is_none());
        let bm = WorldMapBookmark::from_runtime(&json!({
            "scene": {"area": "广州市·越秀区·东山口"},
            "updated_at": 1_800_000_000u64,
        }))
        .unwrap();
        assert_eq!(bm.at, Some(1_800_000_000));
        // updated_at = 0（从没推过 patch）不当成时刻
        let bm2 = WorldMapBookmark::from_runtime(&json!({"scene": {"area": "广州"}})).unwrap();
        assert_eq!(bm2.at, None);
    }

    /// 种子稳定且与 bridge.rs 的实现同源（这里断言的是一致性，不是具体数值）。
    #[test]
    fn area_seed_is_stable_and_area_specific() {
        assert_eq!(area_seed("东山口"), area_seed("东山口"));
        assert_eq!(area_seed(" 东山口 "), area_seed("东山口"));
        assert_ne!(area_seed("东山口"), area_seed("东山口·西"));
        assert!(area_seed("东山口") >= 0, "取正：写进 u64 存档字段不能是负数");
    }
}
