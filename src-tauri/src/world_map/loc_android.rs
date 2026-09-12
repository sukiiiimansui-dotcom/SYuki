//! Android **真实定位**桥（`world_map_location` 的第 ② 条路）
//!
//! ## 为什么需要这个文件
//!
//! 打包成 APK 之后，定位原本只有两条路：**手动坐标** 和 **IP 兜底**（只到城市级）。
//! 要拿真实 GPS 有两条路，本工程都走不通：
//!
//! 1. **`tauri-plugin-geolocation`** —— 本机 crates.io **403**，离线缓存里**没有**这个
//!    crate（`~/.cargo/registry/cache/*/ | grep -i geo` 无输出），加进去就没法在本地
//!    `cargo metadata --offline` 验证；而且它的 Android 实现依赖 **Google Play Services**
//!    （`GoogleApiAvailability.isGooglePlayServicesAvailable()` 不等于 SUCCESS 就直接失败），
//!    国内大量机型没有可用 GMS ⇒ 那条路在目标机器上必然失败。
//!    完整论证见 `docs/world-map/17-Android定位方案调研.md` §2。
//!
//! 2. **WebView 的 `navigator.geolocation`** —— tauri 2.11.1 的 Android 胶水里既没有
//!    `WebSettings.setGeolocationEnabled(true)`，也没有
//!    `WebChromeClient.onGeolocationPermissionsShowPrompt`（全 crate grep 无命中），
//!    `getCurrentPosition` 只会走 error 回调；想补就得覆盖 Wry 已设好的 WebChromeClient
//!    （文件选择 / console 都挂在上面）⇒ 破坏现有功能。
//!
//! 所以走**第三条**：Tauri 官方的**应用内 Android 插件**通路 —— Kotlin 插件写在
//! `gen/android/app/src/main/java/com/syuki/lingchat/location/LocationPlugin.kt`，
//! 这里负责注册它、调用它。
//!
//! ## 调用链（每一环都在已缓存的 tauri 2.11.1 / wry 0.55.1 源码里核实过）
//!
//! ```text
//! world_map_location (live.rs)
//!   └─ loc_android::locate(&app, 预算, 缓存最长年龄)
//!       └─ GpsBridge(PluginHandle<Wry>) ← 由 init() 在插件 setup 里 app.manage
//!           └─ PluginHandle::run_mobile_plugin_async("getLocation", {timeoutMs, maxAgeMs})
//!               └─ tauri-2.11.1/src/plugin/mobile.rs:428 run_command() [cfg(target_os="android")]
//!                   └─ JNI: activity.getPluginManager().runCommand(id, "syuki-location", ...)
//!                       └─ PluginManager.runCommand → PluginHandle.invoke → 反射找 @Command
//!                           └─ Kotlin: LocationManager 取定位
//!               ← 原生回调 handlePluginResponse → oneshot → 这里的 Result
//! ```
//!
//! **全程不经过 WebView / JS / capabilities（ACL）**：`run_command` 是直接从 Rust 走 JNI
//! 打到 Kotlin 的（`mobile.rs:450-472`），所以新增这个插件**不给前端增加任何可调用面**，
//! `src-tauri/capabilities/*.json` 一个字都不用改。
//!
//! ## 为什么是 `Wry` 而不是泛型 `R`
//!
//! `PluginHandle<R>` 必须在 `setup` 里 `app.manage()` 进去、再在命令里
//! `app.try_state::<GpsBridge<...>>()` 取出来 —— 两边的 `R` 必须**完全一致**。
//! 本应用只用 `tauri::Builder::default()`（即 `Wry`），命令签名里的 `AppHandle`
//! 也是 `AppHandle<Wry>`。这里直接写死 `Wry`，把这个「两边泛型没对上」的坑焊死；
//! 代价只是以后要支持别的 runtime 得改这一处。
//!
//! ## 平台边界
//!
//! 非 Android 平台上 [`locate`] 直接返回 [`GpsOutcome::Unavailable`]，`init()` 注册的
//! 是一个**空插件**（没有 setup、没有 JS 命令）—— 桌面端行为与改动前**完全一致**：
//! 手动坐标 → IP 兜底。桌面系统定位（Windows/macOS/Linux 各一套 API）不在本次范围内。

// 非 Android 平台上 MobileFix / Fix / LocationRequest / precision_of 都不会被构造或调用
// （只在类型层面用得到），`Manager` 也只在 Android 的 setup 里用 —— 关掉这两类告警，
// 免得桌面端编译日志被刷屏。**只是关告警，不改变任何行为**。
#![cfg_attr(not(target_os = "android"), allow(dead_code, unused_imports))]

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{plugin::PluginHandle, AppHandle, Manager, Wry};

/// Tauri 插件名。Kotlin 侧 `PluginManager.load(webView, name, plugin, config)` 用它当
/// `plugins` 这个 HashMap 的 key，`runCommand(pluginId=...)` 也按它查 —— **两边必须一致**。
///
/// 为什么不叫 `"geolocation"`：那是官方插件的名字，将来若真把官方插件加进来会撞名
/// （`PluginManager.plugins` 是 HashMap，后注册的会覆盖先注册的）。加 `syuki-` 前缀躲开。
const PLUGIN_NAME: &str = "syuki-location";

