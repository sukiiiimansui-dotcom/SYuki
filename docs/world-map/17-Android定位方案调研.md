# 17 · Android 真实定位（GPS）方案调研

> 目标：让「世界模拟」在 Android 上从**城市级 IP 兜底**升级到**真实 GPS**（街道/区县级）。
> 本文只回答一个问题：**在「本机 crates.io 403、只能在离线缓存里取 crate、不能在手机上编译」这三条约束下，哪条路真能走通。**
>
> 调研日期：2026-09-12 · 调研对象：`~/lingchat-main` @ `feat/world-map` · tauri 2.11.1 / tauri-cli 2.x

---

## 0. 结论速览

| # | 路子 | 可行性 | 结论 |
|---|---|---|---|
| **A** | `tauri-plugin-geolocation` 官方插件 | ⚠️ **能跑但本机验不了** | 离线缓存里**没有**这个 crate；且它的 `timeout` 在 Android 上**被忽略**（源码原文），与本项目「6 秒预算」硬冲突。**不采用** |
| **B** | **自写 Kotlin 插件 + Tauri 移动插件桥** | ✅ **推荐** | **零新 crate**，全部 API 都在已缓存的 `tauri-2.11.1` 源码里核实过；本工程 `tauri-plugin-android-fs` 走的**就是这条同款通路**（现成先例）。✅ 采用 |
| **C** | 纯 Rust 直调 JNI（不写 Kotlin） | ❌ 不成立 | 三个硬阻断：拿不到 Activity（`tauri::runtime` 非 pub）、`LocationListener` 是**接口**（Rust 无法实现）、运行时权限弹窗必须有 Activity 回调 |
| **D** | 其它旁路（`dumpsys` / `/proc` / WebView `navigator.geolocation` / `termux-location`） | ❌ 全部否掉 | 逐条证据见 §5 |

**最终推荐：路子 B —— 已按 B 落地实施**（改了哪些文件、各多少行见 §10.1）。
只动 `src-tauri/**`（含 `gen/android/**`），对「只新增、不改现有功能」这条底线**零破坏**（逐条论证见 §3.6）。

---

## 1. 现状与硬约束（先把事实钉死）

### 1.1 现在只有两条路

`src-tauri/src/world_map/live.rs:164` 的 `world_map_location`，优先级写死在 `live.rs:187~218`：

| 顺序 | 位置 | 说明 |
|---|---|---|
| ① 手动坐标 | `live.rs:188~200` | `lat`+`lng` 都给才走，不碰网络不碰权限 |
| ② **系统定位** | `live.rs:202` | **只有一行注释，是空的** |
| ③ IP 兜底 | `live.rs:204~218` | `ip-api.com`，只到城市级（`precision: "city"`） |

### 1.2 前端契约（不能改，`src/` 归另一个 agent）

`src/api/services/worldMap.ts:94~107` 的 `WorldLocation`：

```ts
{ lat, lng, accuracy?, provider?, source?, precision?, ip_city?, ip_region?, error?, hint?, path?, leaf? }
```

注意 `accuracy` / `provider` **已经在类型里声明了**（`worldMap.ts:97-98`）—— 这正是给 GPS 预留的字段，现在没人填。

两个调用点：

| 调用方 | 传参 | 用到的返回字段 |
|---|---|---|
| `WorldMap.vue:399` `relocate()` | `{force:true, fast:true}` | `loc.error` / `loc.hint` / `loc.lat` / `loc.lng` |
| `DistrictLive.vue:542` `locate()` | `{fast:true}` | **`d.path`**（拼「市·区」当区域名），没有 `path` 就报「定位结果没有行政区信息」 |

### 1.3 ⚠️ 前端硬超时 6 秒 —— 比「GPS 能等多久」更重要

`worldMap.ts:270`：

```ts
return await withTimeout(invoke<WorldLocation>('world_map_location', { ...opts }), 6000, '定位')
```

`worldMap.ts:209` 的 `withTimeout` 一到点就 **reject → 走降级**，Rust 侧算出来的结果**直接被丢掉**。
所以「GPS 冷启动最多等 6~8 秒」这句话在**实际链路上只能兑现 ~5.2 秒**（`live.rs:86` 的 `LOCATION_BUDGET_FAST` 就是这么定的，留 0.8 秒给 IPC）。
本文 §8 的预算表按这个真实上限设计，而不是按 8 秒。

### 1.4 本机约束

| 约束 | 影响 |
|---|---|
| crates.io **403**，只能 `--offline` 用 `~/.cargo/registry/cache/` 里的 crate | **加新依赖＝无法在本地验证解析**（`cargo metadata --offline` 会直接失败） |
| 手机上**不能编译**（Tauri 全量 40 分钟起） | Rust 侧只能「读源码核实 + `rustc --emit=metadata` 语法快检」 |
| 没有 Android SDK / kotlinc | **Kotlin 侧本地编不了**，只能靠「照抄已验证的官方插件写法 + CI 出包验证」 |

---

## 2. 路子 A：`tauri-plugin-geolocation`（官方插件）—— 不采用

### 2.1 离线缓存里有没有？**没有**

```console
$ ls ~/.cargo/registry/cache/*/ | grep -i geo
（无输出，exit=1）
$ ls ~/.cargo/registry/cache/*/ | grep -i location
objc2-core-location-0.3.2.crate
$ ls ~/.cargo/registry/cache/*/ | grep -i tauri
tauri-2.11.1.crate
tauri-build-2.6.1.crate
tauri-codegen-2.6.1.crate
tauri-plugin-2.6.1.crate
tauri-plugin-android-fs-28.4.0.crate
tauri-plugin-dialog-2.7.1.crate
tauri-plugin-fs-2.5.1.crate
tauri-plugin-notification-2.3.3.crate
tauri-plugin-opener-2.5.4.crate
tauri-plugin-process-2.3.1.crate
tauri-plugin-store-2.4.3.crate
tauri-plugin-updater-2.10.1.crate
tauri-runtime-2.11.1.crate
tauri-runtime-wry-2.11.1.crate
tauri-utils-2.9.1.crate
tauri-winres-0.3.6.crate
tauri-winrt-notification-0.7.3.crate
```

