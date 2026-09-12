//! schedule.rs — 日程系统（移植自 Python `schedule.py`，T4-5）
//!
//! 这个模块要解决的是**世界模拟的关键一环**：角色不能在地图上随机游走，
//! 必须「按日程出现在该去的地方」。所以它做两件事：
//!   1. 读 LingChat 的日程/待办/重要日子（没有文件时给默认日程，保证地图与手机永远有内容）
//!   2. 把日程文本翻译成**设施类型**（`classify_activity`），再由设施类型落到**具体设施点**
//!      （`role_place`）—— 同一个角色每次稳定命中同一处，不会每次刷新都跳来跳去。
//!
//! LingChat 侧的数据结构（Rust `UserScheduleSettings`，serde camelCase）：
//! `<data_dir>/game_data/schedules.json`
//! ```json
//! { "scheduleGroups": { "<组名>": { "title","description","items":[{"name","time","content"}] } },
//!   "todoGroups":     { "<组名>": { "title","description?","todos":[{...}] } },
//!   "importantDays":  [ { "id","date","title","desc?","cycle?" } ] }
//! ```
//! 兼容 snake_case（`schedule_groups` 等）是因为早期版本写过另一种拼法，读到就认。
//!
//! 为什么不用正则库：本工程为了离线编译只带必要依赖，时间解析与 settings.yml 解析
//! 都是手写扫描，行为与 Python 的 `re` 语义逐条对齐（含贪婪/回溯顺序），测试里有对照。

use serde_json::{json, Value};
use std::path::PathBuf;

/// 数据目录候选（按优先级）。`WM_LINGCHAT_DATA` 可覆盖，便于测试与部署。
pub fn data_dir() -> Option<PathBuf> {
    // 显式指定就**只认它**：指向不存在的路径即等于禁用发现（测试与部署都要能锁定数据源）
    if let Ok(d) = std::env::var("WM_LINGCHAT_DATA") {
        let p = PathBuf::from(d);
        return if p.join("game_data").is_dir() { Some(p) } else { None };
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let cands = [
        format!("{home}/lingchat-data"),
        format!("{home}/companion_new/lingchat-data"),
        format!("{home}/lingchat-main/data"),
        format!("{home}/rikka/Dsh-SYuki/lingchat-data"),
        format!("{home}/lingchat-main/src-tauri/target/debug/data"),
    ];
    for d in cands {
        let p = PathBuf::from(&d);
        if p.join("game_data").is_dir() {
            return Some(p);
        }
    }
    None
}

/// LingChat 的 schedules.json 路径（找不到数据目录则为 None）
pub fn schedules_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("game_data").join("schedules.json"))
}

/// 角色目录（头像与 settings.yml 都在这里）
pub fn characters_dir() -> Option<PathBuf> {
    data_dir().map(|d| d.join("game_data").join("characters"))
}

/// 活动 → 设施类型的映射。**顺序即优先级**：
/// 「通勤去公司」必须判成「在路上」而不是「到了公司」，所以 transit 排第一。
pub const ACTIVITY_MAP: [(&[&str], &str); 10] = [
    (&["通勤", "地铁", "公交", "坐车", "开车", "打车", "路上", "赶路", "车站", "机场", "出发", "赶往"], "transit"),
    (&["睡觉", "睡眠", "休息", "午休", "起床", "洗漱", "洗澡", "在家", "回家", "宅"], "residential"),
    (&["上班", "工作", "办公", "公司", "开会", "会议", "研究", "学术", "实验", "写论", "赶稿", "值班"], "commercial"),
    (&["早饭", "早餐", "午饭", "午餐", "晚饭", "晚餐", "吃饭", "用餐", "食堂", "餐厅", "咖啡", "喝茶", "下午茶", "外卖"], "commercial"),
    (&["买菜", "超市", "购物", "逛街", "商场", "买东"], "commercial"),
    (&["上课", "学习", "自习", "图书馆", "学校", "教室", "考试", "补课", "读书"], "education"),
    (&["看病", "医院", "体检", "买药", "复诊", "牙医"], "medical"),
    (&["运动", "健身", "跑步", "锻炼", "打球", "游泳", "散步", "公园", "娱乐", "看电影", "电影", "玩游戏", "唱歌", "逛街玩", "遛"], "leisure"),
    (&["办事", "政务", "银行", "邮局", "缴费", "派出所", "居委会"], "civic"),
    (&["出差", "旅游", "旅行", "住店", "酒店", "旅馆", "民宿"], "lodging"),
];