/// Kotlin 侧包名（`register_android_plugin` 会把 `.` 换成 `/` 再拼类名）。
#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "com.syuki.lingchat.location";

/// Kotlin 侧的类名（同包下）。
#[cfg(target_os = "android")]
const PLUGIN_CLASS: &str = "LocationPlugin";

/// 传给 Kotlin 的入参。字段名用 camelCase —— Kotlin 侧 `LocationArgs` 的字段同名，
/// Jackson 按字段名（`PropertyAccessor.FIELD` + `ANY`）填。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationRequest {
    /// 冷启动最多等多久（毫秒）。由调用方按「前端 6 秒预算」算好传进来。
    pub timeout_ms: u64,
    /// 系统缓存定位的最大可接受年龄（毫秒），负数表示不限。
    pub max_age_ms: i64,
}

/// Kotlin [`Invoke::resolve`] 回来的 JSON。
///
/// 全部字段都给了默认值：Kotlin 侧「精度未知」时会**不传** `accuracy`，
/// 少一个字段不能让整次定位解析失败。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MobileFix {
    lat: f64,
    lng: f64,
    #[serde(default)]
    accuracy: Option<f64>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    age_ms: Option<u64>,
    #[serde(default)]
    cached: bool,
}

/// 一次成功的系统定位。
#[derive(Debug, Clone)]
pub struct Fix {
    pub lat: f64,
    pub lng: f64,
    /// 精度（米）。`None` = Android 没给（`Location.hasAccuracy()` 为 false）。
    pub accuracy: Option<f64>,
    /// Android 的 provider 名：`gps` / `network` / `fused` / `passive` …
    pub provider: Option<String>,
    /// 是不是直接吃的系统缓存（毫秒级返回那种）。仅用于日志。
    pub cached: bool,
    /// 这个定位距现在多久（毫秒）。
    pub age_ms: Option<u64>,
}

impl Fix {
    /// 转成 `WorldLocation` 的附加字段 —— 与 `live.rs` 里 `IpFix::extra()` 同一套写法，
    /// 只是 `source` 换成 `"gps"`、`precision` 按真实精度算。
    pub fn extra(&self) -> serde_json::Value {
        let mut o = serde_json::Map::new();
        o.insert("source".into(), json!("gps"));
        o.insert("precision".into(), json!(precision_of(self.accuracy)));
        // 值是 Option 的字段只在有时才放进去，免得给前端一个 null
        //（`WorldLocation` 里它们声明成可选而不是可空）
        if let Some(p) = &self.provider {
            if !p.is_empty() {
                o.insert("provider".into(), json!(p));
            }
        }
        if let Some(a) = self.accuracy {
            o.insert("accuracy".into(), json!(a));
        }
        serde_json::Value::Object(o)
    }
}

/// 精度（米） → 前端 `WorldLocation.precision`。
///
/// 阈值怎么定的：城市级 IP 定位误差通常十几公里（`source: "ip"` 那条固定给 `"city"`）；
/// 基站/WiFi 定位（Android `network` provider）典型误差 100~3000 米；GPS 冷启动后
/// 很快收敛到 5~30 米。所以：
///   · ≤120 米   —— 能定位到具体路段 → `street`
///   · ≤1200 米  —— 能定位到区县     → `district`
///   · 其它/未知 —— 只能到城市级     → `city`（与 IP 那条保持一致，前端不会看到新值）
fn precision_of(accuracy_m: Option<f64>) -> &'static str {
    match accuracy_m {
        Some(a) if a.is_finite() && a <= 120.0 => "street",
        Some(a) if a.is_finite() && a <= 1200.0 => "district",
        _ => "city",
    }
}

/// 系统定位的结局。**每一种都必须是「可降级」的**，绝不 `Err` 出去 ——
/// `world_map_location` 拿到任何一个都要继续往 IP 兜底走。
#[derive(Debug, Clone)]
pub enum GpsOutcome {
    /// 拿到真定位
    Fix(Fix),
    /// 权限被拒绝 / 用户在系统设置里关了定位 / 用户拒绝授权
    Denied(String),
    /// 预算内没拿到定位（冷启动常见）
    Timeout,
    /// 非 Android 平台，或插件没注册成功
    Unavailable,
    /// 其它失败（没有定位服务、provider 全关、坐标非法 …）
    Failed(String),
}

/// `PluginHandle` 的包装，用来 `app.manage()` 进全局状态。
///
/// 为什么不放 `static`：`PluginHandle<Wry>` 里含 `AppHandle<Wry>`，是 `Send + Sync`，
/// 但存进 `static` 需要 `Lazy<Mutex<..>>` 且要处理中毒；`app.manage/try_state` 是
/// Tauri 的标准做法 —— 本工程已有的 `tauri-plugin-android-fs`
/// （`tauri-plugin-android-fs-28.4.0/src/lib.rs:38-41`）就是这么干的。
pub struct GpsBridge(PluginHandle<Wry>);