- 缓存共 **1290** 个 crate，**没有** `tauri-plugin-geolocation`。
- 唯一的 `objc2-core-location-0.3.2.crate` 是 **iOS/macOS 专用**（Objective-C 绑定），且已在 `Cargo.lock:4981`——它的依赖方是 `objc2-ui-kit`（`Cargo.lock:5144`），对 Android **一点用没有**。
- 结论：加这个插件 ⇒ `Cargo.toml` 多一行 `tauri-plugin-geolocation = "2"` ⇒ **本机 `cargo metadata --offline` 立刻失败**，本地验证链路整条断掉，只能盲推给 CI。

### 2.2 它的 Rust API 长什么样（我读了上游 `v2` 分支源码）

`plugins/geolocation/src/lib.rs` / `src/mobile.rs`（GitHub `tauri-apps/plugins-workspace@v2`）：

```rust
// lib.rs
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("geolocation")
        .invoke_handler(tauri::generate_handler![
            commands::get_current_position, commands::watch_position,
            commands::clear_watch, commands::check_permissions, commands::request_permissions
        ])
        .setup(|app, api| { #[cfg(mobile)] let g = mobile::init(app, api)?; app.manage(g); Ok(()) })
        .build()
}
// mobile.rs
impl<R: Runtime> Geolocation<R> {
    pub fn get_current_position(&self, options: Option<PositionOptions>) -> crate::Result<Position> {
        self.0.run_mobile_plugin("getCurrentPosition", options.unwrap_or_default()).map_err(Into::into)
    }
}
```

也就是说 **Rust 侧确实有 API**（`app.geolocation().get_current_position(...)`，约 10 行就能接完），而且 **Android 权限不用手改 manifest** —— 插件自己的 `android/src/main/AndroidManifest.xml` 就声明了：

```xml
<uses-permission android:name="android.permission.ACCESS_COARSE_LOCATION" />
<uses-permission android:name="android.permission.ACCESS_FINE_LOCATION" />
```

（AAR 的 manifest 会被 manifest merger 合进 APK —— 这是它相对 B 方案唯一的实质优势。）

### 2.3 但它有三个**致命点**

**① `timeout` 在 Android 上被忽略（源码原文）。**

`plugins/geolocation/src/models.rs`：

```rust
pub struct PositionOptions {
    pub enable_high_accuracy: bool,
    /// The maximum wait time in milliseconds for location updates.
    /// Default: 10000
    /// On Android the timeout gets ignored for getCurrentPosition.   // ← 原文
    pub timeout: u32,
    ...
}
```

这与本任务「**超时要短**，拿不到就退 IP」的硬要求正面冲突：`getCurrentPosition` 在 Android 上走 `Geolocation.kt` 的 `getCurrentLocation`/`requestLocationUpdates`，**等多久不由我们说了算**。
而且 `get_current_position()` 走的是**同步阻塞**的 `run_mobile_plugin`（`mobile.rs` 用的是 `PluginHandle::run_mobile_plugin`，不是 `run_mobile_plugin_async`），我们只能 `spawn_blocking` + `tokio::time::timeout` 包一层 —— 超时后那条阻塞线程**收不回来**，每次冷启动都会漏一个卡住的线程。

**② 它的 Android 实现依赖 Google Play Services —— 国内机型上必然失败。**

`plugins/geolocation/android/src/main/java/Geolocation.kt` 的第一句就是：

```kotlin
fun sendLocation(enableHighAccuracy: Boolean, successCallback: ..., errorCallback: ...) {
    val resultCode = GoogleApiAvailability.getInstance().isGooglePlayServicesAvailable(context);
    if (resultCode == ConnectionResult.SUCCESS) {
        ... LocationServices.getFusedLocationProviderClient(context).getCurrentLocation(prio, null) ...
    } else {
        errorCallback("Google Play Services not available.")     // ← 直接失败
    }
}
```

它用的是 `com.google.android.gms.location.FusedLocationProviderClient`，而本工程
`gen/android/app/build.gradle.kts` 的 `dependencies` 里**没有** `play-services-location`。
没有可用 GMS 的机器（国内大量机型、去 Google 化的 ROM）上，这条路**第一步就被拒**。

**③ 平台支持矩阵是「手机专用」。**

`Cargo.toml`：

```toml
[package.metadata.platforms.support]
windows = { level = "none" }
linux   = { level = "none" }
macos   = { level = "none" }
android = { level = "full" }
ios     = { level = "full" }
```

即桌面端**依然是空的**，与 B 方案的能力边界完全一样 —— 它并不能顺带解决桌面。

### 2.4 要改哪些文件 & 对底线的影响

| 文件 | 改动 | 影响 |
|---|---|---|
| `src-tauri/Cargo.toml` | +1 行依赖 | ⚠️ 本机无法离线解析 |
| `src-tauri/src/lib.rs` | +1 行 `.plugin(tauri_plugin_geolocation::init())` | 只新增 |
| `src-tauri/src/world_map/live.rs` | 加一段 GPS 分支 | 只新增 |
| `src-tauri/gen/android/.../AndroidManifest.xml` | **不用改**（AAR 自带） | — |
| `src-tauri/capabilities/*.json` | **不用改**（我们从 Rust 调，不走 JS/ACL） | — |
| `gen/schemas/*.json`（构建产物，未入 git） | 会多出 `geolocation:default` 等权限项 | 仅生成物 |

> **注意**：插件的 `@Command` 是给 **JS** 用的，走 ACL 检查；我们若从 **Rust** 调 `run_mobile_plugin`，**不经过 ACL**（证据见 §3.2），所以 capabilities 不用加东西。

---

## 3. 路子 B：自写 Kotlin 插件 + Tauri 移动插件桥 —— ✅ 推荐

### 3.1 机制（完整调用链，每一环都在已缓存源码里核实过）