/// 设施类型的中文名（前端标签用）
pub fn type_zh(kind: &str) -> &'static str {
    match kind {
        "residential" => "住宅",
        "commercial" => "商业/办公",
        "education" => "教育",
        "medical" => "医疗",
        "leisure" => "休闲娱乐",
        "civic" => "市政服务",
        "lodging" => "住宿",
        "transit" => "在路上",
        _ => "",
    }
}

/// 日程文本 → 设施类型 key；认不出就当「在外面」（leisure）
pub fn classify_activity(text: &str) -> &'static str {
    for (keys, kind) in ACTIVITY_MAP {
        for k in keys {
            if text.contains(k) {
                return kind;
            }
        }
    }
    "leisure"
}

/// Python 的 `round(x, 3)`：这里用「乘 1000 再取整」近似。
/// 只在恰好 .0005 边界上与 Python 有半个最低位的差别，进度百分比展示无感。
fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn is_ascii_digit(c: char) -> bool {
    c.is_ascii_digit()
}

/// 手写复刻 Python `(\d{1,2})\s*[:：点时]\s*(\d{1,2})?` 的**搜索**语义：
/// 从每个位置起尝试（小时贪婪取 2 位，失败回溯成 1 位），取第一个匹配。
fn find_time_match(s: &str) -> Option<(i64, i64)> {
    let cs: Vec<char> = s.chars().collect();
    let n = cs.len();
    let seps = [':', '：', '点', '时'];
    for i in 0..n {
        if !is_ascii_digit(cs[i]) {
            continue;
        }
        // 小时：先试 2 位，再回退到 1 位（与正则贪婪+回溯一致）
        for hlen in [2usize, 1] {
            if i + hlen > n {
                continue;
            }
            if hlen == 2 && !is_ascii_digit(cs[i + 1]) {
                continue;
            }
            let h: i64 = cs[i..i + hlen].iter().collect::<String>().parse().unwrap_or(-1);
            let mut j = i + hlen;
            while j < n && cs[j].is_whitespace() {
                j += 1;
            }
            if j >= n || !seps.contains(&cs[j]) {
                continue;
            }
            j += 1;
            while j < n && cs[j].is_whitespace() {
                j += 1;
            }
            // 分钟：可选，贪婪取 2 位
            let mut mi = 0i64;
            if j < n && is_ascii_digit(cs[j]) {
                let take = if j + 1 < n && is_ascii_digit(cs[j + 1]) { 2 } else { 1 };
                mi = cs[j..j + take].iter().collect::<String>().parse().unwrap_or(0);
            }
            return Some((h, mi));
        }
    }
    None
}

/// `'08:30'` / `'8点30'` / `'9点'` / `'0830'` / 数字 `8` / `510` → 分钟数(0-1439)
pub fn parse_time(v: &Value) -> Option<i64> {
    match v {
        Value::Null => None,
        Value::Number(num) => {
            let val = num.as_f64()? as i64;
            // 日程里的数字几乎都是「小时」（8 表示 8 点）；>23 才当成分钟数（510 = 8:30）
            if (0..=23).contains(&val) {
                Some(val * 60)
            } else if (0..=1439).contains(&val) {
                Some(val)
            } else {
                None
            }
        }
        Value::String(s) => {
            if let Some((h, mi)) = find_time_match(s) {
                if (0..=23).contains(&h) && (0..=59).contains(&mi) {
                    return Some(h * 60 + mi);
                }
            }
            let t = s.trim();
            if (t.len() == 3 || t.len() == 4) && t.chars().all(is_ascii_digit) {
                let val: i64 = t.parse().ok()?;
                let (h, mi) = (val / 100, val % 100);
                if (0..=23).contains(&h) && (0..=59).contains(&mi) {
                    return Some(h * 60 + mi);
                }
            }
            None
        }
        _ => None,
    }
}

/// 分钟数 → `HH:MM`
pub fn fmt_min(m: i64) -> String {
    format!("{:02}:{:02}", m.div_euclid(60), m.rem_euclid(60))
}

