//! 网易云音乐模块（公开接口：搜索 / 心情·时段推荐 / 外链播放）。
//! 对标原 SYuki `jukebox_engine.py` + `music_login.py`（公开部分）。
//!
//! 用网易云公开搜索接口（无需登录），按情绪/时段映射关键词推荐，
//! 返回歌曲 + 外链播放地址。登录/VIP 歌单等 weapi 加密能力暂不引入。

use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde::Serialize;
use serde_json::Value;

const UA: &str = "Mozilla/5.0 (Linux; Android 13) AppleWebKit/537.36";
const API_BASE: &str = "https://music.163.com";
const NET_TIMEOUT: Duration = Duration::from_secs(15);

/// 一首歌
#[derive(Debug, Clone, Serialize)]
pub struct NetMusicSong {
    pub source: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub url: String,
    pub cover: String,
    pub duration: u64,
}

/// 构建 HTTP 客户端（统一 TLS webpki，与 bilibili_service 一致）。
pub fn build_client() -> Result<Client, String> {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .roots
        .extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls_config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| format!("rustls 配置失败: {e}"))?
    .with_root_certificates(Arc::new(roots))
    .with_no_client_auth();

    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(NET_TIMEOUT)
        .tls_backend_preconfigured(tls_config)
        .user_agent(UA)
        .build()
        .map_err(|e| format!("创建网易云 HTTP 客户端失败: {e}"))
}

/// 网易云公开搜索接口（POST form），返回歌曲列表。
async fn search_netease(client: &Client, keyword: &str, limit: usize) -> Vec<NetMusicSong> {
    let resp = client
        .post(format!("{API_BASE}/api/search/get/web"))
        .header(reqwest::header::REFERER, "https://music.163.com/")
        .form(&[
            ("s", keyword.to_string()),
            ("type", "1".to_string()),
            ("limit", limit.max(1).to_string()),
            ("offset", "0".to_string()),
        ])
        .send()
        .await;
    let Ok(resp) = resp else { return Vec::new() };
    let Ok(data) = resp.json::<Value>().await else {
        return Vec::new();
    };
    let Some(songs) = data.pointer("/result/songs").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for s in songs.iter().take(limit.max(1)) {
        let id = s.get("id").and_then(Value::as_u64).unwrap_or(0);
        let title = s.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        if title.is_empty() {
            continue;
        }
        let artists = s
            .get("artists")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.get("name").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let album = s
            .pointer("/album/name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let cover = s
            .pointer("/album/picUrl")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let duration = s.get("duration").and_then(Value::as_u64).unwrap_or(0) / 1000;
        out.push(NetMusicSong {
            source: "netease".into(),
            title,
            artist: artists,
            album,
            url: playing_url(id),
            cover,
            duration,
        });
    }
    out
}

/// 搜索歌曲（公开接口）。
pub async fn search_songs(client: &Client, keyword: &str, limit: usize) -> Vec<NetMusicSong> {
    search_netease(client, keyword, limit).await
}

/// 心情/时段推荐：把 mood 映射成搜索关键词。
pub async fn recommend(client: &Client, mood: &str, limit: usize) -> Vec<NetMusicSong> {
    let kw: &str = match mood {
        "happy" => "元气 歌曲",
        "sad" => "治愈 慢歌",
        "angry" => "燃 摇滚",
        "sleepy" => "轻音乐 助眠",
        "calm" => "纯音乐",
        "nostalgic" => "经典老歌",
        _ => "流行 歌曲",
    };
    search_netease(client, kw, limit).await
}

/// 外链播放地址（免费歌可直链；VIP 受版权限制需登录）。
pub fn playing_url(song_id: u64) -> String {
    format!("{API_BASE}/song/media/outer/url?id={song_id}.mp3")
}

/// helper：mood 归一化。
pub fn normalize_mood(mood: &str) -> String {
    mood.trim().to_lowercase()
}