```
Rust 命令 world_map_location
  └─ app.state::<GpsBridge>()            // PluginHandle<Wry>，app.manage 存起来
      └─ PluginHandle::run_mobile_plugin_async("getLocation", payload)
          └─ tauri-2.11.1/src/plugin/mobile.rs:428  run_command()  [cfg(target_os="android")]
              └─ JNI: activity.getPluginManager().runCommand(id, "syuki-location", "getLocation", data)
                  └─ PluginManager.runCommand → plugins["syuki-location"].invoke(invoke)
                      └─ 反射找到 LocationPlugin 里带 @Command 的方法
                          └─ Kotlin: LocationManager.getLastKnownLocation / requestLocationUpdates
                              └─ invoke.resolve(JSObject{lat,lng,accuracy,provider})  或  invoke.reject(msg, code)
          ← 原生回调 handlePluginResponse(id, success, error) → oneshot → Rust 拿到 Result<Value, ErrorResponse>
```

关键源码位置（全部来自 `~/.cargo/registry/src/*/tauri-2.11.1/`，**已解包**，可直接读）：

| 环节 | 位置 | 核实到的签名 |
|---|---|---|
| 注册 Android 插件 | `src/plugin/mobile.rs:200-280` | `pub fn register_android_plugin(&self, plugin_identifier: &str, class_name: &str) -> Result<PluginHandle<R>, PluginInvokeError>` |
| 插件 setup 钩子 | `src/plugin.rs:431-439` | `F: FnOnce(&AppHandle<R>, PluginApi<R, C>) -> Result<(), Box<dyn Error>>` |
| 句柄 | `src/plugin.rs:126-146` | `pub struct PluginHandle<R: Runtime> { name, handle: AppHandle<R> }` + `Clone` + `app()` |
| 异步调用 | `src/plugin/mobile.rs:286-314` | `pub async fn run_mobile_plugin_async<T: DeserializeOwned>(&self, command: impl AsRef<str>, payload: impl Serialize) -> Result<T, PluginInvokeError>` |
| 错误类型 | `src/plugin/mobile.rs:38-58` | `PluginInvokeError::InvokeRejected(ErrorResponse)`（`ErrorResponse{ code: Option<String>, message: Option<String>, data }`） |
| 模块可见性 | `src/plugin.rs:33-34` | `#[cfg(mobile)] pub mod mobile;` ⇒ **`run_mobile_plugin*` 只在手机目标存在**，Rust 侧必须 `#[cfg(target_os = "android")]` |

**类名怎么被找到的**（决定「自写 Kotlin 放哪」）：

```rust
// tauri-2.11.1/src/plugin/mobile.rs:253
let plugin_class = format!("{}/{}", plugin_identifier.replace('.', "/"), class_name);
// → runtime_handle.find_class(env, activity, plugin_class)
// → wry-0.55.1/src/android/mod.rs:502-517
pub fn find_class<'a>(env, activity, name: String) -> JniResult<JClass<'a>> {
    let class_name = env.new_string(name.replace('/', "."))?;
    activity.call_method(activity, "getAppClass", "(Ljava/lang/String;)Ljava/lang/Class;", [class_name])
}
// → wry-0.55.1/src/android/kotlin/WryActivity.kt:160
fun getAppClass(name: String): Class<*> { return Class.forName(name) }
```

`Class.forName` 用的是 **WryActivity 自己的 ClassLoader**（就是 APK 的 ClassLoader），所以**应用自己包里的类完全找得到** —— 不需要单独打成 AAR。

### 3.2 ⭐ 为什么这条路**不需要动 capabilities / ACL**（这点最容易被误解）

`tauri-2.11.1/src/plugin/mobile.rs:428-499` 的 Android 版 `run_command`：

```rust
fn run<R: Runtime>(id, plugin, command, payload, env, activity) -> Result<(), JniError> {
    let plugin_manager = env.call_method(activity, "getPluginManager", "()Lapp/tauri/plugin/PluginManager;", &[])?.l()?;
    env.call_method(plugin_manager, "runCommand",
        "(ILjava/lang/String;Ljava/lang/String;Ljava/lang/String;)V", ...)?;
    Ok(())
}
```

**它直接从 Rust 走 JNI 打到 Kotlin，中间没有经过 WebView、没有经过 IPC handler、没有经过 ACL/权限校验。** 权限系统（`capabilities/*.json`）只拦「前端 JS `invoke('plugin:xxx|yyy')`」那条路。
⇒ 我们新增的插件**不会**给前端多出任何可调用面，**`capabilities/` 一个字都不用改** —— 这对「只新增」底线是好事。

### 3.3 本工程已有**同款先例**（这是最强的可行性证据）

`src-tauri/Cargo.toml` 里已经有 `tauri-plugin-android-fs = "28"`，`src-tauri/src/lib.rs:235` 无条件 `.plugin(tauri_plugin_android_fs::init())`。它的实现（缓存源码 `tauri-plugin-android-fs-28.4.0/src/lib.rs:31-42`）：

```rust
pub fn init<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R, Option<config::Config>> {
    tauri::plugin::Builder::<R, Option<config::Config>>::new("android-fs")
        .setup(|app, api| {
            #[cfg(target_os = "android")] {
                let handle = api.register_android_plugin("com.plugin.android_fs", "AndroidFsPlugin")?;
                app.manage(AndroidFs { handle: handle.clone() });   // ← PluginHandle 存进 app state
                ...
            }
            Ok(())
        })
}
```

⇒ **`PluginHandle<R>` 可以 `app.manage()`**（即 `Send + Sync + 'static`）**已经在本工程的 release APK 里跑通了**；
`tauri-plugin-dialog` 在 Android 上同样走这条路（`tauri-plugin-dialog-2.7.1/src/mobile.rs:20-30`、`lib.rs:200-215`）。
B 方案用的每一个 API，本工程都已在用。

### 3.4 Kotlin 侧要用到的类 / 注解（逐个核对过定义）

