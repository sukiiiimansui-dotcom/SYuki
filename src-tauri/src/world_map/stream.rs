//! 流式小区生成（移植自 Python `stream_gen.py`）
//!
//! 目标：**让 AI 画图的过程可见** —— 不是等两分钟蹦出一张图，
//! 而是建筑一栋栋冒出来（Python 侧实测：总耗时 129s → 18.6s，快 7 倍）。
//!
//! 关键点：
//!   1. `stream: true` 调 LLM，token 一到就处理
//!   2. **增量 JSON 解析**：从半截 JSON 里抠出已经闭合的 `{...}` 对象
//!      （不能等整个文档解析成功 —— 那时图早画完了）
//!   3. 按元素逐条推送，前端收到一条画一栋
//!
//! 与 Python 侧的差别：Python 因 Termux 的 SSL 长连接 bug 用了 curl 子进程，
//! Rust 直接用 reqwest 的 `bytes_stream()`，干净得多。
use serde_json::{json, Value};
use tokio::sync::mpsc;

/// 流式事件
#[derive(Debug, Clone)]
pub enum Event {
    Start { area: String, size: i32, model: String },
    Meta { name: String },
    Size { size: i32 },
    Item { kind: String, item: Value, index: usize, elapsed: f64 },
    Warn { message: String },
    Debug { chunks: usize, chars: usize, lines: usize },
    Done { layout: Value, elapsed: f64 },
    Error { message: String },
}

impl Event {
    pub fn to_json(&self) -> Value {
        match self {
            Event::Start { area, size, model } => json!({"type":"start","area":area,"size":size,"model":model}),
            Event::Meta { name } => json!({"type":"meta","name":name}),
            Event::Size { size } => json!({"type":"size","size":size}),
            Event::Item { kind, item, index, elapsed } => json!({
                "type": kind, "item": item, "index": index, "elapsed": elapsed
            }),
            Event::Warn { message } => json!({"type":"warn","message":message}),
            Event::Debug { chunks, chars, lines } => json!({
                "type":"debug","stats":{"chunks":chunks,"chars":chars,"lines":lines}
            }),
            Event::Done { layout, elapsed } => json!({
                "type":"done","layout":layout,"elapsed":elapsed,
                "counts": {
                    "buildings": layout.get("buildings").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
                    "roads": layout.get("roads").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
                    "parks": layout.get("parks").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
                    "water": layout.get("water").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
                }
            }),
            Event::Error { message } => json!({"type":"error","message":message}),
        }
    }
}

// ───────────────────────── 增量 JSON 解析（纯函数，重点单测）─────────────────────────

/// 从缓冲区里抠出 `"key": [ ... ]` 中**已经闭合**的对象。
///
/// 返回 (新对象列表, 新的扫描起点)。`from` 之后的 `{` 才开始扫描，
/// 这样已处理过的不会重复产出。
pub fn extract_new_objects(buf: &str, key: &str, from: usize) -> (Vec<Value>, usize) {
    let kpos = match buf.find(&format!("\"{key}\"")) {
        Some(i) => i,
        None => return (Vec::new(), from),
    };
    let lb = match buf[kpos..].find('[') {
        Some(i) => kpos + i,
        None => return (Vec::new(), from),
    };
    let bytes = buf.as_bytes();
    let start_scan = (lb + 1).max(from);
    let mut objs = Vec::new();
    let (mut depth, mut start, mut k) = (0i32, -1i32, start_scan);
    let mut consumed = start_scan;
    while k < bytes.len() {
        match bytes[k] {
            b'{' => {
                if depth == 0 {
                    start = k as i32;
                }
                depth += 1;
            }
            b'}' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 && start >= 0 {
                        let frag = &buf[start as usize..k + 1];
                        if let Ok(v) = serde_json::from_str::<Value>(frag) {
                            objs.push(v);
                            consumed = k + 1;
                        }
                        start = -1;
                    }
                }
            }
            b']' if depth == 0 => {
                consumed = k + 1;
                break;
            }
            _ => {}
        }
        k += 1;
    }
    (objs, consumed)
}

