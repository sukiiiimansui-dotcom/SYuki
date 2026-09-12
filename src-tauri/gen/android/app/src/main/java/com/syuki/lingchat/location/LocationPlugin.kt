package com.syuki.lingchat.location

import android.Manifest
import android.app.Activity
import android.content.Context
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import app.tauri.PermissionHelper
import app.tauri.PermissionState
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.util.concurrent.atomic.AtomicBoolean

/**
 * `getLocation` 的入参（字段名与 Rust 侧 `world_map::loc_android::LocationRequest` 一一对应）。
 *
 * 必须是**顶层类**并且带 `@InvokeArg`：tauri-android 的 consumer proguard 规则里有
 * `-keep @app.tauri.annotation.InvokeArg public class * { *; }`，Jackson 靠反射填字段。
 * 字段有默认值，所以 JSON 里缺字段也能构造出可用对象。
 */
@InvokeArg
class LocationArgs {
  /** 冷启动最多等多久（毫秒）—— 由 Rust 侧按「前端 6 秒预算」算好传进来 */
  var timeoutMs: Long = 3000

  /**
   * 系统缓存定位的「最长可接受年龄」（毫秒），小于 0 表示不限。
   *
   * 5 分钟内的缓存直接采信：对「省 → 市 → 区县」这个粒度的地图来说，5 分钟内的
   * 位置就是当前位置，省下这几秒比死等一次冷启动 GPS 划算得多。
   * （tauri-plugin-geolocation 的 `getLastLocation(maximumAge)` 是同一个思路。）
   */
  var maxAgeMs: Long = 300000
}

/**
 * 世界模拟「真实 GPS 定位」的 Android 端实现（Tauri 移动插件）。
 *
 * ## 这是什么、为什么必须用 Kotlin 写
 *
 * 工程里**没有** `tauri-plugin-geolocation`，本机 crates.io 又 403（完整论证见
 * `docs/world-map/17-Android定位方案调研.md`），所以走 Tauri 官方的
 * **应用内 Android 插件** 通路：Rust 侧 `world_map/loc_android.rs` 用
 * `PluginApi::register_android_plugin` 注册本类，再用 `PluginHandle::run_mobile_plugin_async`
 * 调下面的 `getLocation`。
 *
 * 调用链（每一环都在 tauri 2.11.1 / wry 0.55.1 源码里核实过）：
 * ```
 * Rust  run_mobile_plugin_async("getLocation", {...})
 *  → JNI activity.getPluginManager().runCommand(id, "syuki-location", "getLocation", json)
 *  → PluginManager.runCommand → PluginHandle.invoke → 反射找带 @Command 的方法 → 这里
 *  → invoke.resolve(JSObject) 或 invoke.reject(msg, code) → 原生回调回 Rust
 * ```
 * **全程不经过 WebView / JS / capabilities（ACL）**，所以本类不给前端增加任何可调用面。
 *
 * ## 为什么不复用官方插件的做法（Google Play Services）
 *
 * 官方 `Geolocation.kt` 走的是 `FusedLocationProviderClient`，并且第一句就是
 * `GoogleApiAvailability.getInstance().isGooglePlayServicesAvailable(context)`，
 * 不等于 `SUCCESS` 就直接回 `"Google Play Services not available."`。
 * 国内大量机型没有可用 GMS ⇒ 那条路在这些机器上**必然失败**。
 * 本实现只用 AOSP 的 `LocationManager`（gps / network / fused(若有) / passive），
 * 有 GMS 没 GMS 都能拿到定位。
 *
 * ## 为什么不能用 WebView 的 navigator.geolocation
 *
 * tauri 2.11.1 的 Android 胶水里**没有** `WebSettings.setGeolocationEnabled(true)`，
 * 也没有 `WebChromeClient.onGeolocationPermissionsShowPrompt`（全 crate grep 无命中），
 * `getCurrentPosition` 只会走 error 回调。想在 MainActivity 里补，就必须覆盖
 * Wry 已经设置好的 WebChromeClient（文件选择 / console 都挂在上面）→ 破坏现有功能。
 *
 * ## 权限：三层缺一不可
 *
 * ① `AndroidManifest.xml` 里**必须**声明 ACCESS_FINE/COARSE_LOCATION
 *    —— `PluginHandle.validatePermissions()` 会查 manifest，查不到直接 reject，
 *    连系统弹窗都不会出现（`PermissionHelper.hasDefinedPermissions`）。
 * ② 运行时必须**显式申请**（下面的 `requestPermissionForAliases`），光声明不会弹窗。
 * ③ release 包开了 R8（`isMinifyEnabled = true`），反射加载靠 tauri-android AAR 自带的
 *    `-keep @app.tauri.annotation.TauriPlugin public class *`；本类保持 public + 标注解 +
 *    public ctor 即可，另在 `app/proguard-rules.pro` 加了兜底规则。
 *
 * ## 线程模型（很重要）
 *
 * wry 的 `dispatch()` 把 JNI 闭包投进 `MainPipe`（`wry-0.55.1/src/android/mod.rs:522`），
 * **在 Android 主线程执行**。所以 `@Command` 方法、`Handler(Looper.getMainLooper())` 的回调、
 * 以及权限弹窗用的 `ActivityResultLauncher.launch()` 全都在主线程 —— 天然没有竞态，
 * 下面的 `best` / `graceScheduled` 也就可以直接用普通变量。
 *
 * ## ⚠️ `@TauriPlugin` 注解一个字都不能少
 *
 * 它同时管三件事，缺一条就**静默失效**（不报错、只是永远退回 IP）：
 *   ① `PluginHandle.annotation = instance.javaClass.getAnnotation(TauriPlugin::class.java)`
 *      —— 注解缺失时 `annotation == null`，于是 `getPermissionState()` 返回空 map、
 *      `getPermissionStringsForAliases()` 返回空数组 ⇒ `requestPermissionForAliases`
 *      **什么都不做就返回**，invoke 永远不 resolve（只能等 Rust 侧超时）。
 *   ② `permissions = [Permission(strings=[FINE, COARSE], alias="location")]` 定义了
 *      「location」这个别名对应的真实权限字符串，权限弹窗申请的就是它们。
 *   ③ R8：tauri-android AAR 的 keep 规则是按 `@app.tauri.annotation.TauriPlugin` 匹配的，
 *      没这个注解，release 包里本类会被混淆/裁掉，`Class.forName` 直接失败。
 */