| Kotlin 侧 | 位置 | 核实结果 |
|---|---|---|
| `app.tauri.plugin.Plugin` | `tauri-2.11.1/mobile/android/src/main/java/app/tauri/plugin/Plugin.kt` | `abstract class Plugin(private val activity: Activity)`；`fun requestPermissionForAliases(aliases, invoke, callbackName)` (**public**)；`fun getPermissionState(alias): PermissionState?` |
| `@Command` | `.../app/tauri/annotation/PluginMethod.kt` | `@Retention(RUNTIME) annotation class Command`（文件名是 `PluginMethod.kt`，注解名是 `Command`） |
| `@TauriPlugin` / `@Permission(strings, alias)` | `annotation/TauriPlugin.kt` / `annotation/Permission.kt` | 用于权限别名 |
| `@PermissionCallback` | `annotation/PermissionCallback.kt` | 权限回调方法（**可以 private**，`PluginHandle.indexMethods` 会 `method.isAccessible = true`） |
| `Invoke` | `plugin/Invoke.kt` | `resolve(data: JSObject?)` / `reject(msg, code)` / `parseArgs(cls)` / `getArgs()` |
| `JSObject` | `plugin/JSObject.kt` | `class JSObject : JSONObject` |
| `PermissionHelper` | `PermissionHelper.kt` | `hasPermissions(context, perms)` / **`hasDefinedPermissions`**（manifest 没声明就 reject） |
| `PermissionState` | `PermissionState.kt` | `GRANTED / DENIED / PROMPT / PROMPT_WITH_RATIONALE` |
| 权限弹窗发射器 | `PluginManager.kt` | `requestPermissions()` → `ActivityResultContracts.RequestMultiplePermissions()`；`onActivityCreate` 在 `TauriActivity.onCreate` 里注册 |

**弹窗时机也是安全的**：`PluginManager.onActivityCreate(this)` 在 `TauriActivity.onCreate`（`mobile/android-codegen/TauriActivity.kt`）里调用，而 `Rust.start()` 在 `WryLifecycleObserver.onStart`（`wry-0.55.1/src/android/kotlin/WryActivity.kt:24-27`）里 —— **onCreate 早于 onStart**，所以插件注册时权限 launcher 已经就绪。

### 3.5 ⚠️ 一个只会在 **release 包**里炸的坑：R8 混淆

`gen/android/app/build.gradle.kts` 的 release 开了 `isMinifyEnabled = true`。而插件类名是 `Class.forName("com.syuki.lingchat.location.LocationPlugin")` 这种**字符串反射**：

- **好消息**：`tauri-android` AAR 自带的 consumer proguard 规则（`tauri-2.11.1/mobile/android/proguard-rules.pro`）已经兜住了：
  ```
  -keep @app.tauri.annotation.TauriPlugin public class * {
    @app.tauri.annotation.Command public <methods>;
    @app.tauri.annotation.PermissionCallback <methods>;
    @app.tauri.annotation.ActivityCallback <methods>;
    @app.tauri.annotation.Permission <methods>;
    public <init>(...);
  }
  -keep @app.tauri.annotation.InvokeArg public class * { *; }
  ```
  ⇒ 只要插件类是 **public** + 标了 `@TauriPlugin` + ctor 是 public、`@Command` 方法是 public，**R8 不会动它**。
- **保险起见仍加一条**：`gen/android/app/proguard-rules.pro` 里补 `-keepattributes *Annotation*` + `-keep class com.syuki.lingchat.location.** { *; }`。
  理由：反射靠的是**注解本身**（`isAnnotationPresent(Command::class.java)`），而注解属性在 R8 下不保证默认保留；这条只**禁止裁剪**、不改变任何既有行为（最坏是 APK 大几 KB）。

### 3.6 要改哪些文件 & 对「只新增、不改现有功能」底线的影响

| # | 文件 | 改动性质 | 对底线的影响 |
|---|---|---|---|
| 1 | `src-tauri/src/world_map/loc_android.rs` | **纯新增**文件 | 无 |
| 2 | `src-tauri/src/world_map/mod.rs` | +1 行 `pub mod loc_android;` | 无 |
| 3 | `src-tauri/src/lib.rs` | +1 行 `.plugin(world_map::loc_android::init())` | 无（无 JS 命令、无 ACL） |
| 4 | `src-tauri/src/world_map/live.rs` | 在①③之间**插入**② GPS 段；常量+辅助函数 | **行为变化**：定位结果更准；原来两条路一条都没删 |
| 5 | `gen/android/.../location/LocationPlugin.kt` | **纯新增**文件 | 无 |
| 6 | `gen/android/.../AndroidManifest.xml` | +2 条 `uses-permission` +2 条 `uses-feature` | ⚠️ 见下 |
| 7 | `gen/android/app/proguard-rules.pro` | +2 条 keep 规则 | 仅「禁止裁剪」 |
| 8 | `Cargo.toml` / `capabilities/` / `tauri.conf.json` | **不动** | 无 |
| 9 | `src/**`（前端） | **一个字不动** | 无 |

**唯一需要斟酌的第 6 条**：`ACCESS_FINE_LOCATION` / `ACCESS_COARSE_LOCATION` 是 *dangerous* 权限，
- 只**声明**不会弹窗（弹窗由我们显式 `requestPermissionForAliases` 触发）⇒ 不会打扰没用过地图的用户；
- 但 Google Play 的「隐含特性」规则会让 `ACCESS_FINE_LOCATION` 隐含 `android.hardware.location.gps` **required=true**，可能把无 GPS 设备过滤掉 ⇒ 必须显式写 `uses-feature ... required="false"` 抵消（见 §7）；
- Play Console 的数据安全表单需要勾「位置信息」（若将来上架）。

### 3.7 已知的**行为特征**（不是 bug，但要写清楚）

1. **首次点「📍 按定位」大概率仍返回 IP 结果**：权限弹窗本身要占掉几秒（用户点「允许」的时间算在我们的 5.2 秒预算里）。用户第二次点就是真实 GPS 了。
2. **`getLastKnownLocation` 命中时是毫秒级返回**：Android 系统自己缓存最近一次任何 App 拿到的定位。只要最近 5 分钟内有 App 定位过（地图/外卖/相机都算），我们**立刻**拿到街道级坐标，根本不进冷启动。
3. **拒绝权限后被永久记住**：`PluginHandle.validatePermissions` 会把 DENIED 写进 `PluginManager` 的 `PluginPermStates` SharedPreferences；`getPermissionState()` 读得到 ⇒ 我们**不再重复弹窗**，直接走 IP 兜底。
4. **`DistrictLive.vue` 要的 `path` 仍然来自离线行政区缓存**（`live.rs:257` `reverse_path`），与 GPS/IP 无关 —— GPS 坐标一样走这条路反查，精度提升体现在坐标本身。