/// 注册 `syuki-location` 插件。在 `lib.rs` 的 `tauri::Builder` 上 **无条件** 调用。
///
/// 非 Android 平台上它是一个**没有 setup、没有命令的空插件** —— 除了在插件表里多一个
/// 名字，不产生任何行为。
pub fn init() -> tauri::plugin::TauriPlugin<Wry> {
    #[allow(unused_mut)]
    let mut builder = tauri::plugin::Builder::<Wry>::new(PLUGIN_NAME);

    #[cfg(target_os = "android")]
    {
        builder = builder.setup(|app, api| {
            // ⚠️ 这一步是**阻塞**的：内部 run_on_android_context + `rx.recv()` 等主线程
            // 把 Kotlin 插件 new 出来并 load 进 PluginManager。
            // 时序上安全：PluginManager.onActivityCreate 在 TauriActivity.onCreate 里跑，
            // 而插件 setup 发生在 WryLifecycleObserver.onStart → Rust.start()，
            // **onCreate 早于 onStart**。
            // 官方插件（dialog / android-fs / geolocation）用的都是这一句。
            let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, PLUGIN_CLASS)?;
            app.manage(GpsBridge(handle));
            Ok(())
        });
    }

    builder.build()
}

/// 问系统要一次定位。**永不返回 `Err`**，所有失败都变成 [`GpsOutcome`] 的一个变体。
///
/// * `timeout` —— 交给 Kotlin 的硬超时（冷启动最多等这么久）。
///   这里会在外层再包一层 `timeout + 500ms` 的 tokio 超时：多留这 500ms 是为了让
///   **Kotlin 自己的 reject 先到**（那样能拿到 `PERMISSION_DENIED` / `TIMEOUT` 这种
///   具体错误码，好给出更准确的降级文案）；真等到外层超时，就只剩一句「超时」了。
/// * `max_age` —— 系统缓存定位的最大可接受年龄。
///
/// 非 Android 平台直接返回 [`GpsOutcome::Unavailable`]（`async fn` 里没有 `await`，
/// 不会消耗任何预算）。
#[cfg(not(target_os = "android"))]
pub async fn locate(_app: &AppHandle, _timeout: Duration, _max_age: Duration) -> GpsOutcome {
    GpsOutcome::Unavailable
}

#[cfg(target_os = "android")]
pub async fn locate(app: &AppHandle, timeout: Duration, max_age: Duration) -> GpsOutcome {
    use tauri::plugin::mobile::PluginInvokeError;

    // 先 clone 出句柄再放掉 State —— 后面要跨 await，不能把 State 借出去
    //（`try_state` 而不是 `state`：插件没注册成功时不该 panic）
    let handle = match app.try_state::<GpsBridge>() {
        Some(s) => s.0.clone(),
        None => return GpsOutcome::Unavailable,
    };

    let payload = LocationRequest {
        // `as_millis()` 是 u128；这里最大也就几十秒，截断到 u64 安全
        timeout_ms: timeout.as_millis().min(u64::MAX as u128) as u64,
        max_age_ms: max_age.as_millis().min(i64::MAX as u128) as i64,
    };

    let fut = handle.run_mobile_plugin_async::<MobileFix>("getLocation", payload);
    match tokio::time::timeout(timeout + Duration::from_millis(500), fut).await {
        // 外层超时：Kotlin 连 reject 都没来得及发
        Err(_elapsed) => GpsOutcome::Timeout,
        Ok(Ok(fix)) => GpsOutcome::Fix(Fix {
            lat: fix.lat,
            lng: fix.lng,
            accuracy: fix.accuracy.filter(|a| a.is_finite() && *a > 0.0),
            provider: fix.provider.filter(|p| !p.is_empty()),
            cached: fix.cached,
            age_ms: fix.age_ms,
        }),
        // Kotlin 主动 reject：错误码在 `code` 里（见 LocationPlugin.kt 的 CODE_* 常量）
        Ok(Err(PluginInvokeError::InvokeRejected(resp))) => {
            // 先把两个字段 move 出来，省得跟 `resp` 的借用纠缠
            let code = resp.code.unwrap_or_default();
            let message = resp.message.unwrap_or_default();
            // ⚠️ 这里**故意不用 `match code.as_str()`**：那样 `code` 会被借用贯穿整个
            // match（Rust 2021 的 scrutinee 规则），arm 里就 move 不出来了（E0505）。
            // 用 if/else 比较，借用当场结束。
            if code == "PERMISSION_DENIED" {
                GpsOutcome::Denied(message)
            } else if code == "TIMEOUT" {
                // Kotlin 的硬超时已经烧完了，没必要再给它一次机会
                GpsOutcome::Timeout
            } else if message.is_empty() {
                GpsOutcome::Failed(code)
            } else {
                GpsOutcome::Failed(message)
            }
        }
        // JNI 失败 / 反序列化失败 —— 说明插件桥本身有问题，当成「没有这条路」
        Ok(Err(e)) => GpsOutcome::Failed(e.to_string()),
    }
}
