//! 实时数据通路：**定位**（`world_map_location`）与**天气**（`world_map_weather`）
//!
//! 这两个命令原先只有 HTTP 侧车提供（Python 8790 的 `/api/location`、`/api/weather`，
//! 以及纯 Rust 调试服务 8791 的同名路由）。打包成 APK 之后既没有侧车、也没有那两个
//! 端口，所以前端 `src/api/services/worldMap.ts` 一直走的是**数据层降级**
//! （待补清单就写在那个文件的注释里）。这里把它们搬进应用内 —— 前端那份降级代码
//! 不用改，命令一注册、invoke 一命中，降级分支自然就不再执行。
//!
//! 真源：`~/rikka/Dsh-SYuki/world_map_rs/src/main.rs` 的 `location_api` / `weather_api`
//! / `WEATHER_ZH` / `zh_weather` / `admin_chain` / `location_payload` / `urlencoding`。
//! 那个工程是**只读参考**，本次一个字节都没动它。
//!
//! 搬过来时按「打包之后必须真能跑」改造了四处，每一处都有注释说明原因：
//!
//! 1. **不搬 `termux-location`**（[`world_map_location`] 里有详细说明）。
//! 2. **HTTPS 走 `crate::utils::tls::build_tls_config`**：reqwest 0.13 默认用
//!    rustls-platform-verifier 验系统证书，Android 上未显式初始化会 **TLS panic**
//!    （`utils/tls.rs` 的模块注释写得很清楚，TTS / 创意工坊 / 屏幕分析都踩过）。
//!    裸 `Client::builder().build()` 在这个工程里是错的 —— 本文件统一走 [`http_client`]。
//! 3. **不许出现 libc / `std::os::unix::*`**：Windows 编不过，多平台 CI 会直接拦下来。
//!    时间一律 chrono（真源里 `time_now` 也是这么改的）。
//! 4. **一个依赖都没加**：reqwest / chrono / serde_json / once_cell 本来就在
//!    `Cargo.toml` 里；URL 编码直接用真源自带的 10 行 [`urlencoding`]，
//!    不引 `urlencoding` crate（本机 crates.io 403，能不加就不加）。
//!
//! ## 前端契约（`src/api/services/worldMap.ts`，已逐行核对）
//!
//! | 命令 | 前端怎么调 | 前端给它套的超时 |
//! |---|---|---|
//! | `world_map_location` | `invoke('world_map_location', { force, fast, lat, lng })` | **6 秒** |
//! | `world_map_weather`  | `invoke('world_map_weather', { city })` | **5 秒** |
//!
//! 右边这列比参数更重要：前端超时之后**直接走降级、Rust 的结果被丢掉**。
//! 所以本文件里每个网络超时都按「留出 IPC 余量」挑过值，理由写在常量旁边。
//! 另外两个命令**一律返回 `Ok(对象)`、绝不返回 `Err`**：前端是按对象取字段的
//! （`w?.current ?? w` 那套），返回 Err 只会让它多打一次注定失败的降级 HTTP 请求。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use serde_json::{json, Value};
use tauri::AppHandle;

use super::geo;

// ═══════════════════════════════════════════════════════════════════
// 网络预算常量
// ═══════════════════════════════════════════════════════════════════

/// ip-api.com 免费接口**只有 HTTP**（HTTPS 要付费版，实测直接 403）。
///
/// 明文流量在 release APK 里被 `usesCleartextTraffic`（`gen/android/app/build.gradle.kts`
/// 里 debug=true / release=**false**）关着 —— 但那条策略由 **Java 侧**网络栈执行
/// （OkHttp / HttpURLConnection / WebView 各自查 `NetworkSecurityPolicy`），
/// **拦不到 Rust 的原生 socket**。同工程的 `lan_sync/client.rs` 就一直这么发
/// `http://<host>:<port>/manifest`，可以佐证。
/// （真机 APK 内实测留作 CI 之后的验收项，见交付说明里的「不确定的地方」。）
const IP_API_URL: &str = "http://ip-api.com/json/?lang=zh-CN&fields=status,lat,lon,city,regionName";

/// IP 定位超时（`fast` 未置位时）。真源就是这个值：HTTP 版没有客户端超时，
/// 8 秒是纯网络上限。
const IP_TIMEOUT: Duration = Duration::from_secs(8);