---

## 4. 路子 C：纯 Rust 直调 JNI（不写 Kotlin）—— ❌ 不成立

「加个 `jni` crate 直接调 `LocationManager`」听起来最省事（`jni-0.21.1` / `jni-0.22.4` / `ndk-0.9.0` **都在缓存里**，离线可解析），但三个环节各有一个硬阻断：

1. **拿不到 Activity。**
   `run_on_android_context` 定义在 `tauri-runtime-2.11.1/src/lib.rs:363`，`tauri` 只在 `src/plugin/mobile.rs` 内部通过 `crate::runtime::RuntimeHandle` 用它；
   而 `tauri/src/lib.rs:204` 只 `pub use runtime::ActivationPolicy;` —— **`pub mod runtime` 不存在**，`ManagerBase::runtime()` 也是 sealed trait（`src/lib.rs:1059-1062`）。
   ⇒ 普通 `#[tauri::command]` **没有任何公开途径**拿到 `JNIEnv` + `Activity`。

2. **`LocationListener` 是 Java 接口，Rust 实现不了。**
   `LocationManager.requestLocationUpdates(provider, minTime, minDistance, LocationListener)` 的最后一个参数是接口；
   要用它就必须有一个**真实的 Java/Kotlin 类**（`java.lang.reflect.Proxy` 也只能代理接口，仍需要一个 InvocationHandler 类）。API 30+ 的 `getCurrentLocation(Criteria, CancellationSignal, Executor, Consumer)` 同理（`Consumer` 也是接口）。

3. **运行时权限弹窗必须有 Activity 回调。**
   `ACCESS_FINE_LOCATION` 是 dangerous 权限，必须 `requestPermissions(...)` 弹窗；结果只能从 `onRequestPermissionsResult` / `ActivityResultLauncher` 拿到 —— 那又是 Kotlin 侧的东西。纯 Rust 只能轮询 `checkSelfPermission`，而**用户点「允许」要多久是不可控的**，轮询窗口要么太短要么白等。
   （`ActivityCompat.requestPermissions` 还依赖 androidx，直接 `activity.requestPermissions(String[], int)` 虽可用，但拿不到「用户拒绝」的即时信号。）

**结论**：Kotlin 是绕不开的。既然绕不开，就应该用 Tauri 官方支持的那条 Kotlin 通路（路子 B），而不是自己拼 JNI。

---

## 5. 路子 D：其它旁路 —— 逐条否掉

| 旁路 | 结论 | 证据 / 理由 |
|---|---|---|
| `termux-location` | ❌ **早就否过** | 它是 Termux:API 这个**独立 App** 提供的命令；APK 里 `Command::new("termux-location")` 只会 ENOENT，且未授权时会**挂起不返回**。`live.rs:140-145` 已写明。 |
| `dumpsys location` | ❌ | 需要 `android.permission.DUMP`（`signature\|privileged` 级），普通 App 拿不到；`Runtime.exec("dumpsys location")` 在应用进程里必然 `Permission denial`。这是 **shell/root** 才有的能力。 |
| 读 `/proc/*` | ❌ | Linux 内核里没有 GPS 坐标；Android 定位是 `LocationManager`（Java 服务 + HAL）之上的东西，/proc 不暴露。 |
| WebView `navigator.geolocation` | ❌（**且违反文件所有权**） | ① 胶水里根本没有钩子： |
| | | `grep -rn "setGeolocationEnabled\|onGeolocationPermissionsShowPrompt\|GeolocationPermissions" ~/.cargo/registry/src/*/tauri-2.11.1/` → **无输出（exit=1）**。缺 `WebSettings.setGeolocationEnabled(true)` 与 `WebChromeClient.onGeolocationPermissionsShowPrompt`，`getCurrentPosition` 只会走 error 回调。 |
| | | ② 就算在 `MainActivity.kt` 里补，也必须**覆盖 Tauri/Wry 已设置的 `WebChromeClient`**（文件选择、console、权限请求都挂在上面）⇒ **直接破坏现有功能**，违反底线。 |
| | | ③ 还要前端 `src/` 改成走 `navigator.geolocation` ⇒ **越界**（另一个 agent 在改 `src/`）。 |
| Android `Settings.Secure.LOCATION_MODE` | ❌ | 只能读到「定位开关开没开」，读不到坐标。 |
| 复用 `tauri-plugin-android-fs` 调任意 Java | ❌ | 它只暴露文件相关命令，没有反射/代理入口。 |
| Tauri 内置 `app.tauri` AppPlugin | ❌ | `mobile/android/src/main/java/app/tauri/AppPlugin.kt` 只有 `app_show/app_hide/...`，与定位无关。 |

---

## 6. ⭐ 自写 Kotlin 会不会被 `tauri android init` 覆盖？—— **不会**（源码级证据）

CI 的打包流程（`.github/workflows/build-android.yml`）在构建前**必然**跑一次：

```yaml
- name: 准备 Android 项目
  run: pnpm android:prepare      # = tauri android init && node scripts/generate-android-icons.mjs
```

所以这个问题是**决定性的**。我拉取了 `tauri-cli` 的源码（tag `tauri-v2.11.1`，并在 `dev` 分支复核过同一段），
`crates/tauri-cli/src/mobile/android/project.rs`：

```rust
fn generate_out_file(path: &Path, dest: &Path, package_path: &str, ...) -> std::io::Result<Option<fs::File>> {
  ...
  let mut options = fs::OpenOptions::new();
  options.write(true);
  ...
  if path.file_name().unwrap() == OsStr::new("BuildTask.kt") {
    options.truncate(true).create(true).open(path).map(Some)   // ← 只有 BuildTask.kt 会被覆盖
  } else if !path.exists() {
    options.create(true).open(path).map(Some)                  // ← 不存在才创建
  } else {
    Ok(None)                                                   // ← 已存在：什么都不做
  }
}
```

**结论（三条互相印证的证据）：**

