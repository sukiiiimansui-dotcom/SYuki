//! B站学习模块（对标原 SYuki `bili_learn.py`）。
//!
//! 能力：B站热榜 / 视频搜索 / 视频信息 / 弹幕(统计重复) / 高赞评论
//!       → 学习库（`<data>/bili_knowledge.json`），供对话注入与前端展示。
//! 学习库用 JSON 文件存储（轻量，避免深度 sea_orm 迁移）；AI 提炼(LLM)为后续增强。
//!
//! 网络走 `reqwest`（统一 rustls + webpki-roots，绕开 Android TLS panic），
//! 参考 `tools/web_search.rs` 的客户端构建方式。

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// B站 API 请求的 UA（Android，需带 Referer）
const UA: &str = "Mozilla/5.0 (Linux; Android 13) AppleWebKit/537.36";
const API_BASE: &str = "https://api.bilibili.com";
/// 学习库文件名（位于 data_dir 下）
const DB_FILE: &str = "bili_knowledge.json";
/// 单次网络超时
const NET_TIMEOUT: Duration = Duration::from_secs(15);

/// 学习库中的一条视频知识
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiliVideo {
    pub bvid: String,
    pub title: String,
    pub up: String,
    pub tname: String,
    pub vdesc: String,
    pub repeat_danmaku: String,
    pub top_comments: String,
    pub culture: String,
    pub learned_at: String,
}

/// 搜索结果条目
#[derive(Debug, Clone, Serialize)]
pub struct BiliSearchItem {
    pub bvid: String,
    pub title: String,
    pub up: String,
    pub play: u64,
    pub like: u64,
    pub desc: String,
}

/// 学习结果（返回给调用方）
#[derive(Debug, Clone, Serialize)]
pub struct BiliLearnResult {
    pub ok: bool,
    pub bvid: String,
    pub title: String,
    pub up: String,
    pub danmaku: usize,
    pub repeat: usize,
    pub comments: usize,
    pub culture: String,
}

// ==================== HTTP 客户端 ====================

/// 构建带统一 TLS（webpki-roots）的 HTTP 客户端，与 `web_search.rs` 保持一致。
pub fn build_client() -> Result<Client, String> {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .roots
        .extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls_config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| format!("rustls 协议版本配置失败: {e}"))?
    .with_root_certificates(Arc::new(roots))
    .with_no_client_auth();

    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(NET_TIMEOUT)
        .tls_backend_preconfigured(tls_config)
        .user_agent(UA)
        .build()
        .map_err(|e| format!("创建 B站 HTTP 客户端失败: {e}"))
}

/// B站 API GET 请求（带 Referer），返回 JSON；失败返回空 Value。
async fn get_json(client: &Client, url: &str, referer: &str) -> Value {
    let response = match client
        .get(url)
        .header(reqwest::header::REFERER, referer)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[bilibili] GET {url} 失败: {e}");
            return Value::Null;
        }
    };
    match response.json::<Value>().await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[bilibili] 解析 {url} JSON 失败: {e}");
            Value::Null
        }
    }
}

// ==================== 学习库存取（JSON） ====================

fn db_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join(DB_FILE)
}