/// IP 定位超时（`fast: true`）。
///
/// **两个前端调用方都传了 `fast: true`**（`WorldMap.vue` 的定位按钮、
/// `DistrictLive.vue` 的 locate()），所以这条才是实际生效的那个。
///
/// 为什么不能照抄 8 秒：`worldMap.ts` 给这个命令套了 **6 秒** 的 `withTimeout`，
/// 一超就 reject → 走降级 → 用户看到的是前端写死的那句「定位不可用」。
/// 压到 4 秒，命令就能在预算内**自己**返回 `{error, hint}`（带具体原因），
/// `WorldMap.vue::relocate()` 判 `loc.error` 之后显示的是我们给的 hint，信息量更大。
/// 4 秒 + 下面 `GEOCODE_BUDGET_FAST` 的 1.2 秒 = 5.2 秒，卡在 6 秒预算里。
const IP_TIMEOUT_FAST: Duration = Duration::from_secs(4);

/// 反查城市名（给天气用的 IP 探测）超时。
///
/// 比 [`IP_TIMEOUT`] 短是故意的：这一步只要一个**名字**，拿不到就退 `"Beijing"`，
/// 那本来就是个可接受的默认值；按 8 秒等满只会把整个命令再拖长 8 秒，
/// 而前端 5 秒就已经放弃这个 invoke 了。
const CITY_LOOKUP_TIMEOUT: Duration = Duration::from_secs(3);

/// 整个 `world_map_location` 的墙钟预算（`fast: true`）。
/// 前端套的是 6000ms，这里留 0.8 秒余量给 IPC 序列化和往返。
const LOCATION_BUDGET_FAST: Duration = Duration::from_millis(5200);
/// 不传 `fast` 时的总预算（目前没有调用方走这条，等于「IP 8 秒 + 反查 3 秒」再放一点）
const LOCATION_BUDGET: Duration = Duration::from_secs(11);

/// 反向地理编码（坐标 → 省市区路径）的**单步上限**。
///
/// `geo::GeoSource::fetch` 在缓存缺失时会**联网**，而且一次要试 `_full` 和普通两个
/// 地址、每个 20 秒 —— 冷缓存下整条链能跑到几十秒，足以把定位按钮拖死。
/// 所以给它一个硬墙钟：超时就**只返回坐标**、不带 `path`/`area`/`leaf`
/// （`WorldLocation` 里这三个字段本来就是可选的），绝不把页面卡住。
///
/// 实际用的是 `min(这个上限, 总预算还剩多少)`（见 [`remaining`]）——
/// IP 查得快就多给反查留时间，IP 慢就自动收窄，永远不越过总预算。
/// 这一点对 `DistrictLive.vue` 的 `locate()` 很关键：它**必须要** `path`
/// 才能拼出「市·区」当区域名，拿不到就直接报「定位结果没有行政区信息」。
const GEOCODE_MAX: Duration = Duration::from_secs(3);

/// wttr.in 上游超时：真源就是这个值，照搬。
///
/// ⚠️ 前端给这个命令套的是 **5 秒**（`withTimeout(..., 5000, '天气')`），比 25 秒小。
/// 这里**故意保留 25 秒**而不是砍到 5 秒以内，理由是：
/// Tauri 的 invoke 在前端 reject 之后，Rust 这边的 future 照跑不误，跑完会写进
/// [`WEATHER_CACHE`]；下一次调用（重新进页面、切页签）就是缓存命中、秒回。
/// 本机实测 wttr.in 正常在 1～2.5 秒返回，常见路径本来就落在 5 秒内。
/// 若要把「首次进页面就有天气」也保证下来，改前端那一行（5000 → 26000）
/// 比在这里砍超时更对 —— 砍了连缓存都暖不上。
const WEATHER_TIMEOUT: Duration = Duration::from_secs(25);

/// wttr.in 的 User-Agent。默认 UA（`reqwest/x.y`）会被 wttr.in 判成脚本，
/// 回一段「请用浏览器访问」的提示而不是 JSON，所以必须伪装 curl。
const WEATHER_UA: &str = "curl/8.0";

/// 天气缓存有效期（秒）。真源同值 —— wttr.in 偶发很慢，不能让页面被它拖住。
const WEATHER_TTL_SECS: u64 = 1800;

// ═══════════════════════════════════════════════════════════════════
// 命令一：世界地图定位
// ═══════════════════════════════════════════════════════════════════