@TauriPlugin(
  permissions = [
    Permission(
      strings = [
        Manifest.permission.ACCESS_FINE_LOCATION,
        Manifest.permission.ACCESS_COARSE_LOCATION
      ],
      alias = "location"
    )
  ]
)
class LocationPlugin(private val ctx: Activity) : Plugin(ctx) {

  private companion object {
    /** 权限别名，必须与 @TauriPlugin 注解里的 alias 一致 */
    const val ALIAS_LOCATION = "location"

    /** 错误码：Rust 侧靠它区分「权限被拒」和「超时」，好给出不同的降级文案 */
    const val CODE_DENIED = "PERMISSION_DENIED"
    const val CODE_TIMEOUT = "TIMEOUT"
    const val CODE_NO_SERVICE = "NO_SERVICE"
    const val CODE_LOCATION_DISABLED = "LOCATION_DISABLED"
    const val CODE_BAD_FIX = "BAD_FIX"

    /**
     * 拿到「不够好」的第一个定位后，再多等这么久看有没有更好的（通常是等 GPS 收敛）。
     *
     * 为什么需要：同时向 gps 和 network 要定位时，network 往往 200ms 就给一个
     * 精度一两公里的结果，而 GPS 900ms 能给到 10 米。不等这一下就把 network 那个
     * 交上去，`precision` 会从 street 掉到 city —— 那这次「真实定位」就白做了。
     * 反过来 GPS 先到就立刻收工，不多等。
     */
    const val FIRST_FIX_GRACE_MS = 1200L

    /** 「够好、不用再等」的精度阈值（米）。GPS 与高精度融合定位通常都在这个量级 */
    const val GOOD_ENOUGH_ACCURACY_M = 100f

    /** 参数缺失/超时值不合法时的兜底 */
    const val DEFAULT_TIMEOUT_MS = 3000L
  }