/// 从半截 JSON 里取一个字符串标量（如 `"name":"xxx"`）
pub fn extract_scalar(buf: &str, key: &str) -> Option<Value> {
    let pat = format!("\"{key}\"");
    let mut search = 0usize;
    while let Some(rel) = buf[search..].find(&pat) {
        let i = search + rel + pat.len();
        let rest = buf[i..].trim_start();
        let rest = rest.strip_prefix(':').map(|s| s.trim_start())?;
        if let Some(stripped) = rest.strip_prefix('"') {
            // 字符串：找到未转义的收尾引号
            let mut out = String::new();
            let mut chars = stripped.chars();
            let mut escaped = false;
            while let Some(c) = chars.next() {
                if escaped {
                    out.push(c);
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    return Some(Value::String(out));
                } else {
                    out.push(c);
                }
            }
            return None; // 还没读完
        }
        // 数字
        let numstr: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '.').collect();
        if !numstr.is_empty() {
            if let Ok(n) = numstr.parse::<i64>() {
                return Some(json!(n));
            }
            if let Ok(f) = numstr.parse::<f64>() {
                return Some(json!(f));
            }
        }
        search = i;
    }
    None
}

/// 把流式收到的碎片组装成最终布局（保证与前端逐条看到的一致）
pub fn assemble_layout(buf: &str, area: &str, fallback_size: i32) -> Value {
    let mut layout = json!({
        "name": extract_scalar(buf, "name").and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_else(|| area.to_string()),
        "size": extract_scalar(buf, "size").and_then(|v| v.as_i64()).unwrap_or(fallback_size as i64),
        "buildings": [], "roads": [], "parks": [], "water": [],
        "_streamed": true,
    });
    for key in ["buildings", "roads", "parks", "water"] {
        let (objs, _) = extract_new_objects(&format!("{buf}]"), key, 0);
        if let Some(o) = layout.as_object_mut() {
            o.insert(key.to_string(), json!(objs));
        }
    }
    layout
}

// ───────────────────────── LLM 配置与请求 ─────────────────────────

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub url: String,
    pub model: String,
    pub api_key: String,
}

impl LlmConfig {
    /// 从环境变量读（独立验证项目用）；
    /// 集成到 LingChat 后这里改成复用 `ai_service::llm::slot_snapshot`，其余逻辑不变。
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("WM_LLM_KEY").ok()?;
        Some(Self {
            url: std::env::var("WM_LLM_URL")
                .unwrap_or_else(|_| "https://api.deepseek.com/v1/chat/completions".into()),
            model: std::env::var("WM_LLM_MODEL").unwrap_or_else(|_| "deepseek-flash".into()),
            api_key,
        })
    }

    /// 从 Python 项目的 config.local.js 里读（开发期省事）
    pub fn from_legacy_config(path: &str) -> Option<Self> {
        let txt = std::fs::read_to_string(path).ok()?;
        let grab = |key: &str| -> Option<String> {
            let pat = format!("{key}:");
            let i = txt.find(&pat)?;
            let rest = &txt[i + pat.len()..];
            let q1 = rest.find('"')?;
            let rest2 = &rest[q1 + 1..];
            let q2 = rest2.find('"')?;
            Some(rest2[..q2].to_string())
        };
        Some(Self {
            url: grab("base_url")?,
            model: grab("model").unwrap_or_else(|| "deepseek-flash".into()),
            api_key: grab("api_key")?,
        })
    }
}