/// `world_map_location` — 定位（坐标 + 行政区路径）。
///
/// 前端 `worldMapApi.location(opts)` 调它，`opts` 是 `{ force?, fast?, lat?, lng? }`；
/// 四个参数在 Rust 侧全是 `Option<T>`，不传即 `None`（Tauri 对 `Option` 参数就是这么
/// 处理的，前端 `{...opts}` 里的 `undefined` 会在序列化时被丢掉）。
///
/// ## 三条路的优先级
///
/// ① **手动坐标**（`lat`+`lng` 都给且合法）—— 最先判，不碰网络、不碰权限、永远可用。
/// ② **系统定位** —— **故意留空**，见下面那段长注释（要做得先加插件 + 加权限）。
/// ③ **IP 定位兜底**（ip-api.com）—— 永远不挂起，代价是只到城市级。
///
/// 三条都不成，返回 `{error, hint, source:"none"}` 对象而不是 `Err`：
/// `WorldMap.vue` 判的就是 `loc.error`，拿到就把 hint 显示出来并回到默认城市。
///
/// ## 为什么没有 `termux-location`
///
/// 真源的第二条路是 `tokio::process::Command::new("termux-location")`。**这条绝对不能搬**：
/// 它是 Termux:API 这个独立应用提供的命令，APK 里 `Command::new` 只会拿到 ENOENT；
/// 更糟的是它在**未授权时会挂起不返回**（真源为此专门加了 8 秒超时 + `?force=1` 熔断）。
/// 换句话说，搬过来不是「不生效」，是「可能把地图首页卡死」。
///
/// ## 要补真定位得先做什么（本次**没做**，因为前提都不满足）
///
/// 1. **加插件**：`Cargo.toml` 里现在**没有** `tauri-plugin-geolocation`
///    （已核对全文，也没有任何 geolocation 相关依赖）。
/// 2. **加 Android 权限**：`src-tauri/gen/android/app/src/main/AndroidManifest.xml`
///    目前只有 `INTERNET` / `RECORD_AUDIO` / `MODIFY_AUDIO_SETTINGS`，
///    **没有** `ACCESS_FINE_LOCATION` / `ACCESS_COARSE_LOCATION`。
///    注意这两个是 *dangerous* 权限，光在 manifest 里声明没用，还要**运行时**申请。
/// 3. **走 WebView 的 `navigator.geolocation` 也不行**：Tauri 的 Android WebView 胶水
///    （`tauri-2.11.1/mobile/android/src/main/java/app/tauri/`）里既没有
///    `WebSettings.setGeolocationEnabled(true)`，也没有
///    `WebChromeClient.onGeolocationPermissionsShowPrompt`（全 crate grep 无命中）。
///    少了这两个，`navigator.geolocation.getCurrentPosition` 只会走 error 回调。
///    要做得在 Android 侧自己接这两个回调，属于「改壳」而不是「加命令」。
/// 4. 桌面端还得另配一套后端（同一个插件在 Windows/macOS/Linux 上是不同实现）。
///
/// 结论：定位精度这条留给「加插件 + 加权限」的独立改动，本次只上①③两条永远可用的。
#[tauri::command]
pub async fn world_map_location(
    app: AppHandle,
    lat: Option<f64>,
    lng: Option<f64>,
    fast: Option<bool>,
    force: Option<bool>,
) -> Result<Value, String> {
    // `force` 是 `WorldMap.vue::relocate()` 传下来的（`{ force: true, fast: true }`）。
    // 真源里它的用处是「清掉 termux-location 的熔断标志」；这里根本没有那条路，
    // 也就没有可清的状态。收下不用 —— 只为保住调用点的参数形状，前端不用改。
    let _ = force;

    let fast = fast.unwrap_or(false);
    // 整条命令共用一个截止时刻：后面每个可能联网的步骤都按「还剩多少」收口，
    // 不会出现「IP 先花 4 秒、反查再花 3 秒」把前端的 6 秒预算撑爆的情况。
    let deadline = Instant::now()
        + if fast {
            LOCATION_BUDGET_FAST
        } else {
            LOCATION_BUDGET
        };

    // ── ① 手动坐标 ──────────────────────────────────────────────
    if let (Some(la), Some(ln)) = (lat, lng) {
        if !la.is_finite() || !ln.is_finite() || la.abs() > 90.0 || ln.abs() > 180.0 {
            return Ok(json!({
                "lat": 0.0,
                "lng": 0.0,
                "source": "none",
                "error": "坐标不合法",
                "hint": format!("lat/lng 超出范围：lat={la}, lng={ln}"),
            }));
        }
        let budget = remaining(deadline);
        return Ok(location_payload(&app, la, ln, json!({"source": "manual"}), budget).await);
    }

    // ── ② 系统定位：故意留空，理由见函数文档 ──────────────────────

    // ── ③ IP 兜底（不依赖任何权限，代价是只到城市级）──────────────
    let timeout = if fast { IP_TIMEOUT_FAST } else { IP_TIMEOUT };
    match ip_locate(timeout).await {
        Some(ip) => {
            let budget = remaining(deadline);
            Ok(location_payload(&app, ip.lat, ip.lng, ip.extra(), budget).await)
        }
        None => Ok(json!({
            "lat": 0.0,
            "lng": 0.0,
            "source": "none",
            "error": "定位失败",
            "hint": "IP 定位没拿到结果（断网或 ip-api.com 不可达），可手动选择区域",
        })),
    }
}