  // ═══════════════════════════════════════════════════════════════════
  // 命令入口
  // ═══════════════════════════════════════════════════════════════════

  /**
   * `getLocation` —— Rust 侧唯一会调的命令。
   *
   * **每一条分支都必须以 `invoke.resolve(...)` 或 `invoke.reject(...)` 收尾**，
   * 否则 Rust 侧的 `run_mobile_plugin_async` 会一直挂着（外层虽有 tokio 超时兜底，
   * 但那会白白吃掉本该留给 IP 兜底的预算）。
   */
  @Command
  fun getLocation(invoke: Invoke) {
    // ① 已有「大致位置」权限就能干活（Android 12+ 用户可能只给了「大致」）
    if (hasCoarseLocation()) {
      fetch(invoke)
      return
    }

    // ② 之前明确拒绝过（系统记住了「不再询问」）→ 不再弹窗骚扰，直接让 Rust 走 IP 兜底。
    //    这个状态由 PluginHandle.validatePermissions 写进 PluginManager 的
    //    "PluginPermStates" SharedPreferences，getPermissionState 读得到。
    if (getPermissionState(ALIAS_LOCATION) == PermissionState.DENIED) {
      invoke.reject("定位权限已被拒绝（可在系统设置 → 应用 → 权限里重新开启）", CODE_DENIED)
      return
    }

    // ③ 首次（或只拒绝过一次）→ 弹系统权限框。
    //    这里同时申请 FINE + COARSE（别名 location 绑定的就是这两个字符串），
    //    Android 12+ 会正常给出「精确 / 大致」二选一。
    requestPermissionForAliases(arrayOf(ALIAS_LOCATION), invoke, "locationPermissionCallback")
  }

  /**
   * 权限弹窗返回。
   *
   * ⚠️ 这个回调**只收到 invoke**、收不到原始参数 —— 但 `Invoke` 内部保留了 argsJson，
   * 所以 [fetch] 里重新 `parseArgs` 依然拿得到 timeoutMs / maxAgeMs。
   *
   * 另外注意 `PluginHandle.requestPermissions` 的行为：只有权限**没在 manifest 里声明**时
   * 它才 reject 并且**不**调用本回调；用户只是「拒绝」的话本回调会照常执行，
   * 所以「拒绝」这个分支必须由我们自己处理。
   */
  @PermissionCallback
  private fun locationPermissionCallback(invoke: Invoke) {
    if (hasCoarseLocation()) {
      fetch(invoke)
    } else {
      invoke.reject("用户拒绝了定位权限", CODE_DENIED)
    }
  }

  // ═══════════════════════════════════════════════════════════════════
  // 取定位
  // ═══════════════════════════════════════════════════════════════════

  private fun fetch(invoke: Invoke) {
    // 参数解析失败不该让整条命令失败：退回默认值继续（默认值本身就可直接用）
    val args = try {
      invoke.parseArgs(LocationArgs::class.java)
    } catch (e: Exception) {
      LocationArgs()
    }

    val lm = ctx.getSystemService(Context.LOCATION_SERVICE) as? LocationManager
    if (lm == null) {
      invoke.reject("这台设备没有定位服务", CODE_NO_SERVICE)
      return
    }

    // ① 先吃系统缓存 —— 命中就是毫秒级返回，完全不用等冷启动。
    //    这也是本方案在真实使用里「大多数时候都能立刻拿到真实坐标」的原因：
    //    只要最近 5 分钟内有任何 App（地图/外卖/相机）定位过，缓存里就有。
    val cached = bestLastKnown(lm, args.maxAgeMs)
    if (cached != null) {
      resolveLocation(invoke, cached, true)
      return
    }

    // ② 缓存没有 → 主动要一次新鲜定位，硬超时
    requestFresh(invoke, lm, args.timeoutMs)
  }