/// 构造生成小区的 prompt（与 Python 侧保持同一套要求）
pub fn build_prompt(area: &str, context: &str, base: i32, osm_hint: Option<&str>) -> (String, String) {
    let sys_p = format!(
        "你是城市规划师，为一个二维世界地图生成小区(街区)布局JSON。只返回JSON，格式:\n\
{{\"name\":\"小区名\",\"size\":{base},\
\"buildings\":[{{\"x\":0,\"y\":0,\"w\":3,\"h\":2,\"type\":\"residential\",\"name\":\"3号楼\",\"floors\":6}}],\
\"roads\":[{{\"x1\":0,\"y1\":0,\"x2\":{base},\"y2\":0,\"type\":\"main\",\"name\":\"中山路\"}}],\
\"parks\":[{{\"x\":5,\"y\":5,\"w\":4,\"h\":3,\"name\":\"中心公园\"}}],\
\"water\":[{{\"x\":10,\"y\":10,\"w\":3,\"h\":2,\"name\":\"人工湖\"}}]}}\n\
规则:\n\
1) size 是网格边长(用 {base}), 所有坐标 0..size; 建筑不要重叠。\n\
2) building.type ∈ residential/office/commercial/shop/restaurant/cafe/school/hospital/civic/leisure。\n\
3) **每栋建筑都必须有 name**，中文，具体有辨识度、贴合该区域与剧情设定。\n\
4) residential 用栋号(如「3号楼」); 其余用店名/楼名。每栋都要有 floors(楼层数)。\n\
5) 每条主干道/次干道都要有 name(路名)。\n\
6) parks/water 也要有 name。\n\
7) 建筑数量约 size*0.7 栋，沿道路两侧排布。\n\
先输出 name 和 size，再依次输出 buildings（一栋接一栋）、roads、parks、water。只返回JSON，不要多余文字。"
    );
    let mut usr = format!("区域: {area}。");
    if !context.is_empty() {
        usr.push_str(&format!("剧情: {context}。"));
    }
    if let Some(h) = osm_hint {
        if !h.is_empty() {
            usr.push_str(h);
            usr.push_str("请参考上述真实地物类型与数量分布来设计小区，使其贴近真实。");
        }
    }
    usr.push_str(&format!("请生成这个小区的地图布局（网格 {base}x{base}）。"));
    (sys_p, usr)
}