/// 截止时刻前还剩多少（已过就返回 0），并对 [`GEOCODE_MAX`] 取小。
/// 钳到 0 也安全：`tokio::time::timeout` 拿到 0 会**立刻**返回 `Elapsed`，
/// 于是反查被跳过、命令照样按时返回。
fn remaining(deadline: Instant) -> Duration {
    deadline
        .saturating_duration_since(Instant::now())
        .min(GEOCODE_MAX)
}

/// 坐标 + 行政路径载荷。真源 `location_payload` 的 1:1 移植，多了两处改动：
///   · 反查路径的预算由调用方按**剩余总预算**算好传进来（见 [`remaining`]）
///   · 超时/失败时**静默降级**成「只有坐标」，不报错
async fn location_payload(app: &AppHandle, lat: f64, lng: f64, extra: Value, budget: Duration) -> Value {
    let mut out = json!({"lat": lat, "lng": lng});
    if let Some(obj) = extra.as_object() {
        for (k, v) in obj {
            out[k] = v.clone();
        }
    }
    if let Some(more) = reverse_path(app, lat, lng, budget).await {
        if let (Some(dst), Some(src)) = (out.as_object_mut(), more.as_object()) {
            for (k, v) in src {
                dst.insert(k.clone(), v.clone());
            }
        }
    }
    out
}