1. **模板目录里根本没有我们的文件**：`crates/tauri-cli/templates/mobile/android/app/src/main` 只有 `AndroidManifest.xml / MainActivity.kt / res`。
   `render_with_generator` 只遍历**模板目录**，新加的 `app/src/main/java/com/syuki/lingchat/location/LocationPlugin.kt` **不在遍历范围内 ⇒ 永远不被访问**。
2. **已存在的文件一律跳过**（`else { Ok(None) }`）⇒ `AndroidManifest.xml` 和 `MainActivity.kt` **不会被还原**。
   反证：现在 manifest 里手工加的 `RECORD_AUDIO` 注释（`AndroidManifest.xml:4-10`）和 `MainActivity.kt` 里那一大段安全区注入代码，**都是 CI 反复跑过 `android:prepare` 之后仍然在的**。
3. **git 层面也是安全的**：`git ls-files src-tauri/gen/android` 里 `app/src/main/java/com/syuki/lingchat/MainActivity.kt` **是被追踪的**，
   而 `gen/android/app/.gitignore` 只忽略 `/src/main/**/generated`、`/src/main/jniLibs/**/*.so`、`/src/main/assets/tauri.conf.json`、`/tauri.build.gradle.kts`、`/proguard-tauri.pro`、`/tauri.properties` ——
   **`src/main/java/**` 完全没被忽略** ⇒ 新 Kotlin 文件会被正常提交、进 CI。

---

## 7. 「Android 权限在 Tauri 2 里到底怎么声明才生效」

按 **声明 → 编译 → 运行时** 三层，缺一层就静默失败：

| 层 | 做什么 | 缺了会怎样 | 本方案的落点 |
|---|---|---|---|
| **① Manifest 声明** | `<uses-permission android:name="android.permission.ACCESS_FINE_LOCATION" />`（和 COARSE） | `PermissionHelper.hasDefinedPermissions()` 查 `PackageManager.getPackageInfo(GET_PERMISSIONS)` 查不到 ⇒ `PluginHandle.validatePermissions` **直接 `invoke.reject("Missing the following permissions in AndroidManifest.xml: ...")`**，弹窗都不会出现 | `gen/android/app/src/main/AndroidManifest.xml` |
| **② 隐含特性** | `<uses-feature android:name="android.hardware.location.gps" android:required="false" />`（+ `.network`） | Google Play 会因 `ACCESS_FINE_LOCATION` **隐含** `android.hardware.location.gps required=true`，把无 GPS 设备过滤掉 | 同上 |
| **③ 运行时申请** | 在 Kotlin 里显式 `requestPermissionForAliases(arrayOf("location"), invoke, "回调名")` → 走 `PluginManager.requestPermissions` → `ActivityResultContracts.RequestMultiplePermissions` | 只有声明没有申请 ⇒ `checkSelfPermission` 永远 DENIED ⇒ **永远拿不到坐标**（而且不报错，最坏情况是静默退化成 IP） | `LocationPlugin.kt` |
| **④ 别忘 R8** | release 开 `isMinifyEnabled=true`，反射加载的类要被 keep | 只在 **release 包**里插件「不存在」→ 静默退化 | `proguard-rules.pro` |

另外两条**容易搞错**的点：

- **capabilities/ACL 跟这件事无关**。插件权限（`"geolocation:allow-get-current-position"` 那种字符串）是给**前端 JS invoke** 用的；
  我们从 Rust 走 `run_mobile_plugin*`，**不经过 ACL**（§3.2 有源码证据）。本方案**不需要**在 `capabilities/*.json` 里加任何东西。
- **`android:required` 之类的 manifest 属性必须写对**（`uses-feature` 少一个 `required="false"` 就可能丢设备）。

---

## 8. 推荐实施方案（按 5.2 秒真实预算设计）

### 8.1 新的优先级

```
① 手动坐标（lat+lng 合法）       —— 不碰网络/权限，永远最先判
② 真实 GPS（仅 Android）          —— Kotlin LocationPlugin
③ IP 兜底（ip-api.com）           —— 桌面/无插件/权限被拒/超时 时的保底
✗ 三条全败 → {error, hint, source:"none"}，绝不返回 Err
```

任何一环失败**只往下退，不报错**。权限被拒时额外把「怎么开权限」写进 `hint`（`hint` 是 `WorldLocation` 已声明的字段）。

### 8.2 时间预算（`fast: true`，前端硬限 6000 ms）

| 阶段 | 预算 | 说明 |
|---|---|---|
| 总墙钟 | 5200 ms | 沿用 `live.rs:86`，留 0.8 s 给 IPC |
| ② GPS | ≤ 3000 ms | 缓存命中则 ~0 ms；冷启动最多等 3 s |
| ③ IP | `min(4000, 剩余-1200)` | 剩得少就自动收窄；GPS 未介入时仍是原来的 4 s |
| 反查行政区 | 剩余（上限 3000 ms） | 固定留 1200 ms 给 `reverse_path`，否则 `DistrictLive.vue` 会因为没 `path` 报错 |

非 `fast`（`LOCATION_BUDGET = 11 s`）：GPS 7 s → IP ≤8 s → 反查剩余。

### 8.3 返回字段（GPS 命中）

```json
{ "lat": 31.2304, "lng": 121.4737,
  "source": "gps", "precision": "street" | "district" | "city",
  "provider": "gps" | "network" | "fused" | "passive",
  "accuracy": 12.5,
  "path": [...], "area": "上海市·黄浦区", "leaf": {...} }
```

`precision` 映射（按 Android 给的 `Location.accuracy`，单位米）：
`≤120 → "street"`、`≤1200 → "district"`、其它 / 未知 → `"city"`。

### 8.4 实施清单

见 §3.6 表格（9 条，其中 3 条是纯新增文件、1 条是纯新增 Kotlin、其余是 1~3 行的接线）。

---

## 9. 不确定的地方（如实写）

