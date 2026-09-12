# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# If your project uses WebView with JS, uncomment the following
# and specify the fully qualified class name to the JavaScript interface
# class:
#-keepclassmembers class fqcn.of.javascript.interface.for.webview {
#   public *;
#}

# Uncomment this to preserve the line number information for
# debugging stack traces.
#-keepattributes SourceFile,LineNumberTable

# If you keep the line number information, uncomment this to
# hide the original source file name.
#-renamesourcefileattribute SourceFile

# ══════════════════════════════════════════════════════════════════
# 世界模拟「真实 GPS 定位」的 Kotlin 插件（com.syuki.lingchat.location）
# ══════════════════════════════════════════════════════════════════
# 这个类**不是**被代码直接 new 出来的，而是 Rust 侧通过
#   register_android_plugin("com.syuki.lingchat.location", "LocationPlugin")
#   → wry 的 WryActivity.getAppClass(name) → Class.forName(name)
# 按**字符串**反射加载的，release 包开了 isMinifyEnabled = true，
# 一旦被 R8 改名或裁掉，只在 release 包上静默失效（永远退回 IP 定位，不报错）。
#
# tauri-android AAR 自带的 consumer 规则里已经有
#   -keep @app.tauri.annotation.TauriPlugin public class * { @Command ... }
# 所以正常情况**不需要**下面这两条；这里是兜底 —— 反射还依赖注解本身
# （PluginHandle.indexMethods() 用 isAnnotationPresent(Command::class.java) 找方法），
# 而注解属性在 R8 下不一定默认保留。两条都只「禁止裁剪」，不改变任何既有行为。
-keepattributes *Annotation*
-keep class com.syuki.lingchat.location.** { *; }