/// 坐标 → `{path, area, leaf}`（省 → 市 → 区县）。拿不到就 `None`。
///
/// 注意 [`super::make_source`] 与 `geo::nearest_cached` 是**同步**的
/// （读缓存目录 + 解析 JSON），tokio 的 `timeout` 只能在 `.await` 点抢占，
/// 管不到它们。这里的预算实际是给下面 `fetch` 里的**联网**用的 ——
/// 那才是会跑到几十秒的地方。同步那段的耗时由缓存规模决定（几十个文件），
/// 与 `world_map_blocks_at` 走的是同一条老路，不额外设卡。
async fn reverse_path(app: &AppHandle, lat: f64, lng: f64, budget: Duration) -> Option<Value> {
    let src = super::make_source(app);
    let (ad, _d) = geo::nearest_cached(&src, lng, lat)?;
    let fut = async {
        let mut path: Vec<Value> = vec![json!({"adcode": "100000", "name": "中国"})];
        let mut parent = "100000".to_string();
        for code in admin_chain(&ad) {
            // 名字必须去**上一级**的子列表里查：某个区县的名字只会出现在它所属城市的
            // geojson 里（区县自己那份文件装的是它的下一级，不含自己的名字）。
            let name = match src.fetch(&parent).await {
                Ok(fc) => geo::features(&fc, Some(&parent))
                    .into_iter()
                    .find(|f| f.adcode == code)
                    .map(|f| f.name),
                Err(_) => None,
            };
            path.push(json!({"adcode": code, "name": name.unwrap_or_else(|| code.clone())}));
            parent = code;
        }
        let names: Vec<String> = path
            .iter()
            .filter_map(|p| p.get("name").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .collect();
        json!({
            "path": path,
            "area": names.join("·"),
            "leaf": {
                "adcode": ad,
                "name": names.last().cloned().unwrap_or_else(|| ad.clone()),
            },
        })
    };
    tokio::time::timeout(budget, fut).await.ok()
}

/// 6 位 adcode → 它的行政链（省 → 市 → 区县），已去重。
/// 为什么用算术而不是查表：中国行政区划编码是**层级前缀制**
/// （44 0000 广东省 → 4401 00 广州市 → 440103 荔湾区），前缀推父级永远成立，
/// 不必额外维护一张父级映射表。
fn admin_chain(ad: &str) -> Vec<String> {
    if ad.len() != 6 || !ad.chars().all(|c| c.is_ascii_digit()) {
        return vec![ad.to_string()];
    }
    let province = format!("{}0000", &ad[..2]);
    let city = format!("{}00", &ad[..4]);
    let mut out = vec![province.clone()];
    if city != province {
        out.push(city.clone());
    }
    if ad != city && ad != province {
        out.push(ad.to_string());
    }
    out
}

// ═══════════════════════════════════════════════════════════════════
// IP 定位（两条命令共用）
// ═══════════════════════════════════════════════════════════════════

/// ip-api.com 的裁剪结果。天气那条要用 [`IpFix::city`] 反查城市名，
/// 定位那条要用 [`IpFix::extra`] 补齐 `source`/`precision`/`ip_city`/`ip_region`
/// —— 这四个字段是 `WorldLocation` 里声明过的，前端虽然暂时没读，
/// 但留着才能在 UI 上区分「手动 / IP」。
struct IpFix {
    lat: f64,
    lng: f64,
    city: Option<String>,
    region: Option<String>,
}

impl IpFix {
    fn extra(&self) -> Value {
        let mut o = serde_json::Map::new();
        o.insert("source".into(), json!("ip"));
        // 只到城市级 —— 前端用它决定要不要提示「精度有限」
        o.insert("precision".into(), json!("city"));
        // 值是 Option 的字段只在有时才放进去，免得给前端一个 null
        // （`WorldLocation` 里它们声明成可选而不是可空）
        if let Some(c) = &self.city {
            o.insert("ip_city".into(), json!(c));
        }
        if let Some(r) = &self.region {
            o.insert("ip_region".into(), json!(r));
        }
        Value::Object(o)
    }
}

/// IP 定位。任何一步失败都返回 `None`（调用方各自兜底），**绝不挂起**。
///
/// 真源是同一个 URL、同一个 8 秒；这里只是把「建客户端」换成了带预配置 TLS 的
/// [`http_client`]（裸 builder 在 Android 上会 TLS panic，见文件头第 2 条）。
async fn ip_locate(timeout: Duration) -> Option<IpFix> {
    let client = http_client(timeout, None).ok()?;
    let resp = client.get(IP_API_URL).send().await.ok()?;
    let v = resp.json::<Value>().await.ok()?;
    // ip-api 失败时 HTTP 仍是 200，靠 `status` 字段区分（`fail` / `success`）
    if v.get("status").and_then(|s| s.as_str()) != Some("success") {
        return None;
    }
    let lat = v.get("lat").and_then(|x| x.as_f64())?;
    let lng = v.get("lon").and_then(|x| x.as_f64())?;
    Some(IpFix {
        lat,
        lng,
        city: opt_str(&v, "city"),
        region: opt_str(&v, "regionName"),
    })
}

/// 取一个非空字符串字段（空的当没有）
fn opt_str(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 建一个带**预配置 TLS** 的 reqwest 客户端（全工程 HTTPS 的统一做法）。
///
/// 不能用裸 `reqwest::Client::builder().build()`：reqwest 0.13 默认走
/// rustls-platform-verifier 验系统证书，Android 上没显式初始化会 TLS panic
/// —— `crate::utils::tls` 的模块注释里写着这条，TTS / 创意工坊 / 屏幕分析都踩过。
///
/// `ua` 传 `Some` 时才设 User-Agent：wttr.in 需要 `curl/8.0`，
/// ip-api.com 无所谓（保持默认更省事）。
fn http_client(timeout: Duration, ua: Option<&str>) -> Result<reqwest::Client, String> {
    let tls = crate::utils::tls::build_tls_config()?;
    let mut b = reqwest::Client::builder()
        .timeout(timeout)
        .tls_backend_preconfigured(tls);
    if let Some(ua) = ua {
        b = b.user_agent(ua);
    }
    b.build().map_err(|e| format!("HTTP 客户端创建失败: {e}"))
}

// ═══════════════════════════════════════════════════════════════════
// 命令二：天气
// ═══════════════════════════════════════════════════════════════════

/// `world_map_weather` — 真实天气（wttr.in，免费无需 key）。
///
/// 返回字段与真源**逐字段一致**（前端 `world_weather.js` / `phone.js` 的
/// `normalize()` 就吃这一套）：
/// `city / desc_en / desc / temp_c / feels_like_c / humidity / wind_kmph /
/// cloudcover / precip_mm / visibility_km / is_rain / is_snow / is_fog / cached`。
///
/// `city` 不传时的三级兜底：参数 → IP 反查城市名 → `"Beijing"`。
/// 目前两个调用点（`WorldMap.vue::loadTimeWeather()` 的 `weather()`、
/// 手机天气页）都不传城市，所以实际走的是 IP 反查那条。
///
/// **失败返回对象而不是 `Err`**：`{"error": "...", "city": ...}`。
/// 前端拿到对象后 `const cur = w.current ?? w` 取 `temp_c`，取不到就把顶栏那个
/// 天气标签留空 —— 这是「少一个标签」，不是错误。返回 Err 反而会让它再打一次
/// 注定失败的降级 HTTP 请求，白等 5 秒。
#[tauri::command]
pub async fn world_map_weather(city: Option<String>) -> Result<Value, String> {
    let city = match city.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        Some(c) => c,
        None => match ip_locate(CITY_LOOKUP_TIMEOUT).await.and_then(|ip| ip.city) {
            Some(c) => c,
            // wttr.in 认这个英文名；真源同值
            None => "Beijing".to_string(),
        },
    };

    let now = chrono::Utc::now().timestamp().max(0) as u64;

    // ① 缓存命中（30 分钟内 + 同一个城市）—— 真源同逻辑
    {
        let c = WEATHER_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if !c.2.is_null() && c.1 == city && now.saturating_sub(c.0) < WEATHER_TTL_SECS {
            let mut d = c.2.clone();
            // 拷出来再标 cached，别污染缓存里那份
            d["cached"] = json!(true);
            return Ok(d);
        }
    } // ← 锁在这里就放掉了：下面有 await，**绝不能持锁跨 await**（会死锁）

    // ② 打 wttr.in
    let client = match http_client(WEATHER_TIMEOUT, Some(WEATHER_UA)) {
        Ok(c) => c,
        Err(e) => return Ok(json!({"error": e, "city": city})),
    };
    let url = format!("https://wttr.in/{}?format=j1&lang=zh", urlencoding(&city));
    let data = match client.get(&url).send().await {
        Ok(r) => match r.json::<Value>().await {
            Ok(v) => v,
            Err(e) => return Ok(json!({"error": format!("天气解析失败: {e}"), "city": city})),
        },
        Err(e) => return Ok(json!({"error": format!("天气请求失败: {e}"), "city": city})),
    };
    let cc = match data
        .get("current_condition")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
    {
        Some(c) => c,
        None => return Ok(json!({"error": "天气数据为空", "city": city})),
    };

    // wttr.in 的这些字段全是**字符串**（`"temp_C": "28"`），要自己 parse
    let g = |k: &str| cc.get(k).and_then(|v| v.as_str()).unwrap_or("0");
    let g_f = |k: &str| g(k).parse::<f64>().unwrap_or(0.0);
    // 实测即使带 `lang=zh`，`weatherDesc` 与 `lang_zh` 回的都是**英文**
    // （本机 curl 验过），所以中文必须靠下面这张映射表自己换
    let raw = cc
        .get("weatherDesc")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|o| o.get("value"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let desc = zh_weather(raw);

    let out = json!({
        "city": city,
        "desc_en": raw,
        "desc": desc,
        "temp_c": g_f("temp_C") as i64,
        "feels_like_c": g_f("FeelsLikeC") as i64,
        "humidity": g_f("humidity") as i64,
        "wind_kmph": g_f("windspeedKmph") as i64,
        "cloudcover": g_f("cloudcover") as i64,
        "precip_mm": g_f("precipMM"),
        "visibility_km": g_f("visibility") as i64,
        // 三个布尔量是 T4-2 天气视觉表现的开关（雨/雪/雾粒子），
        // 与 `world_weather.js::kindOf()` 的判定顺序保持一致：雪 → 雨 → 雾
        "is_rain": desc.contains('雨') || desc.contains('雷'),
        "is_snow": desc.contains('雪'),
        "is_fog": desc.contains('雾') || desc.contains('霾'),
        "cached": false,
    });

    // ③ 写缓存（锁同样不跨 await）
    {
        let mut c = WEATHER_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        *c = (
            now,
            out["city"].as_str().unwrap_or("").to_string(),
            out.clone(),
        );
    }
    Ok(out)
}

/// 天气缓存（30 分钟）：`(写入时刻, 城市, 载荷)`。
///
/// 进程级静态 —— 真源同款。为什么用 `Mutex` 而不是 `RwLock`：读也要写
/// （命中时要把 `cached` 标成 `true`），读多写少的收益在这里没有意义。
/// 取锁一律 `unwrap_or_else(|e| e.into_inner())`：中毒也只是少一次缓存命中，
/// 不能让整条命令跟着崩（工程里 `bridge.rs` 是同一套写法）。
static WEATHER_CACHE: Lazy<Mutex<(u64, String, Value)>> =
    Lazy::new(|| Mutex::new((0, String::new(), Value::Null)));

/// 天气中文映射（与 Python 侧 / 真源同一张表，33 条）
const WEATHER_ZH: [(&str, &str); 33] = [
    ("Sunny", "晴"),
    ("Clear", "晴"),
    ("Partly cloudy", "局部多云"),
    ("Cloudy", "多云"),
    ("Overcast", "阴"),
    ("Mist", "薄雾"),
    ("Fog", "雾"),
    ("Freezing fog", "冻雾"),
    ("Patchy rain possible", "可能有零星小雨"),
    ("Patchy rain nearby", "附近有零星小雨"),
    ("Light drizzle", "毛毛雨"),
    ("Light rain", "小雨"),
    ("Moderate rain", "中雨"),
    ("Heavy rain", "大雨"),
    ("Light rain shower", "阵雨"),
    ("Moderate or heavy rain shower", "中到大阵雨"),
    ("Torrential rain shower", "暴雨"),
    ("Patchy light rain", "零星小雨"),
    ("Moderate rain at times", "间歇中雨"),
    ("Heavy rain at times", "间歇大雨"),
    ("Light snow", "小雪"),
    ("Moderate snow", "中雪"),
    ("Heavy snow", "大雪"),
    ("Blizzard", "暴雪"),
    ("Patchy snow possible", "可能有雪"),
    ("Patchy light snow", "零星小雪"),
    ("Thundery outbreaks possible", "可能有雷阵雨"),
    ("Thundery outbreaks in nearby", "附近有雷阵雨"),
    ("Patchy light rain with thunder", "零星雷雨"),
    ("Moderate or heavy rain with thunder", "中到大雷雨"),
    ("Light freezing rain", "冻雨"),
    ("Moderate or heavy freezing rain", "强冻雨"),
    ("Light sleet", "雨夹雪"),
];

/// 英文描述 → 中文。两级：先查 33 条精确表，再按关键词模糊兜底
/// （wttr.in 的描述串比这张表多得多，比如 "Patchy rain nearby" 的变体）。
fn zh_weather(desc: &str) -> String {
    let d = desc.trim();
    for (en, zh) in WEATHER_ZH {
        if d == en {
            return zh.to_string();
        }
    }
    let low = d.to_lowercase();
    let hit = [
        ("thunder", "雷雨"),
        ("blizzard", "雪"),
        ("snow", "雪"),
        ("sleet", "雨夹雪"),
        ("drizzle", "雨"),
        ("shower", "雨"),
        ("rain", "雨"),
        ("fog", "雾"),
        ("mist", "雾"),
        ("cloud", "多云"),
        ("sunny", "晴"),
        ("clear", "晴"),
    ];
    for (k, zh) in hit {
        if low.contains(k) {
            return zh.to_string();
        }
    }
    // 都不认就把英文原样透出去 —— 前端 `kindOf()` 还会再按英文关键词兜一层
    d.to_string()
}

/// 极简 URL 编码（城市名可能是中文，必须转义）。
///
/// 真源自带这 10 行，所以**不引 `urlencoding` crate**（本机 crates.io 403，
/// 能不加依赖就不加）。保留字符集与 `encodeURIComponent` 一致，
/// 非 ASCII 一律 `%XX` 逐字节转 —— wttr.in 的路径段就吃这个格式
/// （本机实测 `%E9%87%8D%E5%BA%86%E5%B8%82` → 200 + 重庆的天气）。
fn urlencoding(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