| # | 不确定项 | 我的判断依据 | 风险等级 |
|---|---|---|---|
| 1 | **Kotlin 侧本地编译不了**（无 Android SDK/kotlinc） | 所有 API 都对着缓存里的官方源码逐个核对了签名；写法照抄 `tauri-plugin-android-fs` / `tauri-plugin-geolocation` 的同名调用 | 中（只能 CI 出包验证） |
| 2 | **R8 是否真的保留注解** | AAR consumer 规则里有 `-keep @app.tauri.annotation.TauriPlugin public class *`；本工程 release 包里 `tauri-plugin-dialog` 走同一条反射通路且可用。仍额外加了 `-keepattributes *Annotation*` 兜底 | 低 |
| 3 | **首次弹权限框会把 5.2 秒预算吃光** | 用户点「允许」的耗时不可控；这是**已知行为特征**，不是缺陷（第二次点击即走 GPS） | 中（体验层面） |
| 4 | **`ACCESS_FINE_LOCATION` 对 Google Play 上架的影响** | 需要数据安全表单勾「位置」；`uses-feature required=false` 抵消隐含特性。本工程目前是自签 Release、不走上架 | 低 |
| 5 | **CI 里 `tauri android init` 的版本** | `package.json` 是 `@tauri-apps/cli: ^2`，会拿最新 2.x；我核对的是 `tauri-v2.11.1` tag **与 `dev` 分支**的同一段代码，两处一致 | 低 |
| 6 | **桌面端仍然只有 IP** | `tauri-plugin-geolocation` 的 support 矩阵显示桌面 `none`；自写方案也没解决（Windows/macOS/Linux 各要一套）。**本次范围内不处理** | 已知限制 |

## 10. 实施记录（2026-09-12，按路子 B 落地）

### 10.1 改了哪些文件、各多少行

| 文件 | 性质 | 行数 | 说明 |
|---|---|---|---|
| `src-tauri/src/world_map/loc_android.rs` | **新增** | 281 | 插件注册 + `run_mobile_plugin_async` 调用 + `GpsOutcome` 错误语义 + 精度映射 |
| `src-tauri/gen/android/app/src/main/java/com/syuki/lingchat/location/LocationPlugin.kt` | **新增** | 421 | Kotlin 侧：权限申请、系统缓存优先、gps+network 并发取一次新鲜定位、硬超时 |
| `src-tauri/src/world_map/live.rs` | 改 | +158 / −31 | 插入 ② GPS 段；IP 超时改为「按剩余预算收口」；新增 4 个常量 + 2 个辅助函数 + 文档 |
| `src-tauri/gen/android/app/src/main/AndroidManifest.xml` | 改 | +32 | 2 条定位权限 + 3 条 `uses-feature required="false"` |
| `src-tauri/gen/android/app/proguard-rules.pro` | 改 | +19 | R8 兜底 keep 规则（反射加载 + 注解） |
| `src-tauri/src/lib.rs` | 改 | +7 / −1 | `.plugin(world_map::loc_android::init())` |
| `src-tauri/src/world_map/mod.rs` | 改 | +6 | `pub mod loc_android;` + 说明 |
| `docs/world-map/17-Android定位方案调研.md` | **新增** | 本文件 | — |

**没有动的**：`Cargo.toml`（**零新依赖**）、`Cargo.lock`、`capabilities/*.json`、`tauri.conf.json`、
`src/**`（前端一个字没改）、`~/lingchat-official`、`world_map_rs`。

> 说明：验证时跑了一次 `cargo metadata --offline`，它顺手把 `Cargo.lock` 里一行**陈旧的**
> `libc`（`ling_chat` 的依赖列表，Cargo.toml 里其实早就不直接依赖 libc 了）删掉了。
> 那不是我这次要改的东西，**已经还原**，保持 diff 最小。

### 10.2 `world_map_location` 的新优先级

```
① 手动坐标（lat+lng 都合法）       —— 不碰网络/权限，最先判（行为不变）
② 真实 GPS（仅 Android）           —— loc_android::locate()，3 秒预算
③ IP 兜底（ip-api.com）            —— 超时按剩余预算自动收窄
✗ 三条全败 → {error, hint, source:"none"}，始终 Ok(对象)，绝不 Err
```

**每一环失败只往下退**：桌面端 / 插件没注册 → `Unavailable`；权限被拒 → `Denied`；
超时 → `Timeout`；其它 → `Failed`。四种情况**都继续走 ③**。
只有当 ③ 也拿不到时，才把 ② 的失败原因拼进 `hint` 给前端看。

**GPS 命中时的返回字段**（`source` = `"gps"`）：

| 字段 | 值 |
|---|---|
| `lat` / `lng` | 真实坐标 |
| `source` | `"gps"` |
| `precision` | `accuracy ≤120m → "street"`；`≤1200m → "district"`；其它/未知 → `"city"` |
| `provider` | Android provider 名：`gps` / `network` / `fused` / `passive` |
| `accuracy` | 精度（米），Android 没给就不带这个字段 |
| `path` / `area` / `leaf` | 与原来一样，由离线行政区缓存反查（`remaining(deadline)` 内完成） |

**GPS 失败但 IP 成功**时：仍返回 IP 结果，另外**多带一个 `hint`**（`hint` 是 `WorldLocation`
已声明的可选字段）说明为什么只有城市级；前端只在 `error` 存在时读 `hint`，
所以**界面行为零变化**。

**权限被拒**：Kotlin 侧 `invoke.reject(msg, "PERMISSION_DENIED")` → Rust 侧
`GpsOutcome::Denied` → 走 IP 兜底；IP 也不成时 `hint` 里会写清「系统定位权限被拒绝（…）」，
前端 `WorldMap.vue::relocate()` 判 `loc.error` 后就把这句话显示出来并回到默认城市，
用户可继续用手动选区域 —— 正是任务要求的那条分支。

### 10.3 时间预算的实际情况（`fast: true`）

| 场景 | GPS 阶段 | IP 阶段 | 反查 | 总用时 |
|---|---|---|---|---|
| 桌面端 / 权限已拒（`Unavailable`/`Denied` 立即返回） | ≈0 | ≤4000ms | ≤3000ms | 与改动前一致 |
| **系统缓存命中** | ~毫秒 | 跳过 | 剩余全给（≤3000ms） | **最快** |
| GPS 冷启动拿到 | 1~3s | 跳过 | 剩余 | ≤5.2s |
| GPS 空手而归 | 3000ms | ≤1000ms | ≥1200ms | ≤5.2s |