  /**
   * 在所有 provider 里挑一个「够新鲜 + 精度最好」的缓存定位；都没有就 null。
   *
   * 用 `lm.allProviders` 而不是写死 gps/network：厂商 ROM 常有额外 provider，
   * 而且对不存在的 provider 调 `getLastKnownLocation` 会抛 IllegalArgumentException。
   * 年龄用 `SystemClock.elapsedRealtimeNanos()` 算（单调时钟，不受用户改系统时间影响），
   * 与官方插件 `Geolocation.getLastLocation()` 的做法一致。
   */
  private fun bestLastKnown(lm: LocationManager, maxAgeMs: Long): Location? {
    val nowNanos = SystemClock.elapsedRealtimeNanos()
    var best: Location? = null
    val providers = try {
      lm.allProviders
    } catch (e: Exception) {
      emptyList<String>()
    }
    for (provider in providers) {
      // 权限被用户在设置里撤销会抛 SecurityException —— 一律当「这个 provider 没有」
      val loc = try {
        lm.getLastKnownLocation(provider)
      } catch (e: Exception) {
        null
      } ?: continue

      if (maxAgeMs >= 0) {
        val ageMs = (nowNanos - loc.elapsedRealtimeNanos) / 1_000_000L
        if (ageMs > maxAgeMs || ageMs < -DEFAULT_TIMEOUT_MS) continue // 负数过大＝时钟异常，不可信
      }
      best = pickBetter(best, loc)
    }
    return best
  }

  /** a / b 谁更准用谁；精度未知（hasAccuracy()==false 或 accuracy<=0）的排在已知精度之后 */
  private fun pickBetter(a: Location?, b: Location): Location {
    if (a == null) return b
    val aa = if (a.hasAccuracy() && a.accuracy > 0f) a.accuracy else Float.MAX_VALUE
    val ba = if (b.hasAccuracy() && b.accuracy > 0f) b.accuracy else Float.MAX_VALUE
    return if (ba < aa) b else a
  }

  /**
   * 主动请求一次定位：同时挂 gps + network（+ fused），**先到先用、够好就收**。
   *
   * 收工条件（按顺序）：
   *   · 来的是 GPS 定位，或精度 ≤ [GOOD_ENOUGH_ACCURACY_M] → 立刻返回
   *   · 只是一个「不够好」的定位（比如 network 的一两公里）→ 再等 [FIRST_FIX_GRACE_MS]
   *     看有没有更好的，到点就用手里最好的那个返回
   *   · 到 timeoutMs 还没定位 → 有次好的就用次好的，什么都没有才 reject(TIMEOUT)
   */
  private fun requestFresh(invoke: Invoke, lm: LocationManager, timeoutMs: Long) {
    val providers = ArrayList<String>(3)
    if (isProviderEnabled(lm, LocationManager.GPS_PROVIDER)) providers.add(LocationManager.GPS_PROVIDER)
    if (isProviderEnabled(lm, LocationManager.NETWORK_PROVIDER)) providers.add(LocationManager.NETWORK_PROVIDER)
    if (isProviderEnabled(lm, "fused")) providers.add("fused")
    // 只剩 passive 也凑合：它只会转发别的 App 已经要到的定位，不会自己开 GPS
    if (providers.isEmpty() && isProviderEnabled(lm, LocationManager.PASSIVE_PROVIDER)) {
      providers.add(LocationManager.PASSIVE_PROVIDER)
    }
    if (providers.isEmpty()) {
      invoke.reject("系统定位服务未开启（请在系统设置里打开「位置信息」）", CODE_LOCATION_DISABLED)
      return
    }

    val budgetMs = if (timeoutMs > 0) timeoutMs else DEFAULT_TIMEOUT_MS
    val handler = Handler(Looper.getMainLooper())
    val done = AtomicBoolean(false)
    var best: Location? = null
    var graceScheduled = false
    // 先声明成可空，是为了让下面的局部函数能引用它（局部函数只看得见它**之前**的声明）
    var listener: LocationListener? = null

    /** 摘掉监听与所有定时器。三条收工路径都走这里，保证 GPS 引擎不会被我们一直开着 */
    fun cleanup() {
      handler.removeCallbacksAndMessages(null)
      listener?.let { l ->
        try {
          lm.removeUpdates(l)
        } catch (e: Exception) {
          // 权限刚被撤销时会抛，忽略即可（没有监听在跑了）
        }
      }
    }

    /** 拿到结果：只认第一次 */
    fun succeed(loc: Location) {
      if (!done.compareAndSet(false, true)) return
      cleanup()
      resolveLocation(invoke, loc, false)
    }

    /** 到点了：手里有次好的就用次好的，什么都没有才报超时 */
    fun settle() {
      if (!done.compareAndSet(false, true)) return
      cleanup()
      val loc = best
      if (loc != null) {
        resolveLocation(invoke, loc, false)
      } else {
        invoke.reject("定位超时（${budgetMs}ms 内没有拿到任何定位）", CODE_TIMEOUT)
      }
    }

    val locationListener = object : LocationListener {
      override fun onLocationChanged(location: Location) {
        if (done.get()) return
        best = pickBetter(best, location)

        val acc = if (location.hasAccuracy()) location.accuracy else Float.MAX_VALUE
        val isGps = location.provider == LocationManager.GPS_PROVIDER
        if (isGps || acc <= GOOD_ENOUGH_ACCURACY_M) {
          // 够好了：立刻收工，别为了「更好」再等
          best?.let { succeed(it) }
          return
        }
        // 不够好：给 GPS 一点时间，但只排一次
        if (!graceScheduled) {
          graceScheduled = true
          handler.postDelayed({ settle() }, FIRST_FIX_GRACE_MS)
        }
      }

      // 下面三个在 API 30+ 有默认实现，但 minSdk 是 24 —— 老设备上不实现会
      // AbstractMethodError（框架会回调 onProviderEnabled/Disabled），所以一律显式实现。
      @Deprecated("Deprecated in Java")
      override fun onStatusChanged(provider: String?, status: Int, extras: Bundle?) {}

      override fun onProviderEnabled(provider: String) {}

      override fun onProviderDisabled(provider: String) {}
    }
    listener = locationListener

    try {
      for (p in providers) {
        // minTime=0 / minDistance=0：要的就是「一有定位马上给我」，节流由我们自己按精度判
        lm.requestLocationUpdates(p, 0L, 0f, locationListener, Looper.getMainLooper())
      }
    } catch (e: Exception) {
      // 权限刚被撤销 / provider 不存在
      invoke.reject("无法启动定位监听：${e.message}", CODE_NO_SERVICE)
      return
    }

    // 硬超时：保证在 Rust 给的预算内一定收工
    handler.postDelayed({ settle() }, budgetMs)
  }

