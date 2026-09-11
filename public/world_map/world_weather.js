/* world_weather.js — L-SYuki 天气系统 (T4-2)
 * 纯前端、零依赖，浏览器与 Node 双端可用（Node 下 require 可直接跑自检）。
 *
 * 数据源：hier_api.py /api/weather?city=  （wttr.in 真实天气）
 *   desc/desc_en/temp_c/feels_like_c/humidity/wind_kmph/cloudcover/precip_mm
 *   visibility_km/is_rain/is_snow/is_fog
 *
 * 提供：
 *   1. 天气状态对象  normalize()/state()：晴/阴/雨/雪/雾 + 温度 + 云量 + 强度
 *   2. 视觉表现（核心）：
 *        applyWeatherTint(ctx, weather, w, h)   天气色调（雨灰蓝 / 雪高亮偏白 / 雾低对比 / 晴暖亮）
 *        drawRain(ctx, w, h, intensity, t)      雨丝动画（斜线，密度随 intensity）
 *        drawSnow(ctx, w, h, intensity, t)      雪花飘落动画
 *        drawFog(ctx, w, h, intensity, t)       雾/霾（半透明叠加 + 柔边模糊感）
 *        drawWeather(...)                       统一入口（含雷雨闪光）
 *   3. 天气影响地图：applyWeatherSurface(ctx, weather, w, h, geo)  雨天路面变暗 / 雪天屋顶变白
 *   4. WeatherLayer：canvas 叠加层 + requestAnimationFrame 驱动（粒子数上限、离屏复用、按帧率节流）
 *
 * 性能约定：所有粒子位置由 t 与索引哈希即时算出，帧间不保存粒子数组 → 零 GC 压力；
 *          雾用一次性生成的离屏软边贴图复用；粒子总数有硬上限（默认雨 260 / 雪 220）。
 */