/// 读取学习库（文件不存在则返回空）。
pub fn load_knowledge(data_dir: &Path) -> Vec<BiliVideo> {
    let path = db_path(data_dir);
    if !path.exists() {
        return Vec::new();
    }
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// 原子写回学习库。
fn save_knowledge(data_dir: &Path, videos: &[BiliVideo]) -> Result<(), String> {
    let path = db_path(data_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let json = serde_json::to_string_pretty(videos).map_err(|e| format!("序列化失败: {e}"))?;
    // 写临时文件后原子替换，避免中途崩溃留半截
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json.as_bytes()).map_err(|e| format!("写入失败: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换失败: {e}"))
}

// ==================== B站 API ====================

/// 视频信息（bvid → aid/title/up/tname/desc/cid）
async fn get_video_info(client: &Client, bvid: &str) -> Value {
    let url = format!("{API_BASE}/x/web-interface/view?bvid={bvid}");
    get_json(client, &url, "https://www.bilibili.com/").await
        .get("data")
        .cloned()
        .unwrap_or(Value::Null)
}

/// 弹幕 XML 解析：提取 `<d p="...">正文</d>` 的正文（不引入 XML 依赖）。
pub fn parse_danmaku(xml: &str, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while out.len() < limit {
        let Some(start) = rest.find("<d p=") else { break };
        let Some(tag_end) = rest[start..].find('>') else { break };
        let content_start = start + tag_end + 1;
        if let Some(end) = rest[content_start..].find("</d>") {
            out.push(rest[content_start..content_start + end].to_string());
            rest = &rest[content_start + end + 4..];
        } else {
            break;
        }
    }
    out
}

/// 热榜（rid=0 全站），返回 bvid 列表。
pub async fn get_hot_bvids(client: &Client, limit: usize) -> Vec<String> {
    let url = format!("{API_BASE}/x/web-interface/ranking?rid=0&type=all");
    let data = get_json(client, &url, "https://www.bilibili.com/").await;
    let list = data.get("data").and_then(|d| d.get("list")).and_then(Value::as_array);
    if let Some(list) = list {
        return list
            .iter()
            .filter_map(|v| v.get("bvid").and_then(Value::as_str).map(str::to_string))
            .take(limit)
            .collect();
    }
    Vec::new()
}

/// 视频搜索（keyword → 搜索结果）。
pub async fn search_videos(client: &Client, keyword: &str, limit: usize) -> Vec<BiliSearchItem> {
    let url = format!(
        "{API_BASE}/x/web-interface/search/type?search_type=video&keyword={}&page=1",
        urlencoding_encode(keyword)
    );
    let data = get_json(client, &url, "https://www.bilibili.com/").await;
    if data.get("code").and_then(Value::as_i64) != Some(0) {
        return Vec::new();
    }
    let result = data
        .get("data")
        .and_then(|d| d.get("result"))
        .and_then(Value::as_array);
    let mut out = Vec::new();
    if let Some(result) = result {
        for r in result.iter().take(limit) {
            let bvid = r.get("bvid").and_then(Value::as_str).unwrap_or("").to_string();
            if bvid.is_empty() {
                continue;
            }
            out.push(BiliSearchItem {
                title: strip_html(r.get("title").and_then(Value::as_str).unwrap_or("").into()),
                up: r.get("author").and_then(Value::as_str).unwrap_or("").to_string(),
                play: r.get("play").and_then(Value::as_u64).unwrap_or(0),
                like: r.get("like").and_then(Value::as_u64).unwrap_or(0),
                desc: r.get("description").and_then(Value::as_str).unwrap_or("").chars().take(80).collect(),
                bvid,
            });
        }
    }
    out
}

/// 学习一个视频：信息 + 弹幕(重复) + 评论 → 存入学习库。
pub async fn learn_video(client: &Client, data_dir: &Path, bvid: &str) -> BiliLearnResult {
    let info = get_video_info(client, bvid).await;
    let title = info.get("title").and_then(Value::as_str).unwrap_or("").to_string();
    if title.is_empty() {
        return BiliLearnResult { ok: false, bvid: bvid.into(), title: String::new(), up: String::new(), danmaku: 0, repeat: 0, comments: 0, culture: String::new() };
    }
    let up = info.get("owner").and_then(|o| o.get("name")).and_then(Value::as_str).unwrap_or("").to_string();
    let tname = info.get("tname").and_then(Value::as_str).unwrap_or("").to_string();
    let vdesc = info.get("desc").and_then(Value::as_str).unwrap_or("").chars().take(500).collect();
    let aid = info.get("aid").and_then(Value::as_u64).unwrap_or(0);
    let cid = info.get("cid").and_then(Value::as_u64).unwrap_or(0);

    // 弹幕：统计重复（高频=流行梗）
    let mut repeat: Vec<String> = Vec::new();
    let mut danmaku_count = 0usize;
    if cid != 0 {
        let url = format!("{API_BASE}/x/v1/dm/list.so?oid={cid}");
        if let Ok(text) = client
            .get(&url)
            .header(reqwest::header::REFERER, "https://www.bilibili.com/")
            .send()
            .await
            .map(|r| r.text())
        {
            if let Ok(xml) = text.await {
                let dms = parse_danmaku(&xml, 500);
                danmaku_count = dms.len();
                repeat = most_common(dms, 15);
            }
        }
    }

    // 高赞评论（sort=2 热度）
    let mut top_comments: Vec<String> = Vec::new();
    if aid != 0 {
        let url = format!("{API_BASE}/x/v2/reply?type=1&oid={aid}&sort=2&ps=30");
        let data = get_json(client, &url, "https://www.bilibili.com/").await;
        if let Some(replies) = data.get("data").and_then(|d| d.get("replies")).and_then(Value::as_array) {
            for r in replies.iter().take(5) {
                if let Some(msg) = r.pointer("/content/message").and_then(Value::as_str) {
                    let text: String = msg.chars().take(100).collect();
                    let like = r.get("like").and_then(Value::as_u64).unwrap_or(0);
                    top_comments.push(format!("[{like}赞] {text}"));
                }
            }
        }
    }

    // LLM 提炼（culture）为后续增强：先存空，保留字段。
    let culture = String::new();

    let mut knowledge = load_knowledge(data_dir);
    // 去重：bvid 相同则覆盖
    knowledge.retain(|v| v.bvid != bvid);
    knowledge.insert(
        0,
        BiliVideo {
            bvid: bvid.into(),
            title,
            up,
            tname,
            vdesc,
            repeat_danmaku: repeat.join(" / "),
            top_comments: top_comments.join(" / "),
            culture,
            learned_at: unix_time_str(),
        },
    );
    if knowledge.len() > 500 {
        knowledge.truncate(500);
    }
    let _ = save_knowledge(data_dir, &knowledge);

    BiliLearnResult {
        ok: true,
        bvid: bvid.into(),
        title: knowledge[0].title.clone(),
        up: knowledge[0].up.clone(),
        danmaku: danmaku_count,
        repeat: repeat.len(),
        comments: top_comments.len(),
        culture: knowledge[0].culture.clone(),
    }
}

/// 查询学习库（q 为空则最新；否则标题/UP/弹幕/文化 模糊匹配）。
pub fn search_knowledge(data_dir: &Path, q: &str, limit: usize) -> Vec<BiliVideo> {
    let mut knowledge = load_knowledge(data_dir);
    if !q.is_empty() {
        let q = q.to_lowercase();
        knowledge.retain(|v| {
            v.title.to_lowercase().contains(&q)
                || v.up.to_lowercase().contains(&q)
                || v.repeat_danmaku.contains(&q)
                || v.culture.contains(&q)
        });
    }
    knowledge.truncate(limit.max(1));
    knowledge
}

// ==================== 工具函数 ====================

/// 统计出现频率最高的前 n 个（用于弹幕去重复 → 流行梗）。
fn most_common(items: Vec<String>, n: usize) -> Vec<String> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for it in items {
        *counts.entry(it).or_insert(0) += 1;
    }
    let mut freq: Vec<(String, usize)> = counts.into_iter().collect();
    freq.sort_by(|a, b| b.1.cmp(&a.1));
    freq.into_iter().take(n).map(|(k, _)| k).collect()
}

/// 极简 URL 编码（B站搜索关键词）。
fn urlencoding_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// 简单去除 HTML 标签（B站搜索标题含 <em class="keyword">）。
fn strip_html(s: String) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

/// 当前时间（unix 秒），作为 learned_at。
fn unix_time_str() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