/// 从 settings.yml 里抠一个 `key: value`（值去掉首尾空白与引号）
fn yml_scalar(txt: &str, key: &str) -> Option<String> {
    for line in txt.lines() {
        if let Some(rest) = line.strip_prefix(key) {
            if let Some(rest) = rest.strip_prefix(':') {
                let v = rest.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// 从 settings.yml 里抠 `info:` 后面的缩进块（`|` / `-` 之类的 YAML 标记都跳过）
fn yml_block(txt: &str) -> Option<String> {
    let lines: Vec<&str> = txt.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if !line.starts_with("info:") {
            continue;
        }
        let mut parts: Vec<String> = Vec::new();
        for l in lines.iter().skip(i + 1) {
            let indent = l.len() - l.trim_start().len();
            if l.trim().is_empty() {
                continue;
            }
            if indent < 2 {
                break;
            }
            parts.push(l.trim().to_string());
        }
        if !parts.is_empty() {
            let joined = parts.join(" ");
            return Some(joined.chars().take(200).collect());
        }
    }
    None
}

/// 列出 LingChat 角色（头像张数与简介都要，供 T4-3 头像映射与 T6-4 注入）
pub fn list_characters() -> Vec<Value> {
    let mut out = Vec::new();
    let cdir = match characters_dir() {
        Some(d) if d.is_dir() => d,
        _ => return out,
    };
    let mut folders: Vec<String> = std::fs::read_dir(&cdir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    folders.sort(); // 与 Python 的 sorted(os.listdir()) 一致，保证角色顺序稳定

    for folder in folders {
        let fp = cdir.join(&folder);
        let mut name = folder.clone();
        let mut subtitle = String::new();
        let mut info = String::new();
        if let Ok(txt) = std::fs::read_to_string(fp.join("settings.yml")) {
            if let Some(v) = yml_scalar(&txt, "ai_name") {
                name = v;
            }
            if let Some(v) = yml_scalar(&txt, "ai_subtitle") {
                subtitle = v;
            }
            if let Some(v) = yml_block(&txt) {
                info = v;
            }
        }
        let n = std::fs::read_dir(fp.join("avatar"))
            .map(|rd| rd.count())
            .unwrap_or(0);
        out.push(json!({
            "name": name, "folder": folder, "subtitle": subtitle,
            "avatarCount": n, "hasAvatar": n > 0, "info": info,
        }));
    }
    out
}

/// 默认日程模板（LingChat 还没设日程时用）
pub const DEFAULT_TEMPLATE: [(&str, &str, &str); 10] = [
    ("睡觉", "23:00", "在家睡觉"),
    ("起床", "07:30", "起床洗漱"),
    ("早餐", "08:00", "在家吃早餐"),
    ("通勤", "08:40", "通勤去公司"),
    ("工作", "09:00", "在公司上班"),
    ("午餐", "12:00", "出去吃午饭"),
    ("工作", "13:30", "下午继续工作"),
    ("下班", "18:00", "下班回家"),
    ("晚餐", "19:00", "在家吃晚饭"),
    ("休闲", "20:00", "在家休息或出门散步"),
];

fn default_items() -> Vec<Value> {
    DEFAULT_TEMPLATE
        .iter()
        .map(|(n, t, c)| json!({"name": n, "time": t, "content": c}))
        .collect()
}

/// 给每个 item 补上解析后的分钟数与设施类型，并按时序排列。
/// 时间不明的项排在最后（保持原有相对顺序）——它们仍要出现在时间轴上，只是没法定位。
pub fn norm_items(items: &[Value]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for it in items {
        let obj = match it.as_object() {
            Some(o) => o,
            None => continue,
        };
        let mins = parse_time(obj.get("time").unwrap_or(&Value::Null));
        let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let content = obj
            .get("content")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(&name)
            .to_string();
        let time = match obj.get("time").and_then(|v| v.as_str()) {
            Some(t) if !t.is_empty() => t.to_string(),
            _ => mins.map(fmt_min).unwrap_or_default(),
        };
        let kind = classify_activity(&content);
        out.push(json!({
            "name": name, "time": time, "content": content,
            "minutes": mins, "kind": kind, "kindZh": type_zh(kind),
        }));
    }
    // 稳定排序：只有时间已知的参与排序，未知的留在末尾
    let (mut known, unknown): (Vec<Value>, Vec<Value>) =
        out.into_iter().partition(|x| x.get("minutes").map(|m| !m.is_null()).unwrap_or(false));
    // 用 sort_by 而非 sort_unstable，保证同一分钟内的先后与输入一致
    known.sort_by(|a, b| {
        let ka = a.get("minutes").and_then(|v| v.as_i64()).unwrap_or(0);
        let kb = b.get("minutes").and_then(|v| v.as_i64()).unwrap_or(0);
        ka.cmp(&kb)
    });
    known.extend(unknown);
    known
}

/// 读 LingChat 日程；没有文件时返回默认日程。
///
/// 返回 `{source, path, roles:[{name,group,title,description,items}], todos, importantDays, raw}`
pub fn load() -> Value {
    let p = schedules_path();
    let mut src = "default";
    let mut raw: Option<Value> = None;
    if let Some(path) = &p {
        if path.exists() {
            if let Ok(txt) = std::fs::read_to_string(path) {
                if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                    raw = Some(v);
                    src = "lingchat";
                }
            }
        }
    }
    let chars: Vec<String> = list_characters()
        .iter()
        .filter_map(|c| c.get("name").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();

    // 用 Vec 保持插入顺序（Python 侧是 dict 的插入顺序，前端依赖它做稳定展示）
    let mut roles: Vec<Value> = Vec::new();
    let mut role_names: Vec<String> = Vec::new();
    if let Some(r) = &raw {
        let groups = r
            .get("scheduleGroups")
            .or_else(|| r.get("schedule_groups"))
            .and_then(|g| g.as_object());
        for (gname, g) in groups.into_iter().flatten() {
            if !g.is_object() {
                continue;
            }
            let items = norm_items(g.get("items").and_then(|i| i.as_array()).map(|a| a.as_slice()).unwrap_or(&[]));
            // 组名通常是角色名；不是角色名时也照样收，用组名当角色
            let mut role = gname.clone();
            for c in &chars {
                if !c.is_empty() && (gname.contains(c.as_str()) || c.contains(gname.as_str())) {
                    role = c.clone();
                    break;
                }
            }
            if !role_names.contains(&role) {
                role_names.push(role.clone());
                roles.push(json!({
                    "name": role, "group": gname,
                    "title": g.get("title").and_then(|v| v.as_str()).unwrap_or(gname),
                    "description": g.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                    "items": items,
                }));
            }
        }
    }
    // 没数据的角色用默认日程补齐（一个角色都没有时留一个「我」，保证地图上有东西）
    let fallback: Vec<String> = if chars.is_empty() { vec!["我".to_string()] } else { chars.clone() };
    for c in fallback {
        if !role_names.contains(&c) {
            role_names.push(c.clone());
            roles.push(json!({
                "name": c, "group": format!("{c}的一天"),
                "title": format!("{c}的一天"),
                "description": "（默认日程，LingChat 尚未设置）",
                "items": norm_items(&default_items()),
            }));
        }
    }

    let mut todos: Vec<Value> = Vec::new();
    let mut important = Vec::new();
    if let Some(r) = &raw {
        let tg = r
            .get("todoGroups")
            .or_else(|| r.get("todo_groups"))
            .and_then(|g| g.as_object());
        for (gname, g) in tg.into_iter().flatten() {
            for t in g.get("todos").and_then(|t| t.as_array()).into_iter().flatten() {
                if let Some(o) = t.as_object() {
                    let mut o = o.clone();
                    o.insert("group".into(), json!(gname));
                    todos.push(Value::Object(o));
                }
            }
        }
        important = r
            .get("importantDays")
            .or_else(|| r.get("important_days"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
    }

    json!({
        "source": src,
        "path": p.map(|x| x.to_string_lossy().to_string()),
        "roles": roles,
        "todos": todos,
        "importantDays": important,
        "raw": raw,
    })
}

/// 返回 `(当前活动, 下一个活动, 进度 0-1)`。`items` 需已过 [`norm_items`] 排序。
pub fn activity_at(items: &[Value], now_min: i64) -> (Option<Value>, Option<Value>, f64) {
    let timed: Vec<&Value> = items
        .iter()
        .filter(|x| x.get("minutes").map(|m| !m.is_null()).unwrap_or(false))
        .collect();
    if timed.is_empty() {
        return (None, None, 0.0);
    }
    let now = now_min.rem_euclid(1440);
    let mins = |v: &Value| v.get("minutes").and_then(|m| m.as_i64()).unwrap_or(0);
    let len = timed.len();
    for i in 0..len {
        let it = timed[i];
        let start = mins(it);
        let nxt = if i + 1 < len { timed[i + 1] } else { timed[0] };
        let nstart = mins(nxt);
        let end = if nstart > start { nstart } else { nstart + 1440 };
        if start <= now && now < end {
            let span = (end - start).max(1);
            return (Some(it.clone()), Some(nxt.clone()), round3((now - start) as f64 / span as f64));
        }
        // 跨零点：今天末尾到明天第一条
        if i == len - 1 {
            let end2 = mins(timed[0]) + 1440;
            if now >= start || now < mins(timed[0]) {
                let span = (end2 - start).max(1);
                let prog = ((now + 1440 - start).rem_euclid(1440)) as f64 / span as f64;
                return (Some(it.clone()), Some(timed[0].clone()), round3(prog));
            }
        }
    }
    let second = if len > 1 { Some(timed[1].clone()) } else { None };
    (Some(timed[0].clone()), second, 0.0)
}

/// 把设施类型落到**具体设施点**上（同一角色稳定命中同一处）。
///
/// `facilities` 传 `facilities::generate_all()` 的 facilities 数组；没有就只给类型，
/// 前端拿类型也能显示「在商业区」这种粗粒度标签。
pub fn role_place(kind: &str, facilities: Option<&[Value]>, seed: u64) -> Value {
    let mut out = json!({"kind": kind, "kindZh": type_zh(kind)});
    if kind == "transit" {
        out["label"] = json!("在路上（通勤中）");
        return out;
    }
    let facs = match facilities {
        Some(f) if !f.is_empty() => f,
        _ => return out,
    };
    let pool: Vec<&Value> = facs
        .iter()
        .filter(|f| {
            f.get("type").or_else(|| f.get("kind")).and_then(|v| v.as_str()) == Some(kind)
        })
        .collect();
    if pool.is_empty() {
        return out;
    }
    let f = pool[(seed % pool.len() as u64) as usize];
    out["facilityId"] = f.get("id").cloned().unwrap_or(Value::Null);
    out["name"] = f.get("name").cloned().unwrap_or(Value::Null);
    out["grid"] = f.get("grid").or_else(|| f.get("pos")).cloned().unwrap_or(Value::Null);
    out["label"] = f
        .get("name")
        .cloned()
        .unwrap_or_else(|| json!(type_zh(kind)));
    out
}

/// 角色名 → 稳定种子（同一角色每次落在同一处设施）
pub fn seed_of_name(name: &str) -> u64 {
    let d = md5::compute(name.as_bytes());
    let hex: String = d.0.iter().take(3).map(|b| format!("{b:02x}")).collect();
    u64::from_str_radix(&hex, 16).unwrap_or(0)
}

/// 设备本地「现在几点几分」（Python 侧是 `time.localtime()`）
///
/// 为什么用 chrono 而不是 `libc::localtime_r`：后者是 **Unix 专属**，
/// Windows 上连编译都过不去，而 LingChat 要同时支持 Windows/macOS/Linux/Android
/// —— 跨平台编译验证会直接把它拦下来。chrono 本来就在依赖里，顺带省掉一个 libc。
pub fn now_minutes() -> i64 {
    use chrono::Timelike;
    let now = chrono::Local::now();
    now.hour() as i64 * 60 + now.minute() as i64
}

/// 给前端/地图的完整载荷：每个角色此刻在做什么、该出现在哪。
///
/// `facilities` 由调用方传入（避免本模块反向依赖 facilities 模块）；
/// 传了就给出具体设施点，没传就退化为「只有类型」。
pub fn payload(now_min: Option<i64>, facilities: Option<&[Value]>, area: Option<&str>) -> Value {
    let now = now_min.unwrap_or_else(now_minutes);
    let d = load();
    let facs = facilities.filter(|f| !f.is_empty());
    let mut roles = Vec::new();
    for r in d["roles"].as_array().cloned().unwrap_or_default() {
        let name = r.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let items = r.get("items").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let (cur, nxt, prog) = activity_at(&items, now);
        let place = cur
            .as_ref()
            .map(|c| role_place(c.get("kind").and_then(|v| v.as_str()).unwrap_or("leisure"), facs, seed_of_name(&name)));
        let brief = |x: &Value| -> Value {
            json!({
                "name": x.get("name").cloned().unwrap_or(Value::Null),
                "time": x.get("time").cloned().unwrap_or(Value::Null),
                "content": x.get("content").cloned().unwrap_or(Value::Null),
                "kind": x.get("kind").cloned().unwrap_or(Value::Null),
                "kindZh": x.get("kindZh").cloned().unwrap_or(Value::Null),
            })
        };
        let mut now_obj = cur.as_ref().map(|c| {
            let mut o = brief(c);
            o["place"] = place.clone().unwrap_or(Value::Null);
            o
        });
        if let Some(o) = now_obj.as_mut() {
            if o.is_null() {
                now_obj = None;
            }
        }
        roles.push(json!({
            "name": name,
            "group": r.get("group").cloned().unwrap_or(Value::Null),
            "title": r.get("title").cloned().unwrap_or(Value::Null),
            "now": now_obj,
            "next": nxt.as_ref().map(brief),
            "progress": prog,
            "timeline": items.iter().map(brief).collect::<Vec<_>>(),
        }));
    }
    json!({
        "ok": true,
        "source": d["source"],
        "path": d["path"],
        "now": fmt_min(now),
        "nowMinutes": now.rem_euclid(1440),
        "area": area,
        "roles": roles,
        "characters": list_characters(),
        "todos": d["todos"],
        "importantDays": d["importantDays"],
        "placeSource": if facs.is_some() { "facilities" } else { "kind-only" },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 改环境变量的测试必须串行（cargo 默认多线程跑测试）
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn parse_time_accepts_all_python_forms() {
        assert_eq!(parse_time(&json!("08:30")), Some(510));
        assert_eq!(parse_time(&json!("8点30")), Some(510));
        assert_eq!(parse_time(&json!("9点")), Some(540));
        assert_eq!(parse_time(&json!("0830")), Some(510));
        assert_eq!(parse_time(&json!(8)), Some(480), "纯数字 8 当作 8 点");
        assert_eq!(parse_time(&json!(510)), Some(510), ">23 当作分钟数");
        assert_eq!(parse_time(&json!("23:00")), Some(1380));
        assert_eq!(parse_time(&json!("8：00")), Some(480), "全角冒号");
        assert_eq!(parse_time(&json!("07:30")), Some(450));
        assert_eq!(parse_time(&json!("")), None);
        assert_eq!(parse_time(&Value::Null), None);
        assert_eq!(parse_time(&json!("上班")), None);
        assert_eq!(parse_time(&json!(99)), Some(99), ">23 的纯数字当作分钟数（99 = 01:39）");
        assert_eq!(parse_time(&json!(1440)), None, "超出一天的分钟数无效");
    }

    #[test]
    fn parse_time_search_semantics_match_regex() {
        // 正则在整串里搜索第一个匹配，不要求从头开始
        assert_eq!(parse_time(&json!("上午8:30 出发")), Some(510));
        // 小时贪婪取 2 位，取不到才回退 1 位："123:45" 应从第 2 个字符起匹配 23:45
        assert_eq!(parse_time(&json!("123:45")), Some(1425));
        // 分钟是可选组，最多两位
        assert_eq!(parse_time(&json!("8:300")), Some(510));
        // 越界的先匹配不上，再退回 3~4 位纯数字分支
        assert_eq!(parse_time(&json!("2560")), None, "25 点无效，且不是合法 3-4 位时间");
    }

    #[test]
    fn classify_follows_priority_order() {
        // 「通勤去公司」必须判成在路上，而不是到了公司
        assert_eq!(classify_activity("通勤去公司"), "transit");
        assert_eq!(classify_activity("在公司上班"), "commercial");
        assert_eq!(classify_activity("在家睡觉"), "residential");
        assert_eq!(classify_activity("去图书馆自习"), "education");
        assert_eq!(classify_activity("去医院看病"), "medical");
        assert_eq!(classify_activity("出门散步"), "leisure");
        assert_eq!(classify_activity("去银行办事"), "civic");
        assert_eq!(classify_activity("出差住酒店"), "lodging");
        assert_eq!(classify_activity(""), "leisure", "认不出就当在外面");
        assert_eq!(classify_activity("随便写点什么"), "leisure");
    }

    #[test]
    fn norm_items_sorts_and_enriches() {
        let items = vec![
            json!({"name": "午餐", "time": "12:00", "content": "出去吃午饭"}),
            json!({"name": "起床", "time": "07:30", "content": "起床洗漱"}),
            json!({"name": "神秘", "time": "??", "content": "时间不明"}),
        ];
        let out = norm_items(&items);
        assert_eq!(out[0]["name"], json!("起床"));
        assert_eq!(out[1]["name"], json!("午餐"));
        assert_eq!(out[2]["name"], json!("神秘"), "时间不明的排在最后");
        assert_eq!(out[0]["kind"], json!("residential"));
        assert_eq!(out[1]["kind"], json!("commercial"));
        assert_eq!(out[2]["minutes"], Value::Null);
        assert_eq!(out[0]["kindZh"], json!("住宅"));
    }

    #[test]
    fn norm_items_tolerates_junk() {
        let out = norm_items(&[json!("不是对象"), json!(123)]);
        assert!(out.is_empty(), "非对象项直接丢弃，不 panic");
        let out2 = norm_items(&[json!({"name": "只有名字"})]);
        assert_eq!(out2.len(), 1);
        assert_eq!(out2[0]["minutes"], Value::Null);
        assert_eq!(out2[0]["content"], json!("只有名字"), "content 缺失时退回 name");
    }

    fn tl() -> Vec<Value> {
        norm_items(&default_items())
    }

    #[test]
    fn activity_at_picks_current_slot() {
        let items = tl();
        let (cur, nxt, prog) = activity_at(&items, 9 * 60 + 30);
        assert_eq!(cur.as_ref().unwrap()["name"], json!("工作"));
        assert_eq!(nxt.as_ref().unwrap()["name"], json!("午餐"));
        assert!(prog > 0.0 && prog < 1.0, "9:30 在工作时段中间，进度应在 0~1，实际 {prog}");
    }

    #[test]
    fn activity_at_handles_midnight_wrap() {
        let items = tl();
        // 23:00 睡觉 → 次日 07:30 起床，02:00 应该还在「睡觉」
        let (cur, nxt, _) = activity_at(&items, 2 * 60);
        assert_eq!(cur.as_ref().unwrap()["name"], json!("睡觉"));
        assert_eq!(nxt.as_ref().unwrap()["name"], json!("起床"));
        // 刚过 23:00 也要落进睡觉
        let (cur2, _, _) = activity_at(&items, 23 * 60 + 30);
        assert_eq!(cur2.as_ref().unwrap()["name"], json!("睡觉"));
    }

    #[test]
    fn activity_at_empty_and_out_of_range() {
        assert!(activity_at(&[], 600).0.is_none());
        let items = tl();
        // now_min 会自动取模，负数也不能 panic
        let (cur, _, _) = activity_at(&items, -60);
        assert!(cur.is_some());
        let (cur2, _, _) = activity_at(&items, 1440 * 3 + 600);
        assert!(cur2.is_some());
    }

    #[test]
    fn fmt_min_pads() {
        assert_eq!(fmt_min(0), "00:00");
        assert_eq!(fmt_min(510), "08:30");
        assert_eq!(fmt_min(1380), "23:00");
        assert_eq!(fmt_min(1439), "23:59");
    }

    #[test]
    fn seed_of_name_is_stable_and_differs() {
        assert_eq!(seed_of_name("小美"), seed_of_name("小美"));
        assert_ne!(seed_of_name("小美"), seed_of_name("阿强"));
        assert!(seed_of_name("小美") < (1 << 24), "只取 6 位十六进制");
    }

    #[test]
    fn role_place_is_stable_per_seed() {
        let facs = vec![
            json!({"id": "f1", "type": "commercial", "name": "便利店", "grid": [3, 4]}),
            json!({"id": "f2", "type": "commercial", "name": "咖啡馆", "grid": [7, 8]}),
            json!({"id": "f3", "type": "residential", "name": "住宅楼", "grid": [1, 1]}),
        ];
        let a = role_place("commercial", Some(&facs), 0);
        let b = role_place("commercial", Some(&facs), 0);
        assert_eq!(a, b, "同一 seed 必须稳定命中同一处");
        assert_eq!(a["facilityId"], json!("f1"));
        let c = role_place("commercial", Some(&facs), 1);
        assert_eq!(c["facilityId"], json!("f2"));
        // 取模回绕
        let d = role_place("commercial", Some(&facs), 4);
        assert_eq!(d["facilityId"], json!("f1"));
    }

    #[test]
    fn role_place_degrades_gracefully() {
        let no_fac = role_place("medical", None, 3);
        assert_eq!(no_fac["kind"], json!("medical"));
        assert_eq!(no_fac["kindZh"], json!("医疗"));
        assert!(no_fac.get("facilityId").is_none() || no_fac["facilityId"].is_null());

        let empty: Vec<Value> = vec![];
        assert!(role_place("medical", Some(&empty), 0)["facilityId"].is_null());

        // 池子里没有这一类型 → 只给类型，不 panic
        let facs = vec![json!({"id": "f1", "type": "commercial"})];
        assert!(role_place("school", Some(&facs), 0)["facilityId"].is_null());

        // 通勤是「在路上」，永远没有具体设施点
        let t = role_place("transit", Some(&facs), 0);
        assert_eq!(t["label"], json!("在路上（通勤中）"));
    }

    #[test]
    fn role_place_accepts_kind_field_too() {
        // facilities 模块早期用 kind 字段，兼容两种写法
        let facs = vec![json!({"id": "k1", "kind": "education", "name": "小学"})];
        let p = role_place("education", Some(&facs), 0);
        assert_eq!(p["facilityId"], json!("k1"));
    }

    #[test]
    fn payload_works_without_lingchat_data() {
        // 本机没有 LingChat 数据目录时也必须给出一份可用载荷（默认日程兜底）
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("WM_LINGCHAT_DATA", "/nonexistent-path-for-test");
        let p = payload(Some(530), None, Some("测试小区"));
        assert_eq!(p["ok"], json!(true));
        assert_eq!(p["now"], json!("08:50"));
        assert_eq!(p["nowMinutes"], json!(530));
        assert_eq!(p["placeSource"], json!("kind-only"));
        let roles = p["roles"].as_array().unwrap();
        assert!(!roles.is_empty(), "没有角色时也要有「我」的默认日程");
        assert_eq!(roles[0]["name"], json!("我"));
        let now = &roles[0]["now"];
        assert_eq!(now["name"], json!("通勤"), "08:50 落在通勤时段（08:40–09:00）");
        assert_eq!(now["kind"], json!("transit"));
        assert_eq!(now["place"]["label"], json!("在路上（通勤中）"));
        assert!(p["roles"][0]["timeline"].as_array().unwrap().len() >= 10);
        std::env::remove_var("WM_LINGCHAT_DATA");
    }

    #[test]
    fn payload_uses_facilities_when_given() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("WM_LINGCHAT_DATA", "/nonexistent-path-for-test");
        let facs = vec![json!({"id": "c1", "type": "commercial", "name": "写字楼", "grid": [5, 5]})];
        let p = payload(Some(9 * 60 + 30), Some(&facs), None);
        assert_eq!(p["placeSource"], json!("facilities"));
        let now = &p["roles"][0]["now"];
        assert_eq!(now["kind"], json!("commercial"));
        assert_eq!(now["place"]["name"], json!("写字楼"));
        assert_eq!(now["place"]["grid"], json!([5, 5]));
        std::env::remove_var("WM_LINGCHAT_DATA");
    }

    #[test]
    fn yml_scalar_and_block_parsing() {
        let yml = "ai_name: \"小美\"\nai_subtitle: 大学生\ninfo: |\n  喜欢看书\n  也喜欢散步\nother: x\n";
        assert_eq!(yml_scalar(yml, "ai_name").unwrap(), "小美");
        assert_eq!(yml_scalar(yml, "ai_subtitle").unwrap(), "大学生");
        assert_eq!(yml_block(yml).unwrap(), "喜欢看书 也喜欢散步");
        assert!(yml_scalar("没有任何键\n", "ai_name").is_none());
        assert!(yml_block("info:\nno indent\n").is_none(), "缩进不足不算块");
    }

    #[test]
    fn now_minutes_is_in_a_day() {
        let m = now_minutes();
        assert!((0..1440).contains(&m), "本地时间必须落在一天之内，实际 {m}");
    }
}