(function (root, factory) {
  var api = factory();
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  if (root) root.WORLD_WEATHER = api;
})(typeof globalThis !== 'undefined' ? globalThis : this, function () {
  'use strict';

  var API_BASE = 'http://127.0.0.1:8790';
  var TAU = Math.PI * 2;

  // ─────────────────────────── 工具 ───────────────────────────
  function clamp(v, a, b) { return v < a ? a : (v > b ? b : v); }
  /** 索引哈希 → [0,1)：帧间稳定、无需保存随机数 */
  function hash1(i) { var x = Math.sin(i * 127.1 + 311.7) * 43758.5453123; return x - Math.floor(x); }
  function rgba(c, a) { return 'rgba(' + c[0] + ',' + c[1] + ',' + c[2] + ',' + (Math.round(a * 1000) / 1000) + ')'; }

  // ─────────────────────────── 1. 天气状态 ───────────────────────────

  var KIND_LABEL = {
    clear: '晴', partly: '局部多云', cloudy: '多云', overcast: '阴',
    rain: '雨', thunder: '雷雨', snow: '雪', fog: '雾', haze: '霾', unknown: '未知'
  };
  var KIND_ICON = {
    clear: '☀️', partly: '🌤️', cloudy: '⛅', overcast: '☁️',
    rain: '🌧️', thunder: '⛈️', snow: '❄️', fog: '🌫️', haze: '😶\u200d🌫️', unknown: '❔'
  };

  /** 判定 天气类型（优先级：雪 > 雷雨 > 雨 > 雾 > 霾 > 阴/多云/晴） */
  function classify(d) {
    d = d || {};
    var en = String(d.desc_en || '').toLowerCase();
    var zh = String(d.desc || '');
    var cc = Number(d.cloudcover || 0);
    if (d.is_snow || /snow|blizzard|sleet/.test(en) || zh.indexOf('雪') >= 0) return 'snow';
    if (/thunder/.test(en) || zh.indexOf('雷') >= 0) return 'thunder';
    if (d.is_rain || /rain|drizzle|shower/.test(en) || zh.indexOf('雨') >= 0) return 'rain';
    if (d.is_fog || /fog|mist/.test(en) || zh.indexOf('雾') >= 0) return 'fog';
    if (/haze|smog|sand|dust/.test(en) || zh.indexOf('霾') >= 0) return 'haze';
    if (/overcast/.test(en) || zh.indexOf('阴') >= 0 || cc >= 88) return 'overcast';
    // 注意：文本判定要早于纯云量阈值（"Partly cloudy" 同时含 partly 与 cloud）
    if (/partly|mostly sunny|scattered|few cloud/.test(en) || zh.indexOf('局部') >= 0) return 'partly';
    if (/cloud/.test(en) || zh.indexOf('多云') >= 0 || cc >= 55) return 'cloudy';
    if (/sunny|clear/.test(en) || zh.indexOf('晴') >= 0) return 'clear';
    if (cc >= 25) return 'partly';
    return cc < 25 ? 'clear' : 'cloudy';
  }

  /** 文字等级：毛毛雨 0.25 / 小 0.4 / 中 0.6 / 大 0.85 / 暴 1.0 */
  function levelFromText(text) {
    var s = String(text || '').toLowerCase();
    if (/torrential|blizzard|暴/.test(s)) return 1.0;
    if (/heavy|violent|大/.test(s)) return 0.85;
    if (/moderate|中/.test(s)) return 0.6;
    if (/light|small|patchy|drizzle|毛毛|小|零星/.test(s)) return 0.4;
    if (/possible|nearby|附近|可能/.test(s)) return 0.3;
    return null;
  }
  /** 降水强度 0..1（文字等级 7 : 降水量 3 加权；无雨雪为 0） */
  function intensityOf(kind, d) {
    d = d || {};
    var precip = Number(d.precip_mm || 0);
    var pLevel = precip <= 0 ? 0 : (precip < 0.5 ? 0.3 : precip < 2.5 ? 0.5 : precip < 7.6 ? 0.75 : 1.0);
    if (kind === 'rain' || kind === 'thunder' || kind === 'snow') {
      var txt = levelFromText(d.desc_en) != null ? levelFromText(d.desc_en) : levelFromText(d.desc);
      var base = (txt != null) ? (0.7 * txt + 0.3 * pLevel) : (pLevel || 0.35);
      if (kind === 'thunder') base = Math.max(base, 0.6);
      return clamp(base, 0.22, 1);
    }
    if (kind === 'fog' || kind === 'haze') {
      var v = Number(d.visibility_km);
      if (!isFinite(v) || v <= 0) return 0.6;
      if (v <= 1) return 1.0; if (v <= 2) return 0.85; if (v <= 5) return 0.68;
      if (v <= 10) return 0.45; return 0.3;
    }
    return 0;   // 晴/阴：无降水强度
  }

  /** /api/weather 原始 payload → 统一天气状态对象 */
  function normalize(raw) {
    raw = raw || {};
    var kind = classify(raw);
    var clouds = clamp(Number(raw.cloudcover != null ? raw.cloudcover : 0), 0, 100);
    return {
      kind: kind,
      kindLabel: KIND_LABEL[kind],
      label: KIND_LABEL[kind],
      icon: KIND_ICON[kind],
      desc: raw.desc || KIND_LABEL[kind],
      desc_en: raw.desc_en || '',
      city: raw.city || '',
      temp_c: raw.temp_c != null ? Number(raw.temp_c) : null,
      feels_like_c: raw.feels_like_c != null ? Number(raw.feels_like_c) : null,
      humidity: raw.humidity != null ? Number(raw.humidity) : null,
      wind_kmph: Number(raw.wind_kmph || 0),
      cloudcover: clouds,
      cover: clouds / 100,                     // 云量 0..1
      precip_mm: Number(raw.precip_mm || 0),
      visibility_km: raw.visibility_km != null ? Number(raw.visibility_km) : null,
      intensity: intensityOf(kind, raw),       // 降水/雾的视觉强度 0..1
      isRain: kind === 'rain' || kind === 'thunder',
      isSnow: kind === 'snow',
      isFog: kind === 'fog' || kind === 'haze',
      isThunder: kind === 'thunder',
      isWet: kind === 'rain' || kind === 'thunder',   // 需要"湿地面"效果
      cached: !!raw.cached,
      error: raw.error || null,
      fetchedAt: Date.now()
    };
  }

  // ─────────────────────────── 2. 天气色调 ───────────────────────────

  /* 每种天气对地图整体的一层色（mode: source-over 压暗/洗白，screen 提亮） */
  var WEATHER_TINT = {
    clear:    { c: [255, 236, 188], a: 0.12, mode: 'screen',      label: '晴·暖亮' },
    partly:   { c: [216, 230, 244], a: 0.07, mode: 'screen',      label: '局部多云·轻提亮' },
    cloudy:   { c: [142, 158, 178], a: 0.13, mode: 'source-over', label: '多云·偏灰' },
    overcast: { c: [104, 118, 138], a: 0.21, mode: 'source-over', label: '阴·压暗' },
    rain:     { c: [58, 84, 116],   a: 0.27, mode: 'source-over', label: '雨·灰蓝' },
    thunder:  { c: [36, 46, 70],    a: 0.36, mode: 'source-over', label: '雷雨·沉重' },
    snow:     { c: [228, 240, 255], a: 0.26, mode: 'screen',      label: '雪·高亮偏白' },
    fog:      { c: [196, 204, 212], a: 0.36, mode: 'source-over', label: '雾·低对比' },
    haze:     { c: [198, 184, 164], a: 0.30, mode: 'source-over', label: '霾·泛黄' },
    unknown:  { c: [128, 128, 128], a: 0.00, mode: 'source-over', label: '未知' }
  };

  /** 取某天气对应的 RGBA 叠加色（alpha 随强度缩放；无降水时保持基准） */
  function weatherTint(weather) {
    var w = (typeof weather === 'string') ? { kind: weather, intensity: 1 } : (weather || {});
    var p = WEATHER_TINT[w.kind] || WEATHER_TINT.unknown;
    var it = Number(w.intensity || 0);
    var k = (w.kind === 'rain' || w.kind === 'thunder' || w.kind === 'snow' || w.kind === 'fog' || w.kind === 'haze')
      ? (0.55 + 0.45 * clamp(it, 0, 1))      // 降水类：强度越高色越重
      : 1;
    var a = Math.round(p.a * k * 1000) / 1000;
    return {
      kind: w.kind || 'unknown', r: p.c[0], g: p.c[1], b: p.c[2], a: a, alpha: a,
      mode: p.mode, label: p.label, css: rgba(p.c, a)
    };
  }

  /** 天气色调叠加到 2D 画布（core API） */
  function applyWeatherTint(ctx, weather, w, h, opts) {
    if (!ctx) return null;
    opts = opts || {};
    var t = weatherTint(weather);
    if (t.a <= 0) return t;
    w = w || ctx.canvas.width; h = h || ctx.canvas.height;
    var prevOp = ctx.globalCompositeOperation, prevA = ctx.globalAlpha;
    ctx.globalCompositeOperation = t.mode;      // screen=提亮 / source-over=压暗
    if (opts.alphaScale != null) ctx.globalAlpha = clamp(opts.alphaScale, 0, 1);
    ctx.fillStyle = rgba([t.r, t.g, t.b], t.mode === 'screen' ? t.a : t.a);
    ctx.fillRect(0, 0, w, h);
    ctx.globalCompositeOperation = prevOp; ctx.globalAlpha = prevA;
    return t;
  }

  // ─────────────────────────── 3. 粒子动画（雨/雪/雾/闪电）───────────────────────────

  /**
   * 雨丝动画：斜线，密度随 intensity。位置由 t 即时算出，帧间无状态。
   * @param {CanvasRenderingContext2D} ctx
   * @param {number} w,h      画布尺寸
   * @param {number} intensity 0..1
   * @param {number} t        毫秒时间戳（performance.now）
   * @param {object} [opts]   {maxParticles, slant, speed, color}
   * @returns {number} 实际绘制的雨丝数
   */
  function drawRain(ctx, w, h, intensity, t, opts) {
    intensity = clamp(Number(intensity) || 0, 0, 1);
    if (intensity <= 0.01 || !ctx) return 0;
    opts = opts || {};
    var maxN = opts.maxParticles || 260;              // 粒子硬上限
    var n = Math.round(maxN * intensity);              // 密度 ∝ 强度
    var slant = (opts.slant == null) ? 0.26 : opts.slant;   // 斜线斜率（风偏）
    var speed = opts.speed || 1;
    var col = opts.color || [198, 220, 240];
    var ts = (t || 0) / 1000;
    var buckets = [                                    // 3 档粗细：批量描边，减少状态切换
      { lw: 1.0, a: 0.20, len: 13 },
      { lw: 1.4, a: 0.32, len: 21 },
      { lw: 1.8, a: 0.44, len: 30 }
    ];
    var drawn = 0;
    for (var b = 0; b < buckets.length; b++) {
      var bk = buckets[b];
      ctx.beginPath();
      for (var i = b; i < n; i += buckets.length) {
        var s1 = hash1(i * 3.1 + 1), s2 = hash1(i * 7.3 + 2), s3 = hash1(i * 11.7 + 3);
        var len = bk.len * (0.65 + s3 * 0.75) * (0.75 + 0.45 * intensity);
        var spd = (620 + s2 * 560) * (0.55 + 0.65 * intensity) * speed;     // px/s
        var x = s1 * (w + 160) - 80;
        var y = (((s2 * 4096) + ts * spd) % (h + 90)) - 45;
        ctx.moveTo(x, y);
        ctx.lineTo(x - len * slant, y + len);
        drawn++;
      }
      ctx.strokeStyle = rgba(col, clamp(bk.a * (0.7 + 0.5 * intensity), 0, 1));
      ctx.lineWidth = bk.lw;
      ctx.lineCap = 'round';
      ctx.stroke();
    }
    return drawn;
  }

  /**
   * 雪花飘落：正弦横摆 + 分档大小，批量填充（一次 fill 画完一档）。
   * @returns {number} 实际绘制的雪花数
   */
  function drawSnow(ctx, w, h, intensity, t, opts) {
    intensity = clamp(Number(intensity) || 0, 0, 1);
    if (intensity <= 0.01 || !ctx) return 0;
    opts = opts || {};
    var maxN = opts.maxParticles || 220;
    var n = Math.round(maxN * intensity);
    var ts = (t || 0) / 1000;
    var col = opts.color || [255, 255, 255];
    var buckets = [
      { r: 1.0, a: 0.85 },
      { r: 1.9, a: 0.68 },
      { r: 3.1, a: 0.50 }
    ];
    var drawn = 0;
    for (var b = 0; b < buckets.length; b++) {
      var bk = buckets[b];
      ctx.beginPath();
      for (var i = b; i < n; i += buckets.length) {
        var s1 = hash1(i * 5.1 + 1), s2 = hash1(i * 13.3 + 2), s3 = hash1(i * 17.9 + 3), s4 = hash1(i * 19.1 + 4);
        var spd = (34 + s2 * 76) * (0.6 + 0.6 * intensity);              // 下落 px/s
        var sway = (10 + s3 * 26) * (0.5 + intensity);
        var x = (s1 * (w + 40) - 20) + Math.sin(ts * (0.5 + s4) + s4 * TAU) * sway;
        var y = (((s4 * 4096) + ts * spd) % (h + 30)) - 15;
        var r = bk.r * (0.8 + s3 * 0.4);
        ctx.moveTo(x + r, y);
        ctx.arc(x, y, r, 0, TAU);
        drawn++;
      }
      ctx.fillStyle = rgba(col, clamp(bk.a * (0.65 + 0.4 * intensity), 0, 1));
      ctx.fill();
    }
    return drawn;
  }

  // 雾用的软边贴图：惰性生成一次，之后每帧只 drawImage（离屏复用）
  var _puff = null, _puffColor = '';
  function getPuff(color) {
    if (typeof document === 'undefined' || !document.createElement) return null;
    if (_puff && _puffColor === color) return _puff;
    var S = 256, cv = document.createElement('canvas');
    cv.width = cv.height = S;
    var g = cv.getContext('2d');
    var rg = g.createRadialGradient(S / 2, S / 2, 0, S / 2, S / 2, S / 2);
    rg.addColorStop(0, rgba(color, 0.55));      // 中心柔和，边缘完全透明 → 自带"模糊感"
    rg.addColorStop(0.45, rgba(color, 0.28));
    rg.addColorStop(0.75, rgba(color, 0.09));
    rg.addColorStop(1, rgba(color, 0));
    g.fillStyle = rg; g.fillRect(0, 0, S, S);
    _puff = cv; _puffColor = color;
    return cv;
  }

  /**
   * 雾 / 霾：大尺寸柔边雾团横向缓慢漂移叠加（低对比、模糊感）。
   * 命中离屏软边贴图，每团一次 drawImage；无 DOM（Node 自检）时退回径向渐变。
   * @returns {number} 绘制的雾团数
   */
  function drawFog(ctx, w, h, intensity, t, opts) {
    intensity = clamp(Number(intensity) || 0, 0, 1);
    if (intensity <= 0.01 || !ctx) return 0;
    opts = opts || {};
    var count = opts.puffs || Math.round(7 + 7 * intensity);
    var col = opts.color || [214, 220, 228];
    var ts = (t || 0) / 1000;
    var sprite = opts.sprite !== false ? getPuff(col) : null;
    var prevA = ctx.globalAlpha;
    for (var i = 0; i < count; i++) {
      var s1 = hash1(i * 23.7 + 7), s2 = hash1(i * 29.3 + 11), s3 = hash1(i * 31.1 + 13);
      var R = (0.30 + s1 * 0.45) * Math.max(w, h) * (0.65 + 0.55 * intensity);
      var x = ((ts * (6 + 10 * s3) + s1 * w * 1.7) % (w + 2 * R)) - R;   // 缓慢横移
      var y = h * (0.15 + s2 * 0.8) + Math.sin(ts * 0.18 + i) * h * 0.035;
      ctx.globalAlpha = clamp((0.08 + 0.20 * s2) * (0.45 + 0.75 * intensity), 0, 0.6);
      if (sprite) {
        ctx.drawImage(sprite, x - R, y - R, R * 2, R * 2);
      } else {
        var rg = ctx.createRadialGradient(x, y, 0, x, y, R);
        rg.addColorStop(0, rgba(col, 0.5)); rg.addColorStop(1, rgba(col, 0));
        ctx.fillStyle = rg; ctx.beginPath(); ctx.arc(x, y, R, 0, TAU); ctx.fill();
      }
    }
    ctx.globalAlpha = prevA;
    return count;
  }

  /** 雷雨闪电：按 5 秒一个时隙随机触发，双次闪烁 */
  function drawLightning(ctx, w, h, t, opts) {
    if (!ctx) return 0;
    opts = opts || {};
    var ts = (t || 0) / 1000, slot = Math.floor(ts / 5);
    if (hash1(slot * 3.7 + 1) > 0.6) return 0;          // 约 60% 的时隙有闪电
    var local = ts % 5;
    var k = 0;
    if (local < 0.12) k = 1 - local / 0.12;
    else if (local > 0.22 && local < 0.30) k = (0.30 - local) / 0.08 * 0.7;
    if (k <= 0) return 0;
    var prevOp = ctx.globalCompositeOperation;
    ctx.globalCompositeOperation = 'screen';
    ctx.fillStyle = 'rgba(214,232,255,' + (0.45 * k).toFixed(3) + ')';
    ctx.fillRect(0, 0, w, h);
    ctx.globalCompositeOperation = prevOp;
    return 1;
  }

  /** 统一入口：按天气类型画粒子（雨/雪/雾/雷雨闪光） */
  function drawWeather(ctx, weather, w, h, t, opts) {
    var w0 = (typeof weather === 'string') ? { kind: weather, intensity: 1 } : (weather || {});
    var it = w0.intensity == null ? 1 : w0.intensity;
    var out = { kind: w0.kind || 'unknown', particles: 0, fx: 0 };
    if (w0.kind === 'rain' || w0.kind === 'thunder') {
      out.particles = drawRain(ctx, w, h, it, t, opts);
      if (w0.kind === 'thunder') out.fx = drawLightning(ctx, w, h, t, opts);
    } else if (w0.kind === 'snow') {
      out.particles = drawSnow(ctx, w, h, it, t, opts);
    } else if (w0.kind === 'fog' || w0.kind === 'haze') {
      out.particles = drawFog(ctx, w, h, it, t, opts);
    }
    return out;
  }

  // ─────────────────────────── 4. 天气影响地图表面 ───────────────────────────

  /**
   * 天气对地图要素的影响（可简化：有 geo 就精确画，没有就跑全屏近似）。
   * geo = { roads: [[{x,y},…], …], roofs: [{x,y,w,h}|[{x,y},…], …] }
   *   · 雨天：路面变暗 + 一道湿反光
   *   · 雪天：屋顶变白（积雪）
   * @returns {{mode:string, items:number}}
   */
  function applyWeatherSurface(ctx, weather, w, h, geo, opts) {
    if (!ctx) return { mode: 'none', items: 0 };
    var w0 = (typeof weather === 'string') ? { kind: weather, intensity: 1 } : (weather || {});
    var it = clamp(Number(w0.intensity == null ? 1 : w0.intensity), 0, 1);
    opts = opts || {};
    geo = geo || {};
    var mode = 'none', items = 0;

    // ① 雨天：路面变暗（湿地面）
    if (w0.kind === 'rain' || w0.kind === 'thunder') {
      mode = 'wet';
      if (geo.roads && geo.roads.length) {
        ctx.save();
        ctx.lineCap = 'round'; ctx.lineJoin = 'round';
        ctx.strokeStyle = 'rgba(14,22,34,' + (0.30 + 0.25 * it).toFixed(3) + ')';   // 路面变暗
        ctx.lineWidth = opts.roadWidth || 6;
        for (var r = 0; r < geo.roads.length; r++) {
          var pts = geo.roads[r]; if (!pts || pts.length < 2) continue;
          ctx.beginPath(); ctx.moveTo(pts[0].x, pts[0].y);
          for (var p = 1; p < pts.length; p++) ctx.lineTo(pts[p].x, pts[p].y);
          ctx.stroke();
          ctx.strokeStyle = 'rgba(150,190,225,' + (0.10 + 0.14 * it).toFixed(3) + ')';  // 湿反光
          ctx.lineWidth = Math.max(1, (opts.roadWidth || 6) * 0.28);
          ctx.stroke();
          ctx.strokeStyle = 'rgba(14,22,34,' + (0.30 + 0.25 * it).toFixed(3) + ')';
          ctx.lineWidth = opts.roadWidth || 6;
          items++;
        }
        ctx.restore();
      } else {
        // 无几何数据：底部渐深（近似"湿地面反光"），避免逐像素处理
        var g = ctx.createLinearGradient(0, 0, 0, h);
        g.addColorStop(0, 'rgba(16,26,40,' + (0.06 * (0.5 + it)).toFixed(3) + ')');
        g.addColorStop(1, 'rgba(10,18,30,' + (0.26 * (0.5 + it)).toFixed(3) + ')');
        ctx.save(); ctx.fillStyle = g; ctx.fillRect(0, 0, w, h); ctx.restore();
        items = 0;
      }
    }
    // ② 雪天：屋顶变白（积雪）
    else if (w0.kind === 'snow') {
      mode = 'snowcap';
      ctx.save();
      if (geo.roofs && geo.roofs.length) {
        ctx.fillStyle = 'rgba(246,250,255,' + (0.55 + 0.35 * it).toFixed(3) + ')';
        for (var i2 = 0; i2 < geo.roofs.length; i2++) {
          var f = geo.roofs[i2]; if (!f) continue;
          ctx.beginPath();
          if (f.length) {                                   // 多边形屋顶
            ctx.moveTo(f[0].x, f[0].y);
            for (var q = 1; q < f.length; q++) ctx.lineTo(f[q].x, f[q].y);
            ctx.closePath();
          } else if (f.w != null) {                          // 矩形屋顶
            ctx.rect(f.x, f.y, f.w, f.h);
          } else continue;
          ctx.fill();
          items++;
        }
      } else {
        // 无几何数据：按索引撒白色小片模拟积雪（数量随强度）
        ctx.fillStyle = 'rgba(250,253,255,' + (0.22 + 0.28 * it).toFixed(3) + ')';
        var n = Math.round(40 + 90 * it);
        for (var s = 0; s < n; s++) {
          var sx = hash1(s * 3.3 + 1) * w, sy = hash1(s * 7.7 + 2) * h;
          var rr = 2 + hash1(s * 11.1 + 3) * 7;
          ctx.beginPath(); ctx.ellipse(sx, sy, rr * 1.6, rr * 0.7, 0, 0, TAU); ctx.fill();
          items++;
        }
        ctx.fillStyle = 'rgba(255,255,255,' + (0.10 + 0.16 * it).toFixed(3) + ')';
        ctx.fillRect(0, 0, w, Math.max(2, h * 0.16));       // 顶部更亮（积雪感）
      }
      ctx.restore();
    }
    return { mode: mode, items: items };
  }

  /**
   * 一次性铺满"大气层"：先天气色调，再昼夜色调（夜间压暗应覆盖天气提亮）。
   * 需要 window.WORLD_TIME 提供 timeTint（未加载时自动跳过昼夜层）。
   */
  function applyAtmosphere(ctx, hour, weather, w, h, opts) {
    var out = { weather: null, time: null };
    out.weather = applyWeatherTint(ctx, weather, w, h, opts);
    var WT = (typeof window !== 'undefined' && window.WORLD_TIME) ||
             (typeof WORLD_TIME !== 'undefined' ? WORLD_TIME : null);
    if (WT && hour != null) out.time = WT.applyTimeTint(ctx, hour, w, h, opts);
    return out;
  }

  // ─────────────────────────── 5. 数据获取 ───────────────────────────

  var _handlers = { change: [], error: [] };
  var _state = normalize({ kind: 'unknown' });
  var _autoTimer = null, _city = '', _api = API_BASE;

  function on(ev, fn) { if (_handlers[ev] && typeof fn === 'function') _handlers[ev].push(fn); return fn; }
  function off(ev, fn) { var l = _handlers[ev]; if (!l) return; var i = l.indexOf(fn); if (i >= 0) l.splice(i, 1); }
  function emit(ev, payload) { (_handlers[ev] || []).forEach(function (f) { try { f(payload); } catch (e) { } }); }

  /** 拉取真实天气；city 为空则服务端按设备定位反查城市 */
  function fetchWeather(city) {
    if (city != null) _city = city;
    if (typeof fetch !== 'function') return Promise.resolve(_state);
    return fetch(_api + '/api/weather' + (_city ? '?city=' + encodeURIComponent(_city) : ''), { cache: 'no-store' })
      .then(function (r) { return r.json(); })
      .then(function (d) {
        _state = normalize(d);
        emit('change', _state);
        if (_state.error) emit('error', _state);
        return _state;
      })
      .catch(function (e) {
        _state = Object.assign(normalize({ kind: 'unknown' }), { error: String(e && e.message || e) });
        emit('error', _state);
        return _state;
      });
  }

  /** 演示/离线用：直接设定天气类型与强度 */
  function setManual(kind, intensity, extra) {
    _state = Object.assign(normalize({ kind: 'unknown' }), {
      kind: kind, kindLabel: KIND_LABEL[kind] || kind, label: KIND_LABEL[kind] || kind,
      icon: KIND_ICON[kind] || '❔', desc: KIND_LABEL[kind] || kind,
      intensity: intensity == null ? 0.6 : clamp(intensity, 0, 1),
      cloudcover: kind === 'clear' ? 5 : kind === 'partly' ? 40 : kind === 'cloudy' ? 70 : 95,
      cover: kind === 'clear' ? 0.05 : kind === 'partly' ? 0.4 : kind === 'cloudy' ? 0.7 : 0.95,
      isRain: kind === 'rain' || kind === 'thunder', isSnow: kind === 'snow',
      isFog: kind === 'fog' || kind === 'haze', isThunder: kind === 'thunder',
      isWet: kind === 'rain' || kind === 'thunder',
      source: 'manual'
    }, extra || {});
    emit('change', _state);
    return _state;
  }

  /** 定时自动刷新（默认 30 分钟，与服务端缓存 TTL 一致） */
  function startAuto(intervalMs, city) {
    stopAuto();
    fetchWeather(city);
    _autoTimer = setInterval(function () { fetchWeather(); }, intervalMs || 30 * 60 * 1000);
    return _state;
  }
  function stopAuto() { if (_autoTimer) clearInterval(_autoTimer); _autoTimer = null; }
  function state() { return _state; }

  // ─────────────────────────── 6. canvas 叠加层（RAF 驱动）───────────────────────────

  /**
   * WeatherLayer — 把天气叠在任意地图画布之上。
   * 用法：
   *   var layer = new WORLD_WEATHER.WeatherLayer(document.getElementById('fx'));
   *   layer.setWeather(weatherState); layer.setTime(hour);   // hour 为 null 时不叠加昼夜
   *   layer.start();
   * 特性：粒子数硬上限、离屏雾贴图复用、帧率节流、页面隐藏自动暂停、Dpr ≤ 2。
   */
  function WeatherLayer(canvas, opts) {
    if (!canvas) throw new Error('WeatherLayer 需要 canvas');
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d');
    this.opts = Object.assign({
      maxDpr: 2, targetFps: 60, maxRain: 260, maxSnow: 220,
      tint: true, particles: true, surface: true, geo: null,
      onFrame: null
    }, opts || {});
    this.weather = null;
    this.hour = null;
    this._raf = null;
    this._last = 0;
    this._acc = 0;
    this.frames = 0;
    this.fps = 0;
    this.lastCost = 0;
    this.lastParticles = 0;
    this.visible = true;
    var self = this;
    if (typeof document !== 'undefined' && document.addEventListener) {
      this._vis = function () { self.visible = !(document.hidden || document.visibilityState === 'hidden'); };
      document.addEventListener('visibilitychange', this._vis);
    }
  }
  WeatherLayer.prototype.setWeather = function (w) { this.weather = w; return this; };
  WeatherLayer.prototype.setTime = function (hour) { this.hour = (hour && typeof hour === 'object') ? hour.hour : hour; return this; };
  WeatherLayer.prototype.setGeo = function (geo) { this.opts.geo = geo; return this; };
  /** 按父容器（或指定尺寸）适配画布，Dpr 上限 2 保性能 */
  WeatherLayer.prototype.resize = function (cssW, cssH) {
    var c = this.canvas, parent = c.parentNode;
    if (cssW == null) {
      cssW = (parent && parent.clientWidth) || c.clientWidth || 800;
      cssH = (parent && parent.clientHeight) || c.clientHeight || 600;
    }
    var dpr = Math.min(this.opts.maxDpr, (typeof devicePixelRatio !== 'undefined' ? devicePixelRatio : 1) || 1);
    this.cssW = cssW; this.cssH = cssH; this.dpr = dpr;
    c.width = Math.max(1, Math.round(cssW * dpr));
    c.height = Math.max(1, Math.round(cssH * dpr));
    c.style.width = cssW + 'px'; c.style.height = cssH + 'px';
    if (this.ctx.setTransform) this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    return this;
  };
  WeatherLayer.prototype.start = function () {
    if (this._raf) return this;
    var self = this;
    this._last = 0;
    this._raf = (typeof requestAnimationFrame === 'function')
      ? requestAnimationFrame(function loop(ts) { self._frame(ts); self._raf = requestAnimationFrame(loop); })
      : null;
    return this;
  };
  WeatherLayer.prototype.stop = function () {
    if (this._raf && typeof cancelAnimationFrame === 'function') cancelAnimationFrame(this._raf);
    this._raf = null; return this;
  };
  WeatherLayer.prototype.destroy = function () {
    this.stop();
    if (this._vis && typeof document !== 'undefined') document.removeEventListener('visibilitychange', this._vis);
  };
  /** 单帧：清屏 → 天气色调 → 表面影响 → 粒子；t 用 performance.now() 保证动画连续 */
  WeatherLayer.prototype._frame = function (ts) {
    if (!ts) ts = (typeof performance !== 'undefined' ? performance.now() : Date.now());
    if (!this._last) this._last = ts;
    var minGap = 1000 / (this.opts.targetFps || 60);
    if (!this._last) this._last = ts - minGap;          // 首帧立即出画（不白等一帧）
    var dt = ts - this._last; this._last = ts;
    this._acc += dt;
    if (this._acc < minGap) return;                                          // 帧率节流
    this._acc = 0;
    if (!this.visible) return;
    var t0 = (typeof performance !== 'undefined' ? performance.now() : Date.now());
    var ctx = this.ctx, w = this.cssW, h = this.cssH, w0 = this.weather || { kind: 'unknown' };
    ctx.clearRect(0, 0, w, h);
    if (this.opts.surface) applyWeatherSurface(ctx, w0, w, h, this.opts.geo);
    if (this.opts.tint) {
      applyWeatherTint(ctx, w0, w, h);
      var WT = (typeof window !== 'undefined' && window.WORLD_TIME) || null;
      if (WT && this.hour != null) WT.applyTimeTint(ctx, this.hour, w, h);
    }
    var res = { particles: 0 };
    if (this.opts.particles) {
      var it = w0.intensity == null ? 0 : w0.intensity;
      if (w0.kind === 'rain') res = drawWeather(ctx, Object.assign({}, w0, { intensity: it }), w, h, ts, { maxParticles: this.opts.maxRain });
      else if (w0.kind === 'thunder') res = drawWeather(ctx, Object.assign({}, w0, { intensity: it }), w, h, ts, { maxParticles: this.opts.maxRain });
      else if (w0.kind === 'snow') res = drawWeather(ctx, w0, w, h, ts, { maxParticles: this.opts.maxSnow });
      else if (w0.kind === 'fog' || w0.kind === 'haze') res = drawWeather(ctx, w0, w, h, ts);
    }
    this.lastParticles = res.particles || 0;
    this.frames++;
    if (this.frames % 30 === 0) this.fps = Math.round(1000 / Math.max(1, dt));
    this.lastCost = (typeof performance !== 'undefined' ? performance.now() : Date.now()) - t0;
    if (this.opts.onFrame) this.opts.onFrame(this);
  };

  return {
    // 状态
    API_BASE: API_BASE, KIND_LABEL: KIND_LABEL, KIND_ICON: KIND_ICON,
    classify: classify, normalize: normalize, intensityOf: intensityOf,
    // 色调
    WEATHER_TINT: WEATHER_TINT, weatherTint: weatherTint, applyWeatherTint: applyWeatherTint,
    // 粒子
    drawRain: drawRain, drawSnow: drawSnow, drawFog: drawFog, drawLightning: drawLightning,
    drawWeather: drawWeather,
    // 地图表面影响
    applyWeatherSurface: applyWeatherSurface, applyAtmosphere: applyAtmosphere,
    // 数据
    fetch: fetchWeather, state: state, setManual: setManual,
    startAuto: startAuto, stopAuto: stopAuto, on: on, off: off, setApi: function (u) { _api = u; },
    // 叠加层
    WeatherLayer: WeatherLayer
  };
});