  // ═══════════════════════════════════════════════════════════════════
  // 工具
  // ═══════════════════════════════════════════════════════════════════

  /** 「大致位置」权限是否已有 —— 这是能拿到坐标的最低要求 */
  private fun hasCoarseLocation(): Boolean =
    PermissionHelper.hasPermissions(ctx, arrayOf(Manifest.permission.ACCESS_COARSE_LOCATION))

  private fun isProviderEnabled(lm: LocationManager, provider: String): Boolean = try {
    lm.isProviderEnabled(provider)
  } catch (e: Exception) {
    false
  }

  /** 把 Android 的 Location 转成 Rust 侧 `loc_android::MobileFix` 认识的 JSON */
  private fun resolveLocation(invoke: Invoke, loc: Location, cached: Boolean) {
    val lat = loc.latitude
    val lng = loc.longitude
    // JSONObject 不接受 NaN / Infinity（会抛 JSONException），先挡掉
    if (!lat.isFinite() || !lng.isFinite()) {
      invoke.reject("定位结果无效（坐标不是有限数）", CODE_BAD_FIX)
      return
    }

    val out = JSObject()
    out.put("lat", lat)
    out.put("lng", lng)
    // hasAccuracy()==false 时 accuracy 是 0：宁可不传，Rust 侧当未知，precision 落到 "city"
    if (loc.hasAccuracy() && loc.accuracy > 0f) {
      out.put("accuracy", loc.accuracy.toDouble())
    }
    val provider: String = loc.provider ?: ""
    out.put("provider", provider)
    out.put("ageMs", ((SystemClock.elapsedRealtimeNanos() - loc.elapsedRealtimeNanos) / 1_000_000L).coerceAtLeast(0L))
    out.put("cached", cached)
    invoke.resolve(out)
  }
}