/// 流式生成：把事件推给 channel，调用方边收边转发（SSE / Tauri Channel）
///
/// 返回接收端；后台任务会在流结束后关闭它。
pub fn spawn_stream(
    cfg: LlmConfig,
    area: String,
    context: String,
    expand: i32,
    osm_hint: Option<String>,
    timeout_secs: u64,
) -> mpsc::Receiver<Event> {
    let (tx, rx) = mpsc::channel::<Event>(256);
    tokio::spawn(async move {
        let t0 = std::time::Instant::now();
        let base = 20 + expand.max(0) * 8;
        let (sys_p, usr) = build_prompt(&area, &context, base, osm_hint.as_deref());
        let _ = tx.send(Event::Start { area: area.clone(), size: base, model: cfg.model.clone() }).await;

        let payload = json!({
            "model": cfg.model,
            "messages": [
                {"role":"system","content": sys_p},
                {"role":"user","content": usr}
            ],
            "temperature": 0.8,
            "max_tokens": 6000,
            "enable_thinking": false,
            "response_format": {"type": "json_object"},
            "stream": true
        });

        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(Event::Error { message: format!("HTTP 客户端创建失败: {e}") }).await;
                return;
            }
        };
        let resp = client
            .post(&cfg.url)
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .bearer_auth(&cfg.api_key)
            .json(&payload)
            .send()
            .await;
        let mut resp = match resp {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                let _ = tx.send(Event::Error { message: format!("LLM 返回 HTTP {}", r.status()) }).await;
                return;
            }
            Err(e) => {
                let _ = tx.send(Event::Error { message: format!("请求失败: {e}") }).await;
                return;
            }
        };

        let mut buf = String::new();
        let mut pending = String::new(); // 未凑成完整行的尾巴
        let mut consumed: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for k in ["buildings", "roads", "parks", "water"] {
            consumed.insert(k.into(), 0);
            counts.insert(k.into(), 0);
        }
        let mut name_sent = false;
        let mut dbg = (0usize, 0usize, 0usize); // chunks, chars, lines

        loop {
            let bytes = match resp.chunk().await {
                Ok(Some(b)) => b,
                Ok(None) => break,
                Err(e) => {
                    let _ = tx.send(Event::Warn { message: format!("读流异常: {e}") }).await;
                    break;
                }
            };
            dbg.0 += 1;
            let text = String::from_utf8_lossy(&bytes);
            pending.push_str(&text);

            // 按行切（最后一段可能不完整，留到下一轮）
            while let Some(nl) = pending.find('\n') {
                let line: String = pending.drain(..=nl).collect();
                let line = line.trim();
                if !line.starts_with("data:") {
                    continue;
                }
                dbg.2 += 1;
                let data = line[5..].trim();
                if data == "[DONE]" {
                    break;
                }
                // 【临时诊断】把前 3 个 data 行原样报给前端，定位「chars=0」的成因
                if dbg.2 <= 3 {
                    let _ = tx.send(Event::Warn {
                        message: format!("RAW#{} {}", dbg.2, &data.chars().take(220).collect::<String>()),
                    }).await;
                }
                let obj: Value = match serde_json::from_str(data) {
                    Ok(v) => v,
                    Err(e) => {
                        if dbg.2 <= 3 {
                            let _ = tx.send(Event::Warn { message: format!("PARSE-FAIL {e}") }).await;
                        }
                        continue;
                    }
                };
                let delta = obj
                    .get("choices")
                    .and_then(|c| c.as_array())
                    .and_then(|a| a.first())
                    .and_then(|c| c.get("delta"));
                let piece = delta
                    .and_then(|d| d.get("content").or_else(|| d.get("reasoning_content")))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if piece.is_empty() {
                    continue;
                }
                dbg.1 += piece.len();
                buf.push_str(piece);

                // 小区名 / 规模（最先出现）
                if !name_sent {
                    if let Some(Value::String(n)) = extract_scalar(&buf, "name") {
                        name_sent = true;
                        let _ = tx.send(Event::Meta { name: n }).await;
                    }
                    if let Some(sz) = extract_scalar(&buf, "size").and_then(|v| v.as_i64()) {
                        let _ = tx.send(Event::Size { size: sz as i32 }).await;
                    }
                }

                // 逐类增量取元素
                for (key, ev) in [
                    ("buildings", "building"),
                    ("roads", "road"),
                    ("parks", "park"),
                    ("water", "water"),
                ] {
                    let from = *consumed.get(key).unwrap_or(&0);
                    let (objs, next) = extract_new_objects(&buf, key, from);
                    consumed.insert(key.into(), next);
                    for o in objs {
                        let c = counts.entry(key.into()).or_insert(0);
                        *c += 1;
                        if tx
                            .send(Event::Item {
                                kind: ev.into(),
                                item: o,
                                index: *c,
                                elapsed: (t0.elapsed().as_millis() as f64) / 1000.0,
                            })
                            .await
                            .is_err()
                        {
                            return; // 接收端已关闭（前端断开）
                        }
                    }
                }
            }
        }

        let _ = tx.send(Event::Debug { chunks: dbg.0, chars: dbg.1, lines: dbg.2 }).await;
        let layout = assemble_layout(&buf, &area, base);
        let _ = tx
            .send(Event::Done { layout, elapsed: (t0.elapsed().as_millis() as f64) / 1000.0 })
            .await;
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_objects_only_complete_ones() {
        let buf = r#"{"name":"测试","buildings":[{"x":1,"y":2,"w":3,"h":2,"name":"1号楼"},{"x":5,"#;
        let (objs, _) = extract_new_objects(buf, "buildings", 0);
        assert_eq!(objs.len(), 1, "只有第一个对象闭合了，应只抠出它");
        assert_eq!(objs[0]["name"], "1号楼");
    }

    #[test]
    fn extract_objects_incremental_no_duplicates() {
        let part1 = r#"{"buildings":[{"name":"A"},{"name":"B"}]"#;
        let (a, pos) = extract_new_objects(part1, "buildings", 0);
        assert_eq!(a.len(), 2);
        // 再喂同样的内容，从上次位置继续 → 不该重复
        let (b, _) = extract_new_objects(part1, "buildings", pos);
        assert!(b.is_empty(), "已处理过的不应重复产出");
        // 追加一个新对象
        let part2 = r#"{"buildings":[{"name":"A"},{"name":"B"},{"name":"C"}]"#;
        let (c, _) = extract_new_objects(part2, "buildings", pos);
        assert_eq!(c.len(), 1, "只应产出新增的那个");
        assert_eq!(c[0]["name"], "C");
    }

    #[test]
    fn extract_objects_handles_nested() {
        // 对象里带嵌套数组/对象，括号计数必须正确
        let buf = r#"{"buildings":[{"name":"A","tags":{"a":[1,2]},"pts":[{"x":1}]},{"na"#;
        let (objs, _) = extract_new_objects(buf, "buildings", 0);
        assert_eq!(objs.len(), 1, "嵌套结构不应打断计数");
        assert_eq!(objs[0]["tags"]["a"][1], 2);
    }

    #[test]
    fn extract_objects_missing_key_returns_empty() {
        let (o, p) = extract_new_objects(r#"{"name":"x"}"#, "buildings", 0);
        assert!(o.is_empty());
        assert_eq!(p, 0);
    }

    #[test]
    fn extract_scalar_string_and_number() {
        assert_eq!(extract_scalar(r#"{"name":"越秀小区","size":20}"#, "name").unwrap(), json!("越秀小区"));
        assert_eq!(extract_scalar(r#"{"name":"甲","size":20}"#, "size").unwrap(), json!(20));
        // 半截字符串 → 还没读完，返回 None
        assert!(extract_scalar(r#"{"name":"越秀小"#, "name").is_none());
        assert!(extract_scalar(r#"{"size":"#, "size").is_none());
    }

    #[test]
    fn extract_scalar_escaped_quote() {
        let v = extract_scalar(r#"{"name":"a\"b"}"#, "name").unwrap();
        assert_eq!(v, json!("a\"b"), "转义引号不应提前结束");
    }

    #[test]
    fn assemble_layout_from_partial_stream() {
        let buf = r#"{"name":"测试小区","size":18,"buildings":[{"x":1,"y":1,"w":2,"h":2,"type":"residential","name":"1号楼"},{"x":5,"y":5,"w":3,"h":2,"type":"office","name":"云顶大厦"}],"roads":[{"x1":0,"y1":0,"x2":18,"y2":0,"type":"main","name":"中山路"}],"parks":[{"x":8,"y":8,"w":3,"h":3,"name":"中心公园"}],"water":[]}"#;
        let lay = assemble_layout(buf, "越秀区", 20);
        assert_eq!(lay["name"], json!("测试小区"));
        assert_eq!(lay["size"], json!(18));
        assert_eq!(lay["buildings"].as_array().unwrap().len(), 2);
        assert_eq!(lay["roads"].as_array().unwrap().len(), 1);
        assert_eq!(lay["parks"].as_array().unwrap().len(), 1);
        assert_eq!(lay["_streamed"], json!(true));
    }

    #[test]
    fn assemble_layout_falls_back_to_area_name() {
        let lay = assemble_layout("{}", "广州市·越秀区", 20);
        assert_eq!(lay["name"], json!("广州市·越秀区"));
        assert_eq!(lay["size"], json!(20));
        assert!(lay["buildings"].as_array().unwrap().is_empty());
    }

    #[test]
    fn prompt_contains_requirements() {
        let (sys_p, usr) = build_prompt("广州市·越秀区", "雨天", 20, Some("该区域真实 OSM 数据：建筑 yes×145。"));
        for kw in ["每栋建筑都必须有 name", "floors", "路名", "size 是网格边长"] {
            assert!(sys_p.contains(kw), "prompt 应含要求: {kw}");
        }
        assert!(usr.contains("广州市·越秀区"));
        assert!(usr.contains("雨天"));
        assert!(usr.contains("真实 OSM 数据"), "OSM 参考应拼进 prompt");
    }

    #[test]
    fn event_serialization() {
        let e = Event::Item { kind: "building".into(), item: json!({"name":"1号楼"}), index: 3, elapsed: 1.5 };
        let j = e.to_json();
        assert_eq!(j["type"], json!("building"));
        assert_eq!(j["index"], json!(3));
        assert_eq!(j["item"]["name"], json!("1号楼"));
        // Done 事件要带 counts，前端据此显示统计
        let d = Event::Done { layout: json!({"buildings":[1,2],"roads":[1]}), elapsed: 9.9 };
        let dj = d.to_json();
        assert_eq!(dj["counts"]["buildings"], json!(2));
        assert_eq!(dj["counts"]["roads"], json!(1));
        assert_eq!(dj["counts"]["water"], json!(0));
    }

    #[test]
    fn legacy_config_parsing() {
        let dir = std::env::temp_dir().join(format!("wm_cfg_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let fp = dir.join("config.local.js");
        std::fs::write(
            &fp,
            "export default {\n  base_url: \"https://api.deepseek.com/v1/chat/completions\",\n  model: \"deepseek-flash\",\n  api_key: \"sk-test\"\n}\n",
        )
        .unwrap();
        let cfg = LlmConfig::from_legacy_config(fp.to_str().unwrap()).unwrap();
        assert_eq!(cfg.model, "deepseek-flash");
        assert_eq!(cfg.api_key, "sk-test");
        assert!(cfg.url.contains("deepseek.com"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