关键点：**GPS 没介入时，IP 拿到的仍然是原来的 4 秒**（`left(deadline)=5200`，
减掉 `GEOCODE_RESERVE=1200` 还是 4000ms）—— 也就是「没装/没权限的机器行为完全不变」。

### 10.4 Kotlin 侧的关键设计（为什么这么写）

| 设计 | 为什么 |
|---|---|
| 先 `getLastKnownLocation`（`lm.allProviders`，5 分钟内） | 命中就是**毫秒级**。只要最近 5 分钟内有任何 App 定位过，立刻拿到真实坐标，完全不触发冷启动 |
| 年龄用 `SystemClock.elapsedRealtimeNanos()` 算 | 单调时钟，不受用户改系统时间影响（官方插件 `getLastLocation()` 同款） |
| 缓存没命中才 `requestLocationUpdates`，同时挂 gps + network | network 常常 200ms 就回一个精度一两公里的结果，GPS 900ms 能给到 10 米 —— 两个一起要，谁先到谁用 |
| 拿到「不够好」的定位后再等 `FIRST_FIX_GRACE_MS=1200ms` | 给 GPS 一点收敛时间。不等这一下，`precision` 会从 `street` 掉到 `city`，这次定位就白做了 |
| 拿到 GPS 定位或精度 ≤100m → **立刻**收工 | 够好就别再等 |
| `handler.postDelayed` 硬超时，到点有次好的就用次好的 | 保证在 Rust 给的预算内一定 `resolve/reject`，绝不挂住 invoke |
| 四条收工路径都走同一个 `cleanup()`（摘监听 + 清定时器） | 不让 GPS 引擎一直被我们开着（耗电 / 后台定位问题） |
| 用 AOSP `LocationManager`，**不用** Google Play Services | 目标机型大量没有可用 GMS（官方插件正是死在这一条） |
| `getPermissionState() == DENIED` 时不再弹窗 | 系统记住「不再询问」后再申请也是静默拒绝，不如直接走 IP，少骚扰用户一次 |

### 10.5 本次做过的验证（以及没做到的）

| 验证 | 手段 | 结果 |
|---|---|---|
| Rust 语法 | `rustc --edition 2021 --crate-type lib --emit=metadata` | `loc_android.rs` / `live.rs` **只剩 unresolved import 类错误**（E0432/E0433 + 缺 serde 属性），无语法/其它错误 |
| 借用与移动逻辑 | 用与 tauri **同形的桩类型**写了一个可运行的复刻（`map_result` / `precision_of` / match+return） | `rustc` 退出码 0，运行输出 `borrow/logic check OK`，断言 `precision_of` 四档全对（含 `NaN → "city"`） |
| 依赖是否可解析 | `cargo metadata --offline` | 退出码 **0**，1008 个包，`tauri 2.11.1`；`Cargo.toml`/`Cargo.lock` 均无改动 |
| 用到的每个 tauri API 是否真实存在 | 逐个读缓存里的源码核签名 | 见 §3.1 / §3.4 的表格，全部命中 |
| `tauri android init` 会不会覆盖自写 Kotlin | 读 tauri-cli 源码（tag + dev 分支） | `generate_out_file()` 里 `else { Ok(None) }` —— **不会** |
| **Kotlin 编译** | ❌ **做不到** | 本机没有 Android SDK / kotlinc。Kotlin 侧的正确性 = 「照抄官方插件同款调用 + 每个 API 对着 `tauri-2.11.1/mobile/android` 源码核对」 |
| **真机运行** | ❌ **做不到** | 本机打不了 APK，只能等 CI 出包（`.github/workflows/build-android.yml`，手动触发） |

### 10.6 一个自我更正（如实写）

写 `loc_android.rs` 的错误映射时，我一开始用的是 `match code.as_str() { ... }`，
后来担心 `code` 被 match 的 scrutinee 借用贯穿整个 match、导致 arm 里 move 不出来
（E0505），于是改成了 `if / else if`。**为了确认这个判断，我用桩类型把两个版本都编了一遍：
两个都能过。** 也就是说那次「修复」不是必需的 —— NLL 在 `_` 分支里已经不认为
`&str` 还被用着了。`if/else` 版本**保留**（等价、且不依赖这层微妙的 NLL 推理），
但结论要如实说：**这不是一个真实的 bug**。

---

*调研 + 实施：2026-09-12 · 改动的文件见 §10.1*

---

## 附：本次调研用到的命令（可复现）

```bash
# 1) 缓存里有没有定位相关 crate
ls ~/.cargo/registry/cache/*/ | grep -i geo          # 无输出
ls ~/.cargo/registry/cache/*/ | grep -i location     # objc2-core-location-0.3.2.crate（iOS 专用）

# 2) tauri Android 胶水有没有 WebView 定位钩子
grep -rn "setGeolocationEnabled\|onGeolocationPermissionsShowPrompt\|GeolocationPermissions" \
  ~/.cargo/registry/src/*/tauri-2.11.1/              # 无输出

# 3) 移动插件桥的 API
grep -rn "run_mobile_plugin\|register_android_plugin" ~/.cargo/registry/src/*/tauri-2.11.1/src/
# src/plugin/mobile.rs:201  pub fn register_android_plugin(
# src/plugin/mobile.rs:286  pub async fn run_mobile_plugin_async<T: DeserializeOwned>(
# src/plugin/mobile.rs:317  pub fn run_mobile_plugin<T: DeserializeOwned>(

# 4) 类名怎么被解析（决定 Kotlin 放哪）
sed -n '502,517p' ~/.cargo/registry/src/*/wry-0.55.1/src/android/mod.rs   # find_class → getAppClass
sed -n '158,162p' ~/.cargo/registry/src/*/wry-0.55.1/src/android/kotlin/WryActivity.kt  # Class.forName

# 5) tauri android init 会不会覆盖
python3 - <<'PY'   # 走 api.github.com 取 tauri-cli 源码
# crates/tauri-cli/src/mobile/android/project.rs :: generate_out_file()
PY

# 6) gen/android 是否被 git 追踪
git ls-files src-tauri/gen/android | grep java
# src-tauri/gen/android/app/src/main/java/com/syuki/lingchat/MainActivity.kt
```
