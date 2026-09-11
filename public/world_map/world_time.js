/* world_time.js — L-SYuki 时间系统 (T4-1)
 * 纯前端、零依赖，浏览器与 Node 双端可用（Node 下 require 可直接跑自检）。
 *
 * 职责：
 *   1. 时间状态对象：时段(dawn/morning/noon/afternoon/dusk/evening/night)、是否白天、当前小时
 *   2. 昼夜光照：timeTint(hour) → RGBA 叠加滤镜（凌晨深蓝 / 正午透明 / 黄昏橙红）
 *   3. 本地时钟推进：初始同步 /api/time，之后 setInterval 每秒自增，每 30 分钟校正一次
 *   4. 时段影响：isFacilityOpen(type, hour) 供设施系统使用（学校 7-18、商铺 9-22、酒吧 18-次日2 …）
 *   5. 日出日落：按纬度用简化太阳赤纬公式估算 {sunrise, sunset}
 *
 * 与后端 hier_api.py /api/time 的字段口径保持一致：
 *   period 边界 5/8/11/14/17/19/23；is_day = 6 <= h < 18
 */
(function (root, factory) {
  var api = factory();
  if (typeof module !== 'undefined' && module.exports) module.exports = api;   // Node 自检
  if (root) root.WORLD_TIME = api;                                              // 浏览器全局
})(typeof globalThis !== 'undefined' ? globalThis : this, function () {
  'use strict';

  var API_BASE = 'http://127.0.0.1:8790';   // 与 bigmap.html / hier_api.py 一致
  var D2R = Math.PI / 180, R2D = 180 / Math.PI;

  // ─────────────────────────── 1. 时段 / 时间状态 ───────────────────────────

  var PERIOD_ORDER = ['dawn', 'morning', 'noon', 'afternoon', 'dusk', 'evening', 'night'];
  var PERIOD_LABEL = {
    dawn: '黎明', morning: '清晨', noon: '正午', afternoon: '午后',
    dusk: '黄昏', evening: '傍晚', night: '夜晚'
  };
  var PERIOD_ICON = {
    dawn: '🌅', morning: '🌤️', noon: '☀️', afternoon: '🌤️',
    dusk: '🌇', evening: '🌆', night: '🌙'
  };

  function normHour(h) {           // 任意小时数 → [0,24)
    h = Number(h) || 0;
    h = h % 24;
    return h < 0 ? h + 24 : h;
  }

  /** 取时段（与 hier_api.time_payload 完全同界） */
  function periodOf(hour) {
    var h = Math.floor(normHour(hour));
    if (h >= 5 && h < 8) return 'dawn';
    if (h >= 8 && h < 11) return 'morning';
    if (h >= 11 && h < 14) return 'noon';
    if (h >= 14 && h < 17) return 'afternoon';
    if (h >= 17 && h < 19) return 'dusk';
    if (h >= 19 && h < 23) return 'evening';
    return 'night';               // 23,0,1,2,3,4
  }

  /** 是否白天（与后端 is_day 同界：6 ≤ h < 18） */
  function isDay(hour) { var h = normHour(hour); return h >= 6 && h < 18; }

  function periodLabel(period) { return PERIOD_LABEL[period] || period || ''; }
  function periodIcon(period) { return PERIOD_ICON[period] || '🕐'; }

  // ─────────────────────────── 2. 昼夜光照 timeTint ───────────────────────────

  /* 关键帧：{h: 小时, c:[r,g,b], a: alpha}
   * 深夜深蓝(不透明度高) → 黎明紫粉 → 日出暖橙 → 白天完全透明 → 黄昏橙红 → 暮色玫紫 → 夜深 */
  var TINT_KEYS = [
    { h: 0.0, c: [8, 16, 46], a: 0.56 },      // 深夜
    { h: 3.6, c: [8, 16, 46], a: 0.56 },
    { h: 4.8, c: [22, 34, 78], a: 0.50 },     // 拂晓前：蓝紫
    { h: 5.6, c: [78, 62, 108], a: 0.34 },    // 黎明：紫粉
    { h: 6.4, c: [196, 116, 78], a: 0.18 },   // 日出：暖橙
    { h: 7.6, c: [255, 198, 140], a: 0.08 },  // 晨光
    { h: 9.5, c: [255, 255, 255], a: 0.00 },  // 白天：透明
    { h: 15.5, c: [255, 255, 255], a: 0.00 },
    { h: 16.8, c: [255, 206, 150], a: 0.09 }, // 午后偏暖
    { h: 18.0, c: [255, 126, 64], a: 0.26 },  // 黄昏：橙红
    { h: 19.1, c: [150, 62, 96], a: 0.38 },   // 暮色：玫紫
    { h: 20.3, c: [34, 44, 96], a: 0.50 },
    { h: 22.0, c: [10, 20, 56], a: 0.55 },
    { h: 24.0, c: [8, 16, 46], a: 0.56 }
  ];

  /**
   * 该时刻对地图整体的颜色滤镜（线性插值关键帧）。
   * @param {number} hour 0~24，可带小数（17.5 = 17:30）
   * @returns {{r,g,b,a,css,alpha,period,isDay,label,phase}} RGBA 叠加色
   */
  function timeTint(hour) {
    var h = normHour(hour), i, k0 = TINT_KEYS[0], k1 = TINT_KEYS[TINT_KEYS.length - 1];
    for (i = 0; i < TINT_KEYS.length - 1; i++) {
      if (h >= TINT_KEYS[i].h && h <= TINT_KEYS[i + 1].h) { k0 = TINT_KEYS[i]; k1 = TINT_KEYS[i + 1]; break; }
    }
    var span = k1.h - k0.h, u = span > 0 ? (h - k0.h) / span : 0;
    var r = Math.round(k0.c[0] + (k1.c[0] - k0.c[0]) * u);
    var g = Math.round(k0.c[1] + (k1.c[1] - k0.c[1]) * u);
    var b = Math.round(k0.c[2] + (k1.c[2] - k0.c[2]) * u);
    var a = Math.round((k0.a + (k1.a - k0.a) * u) * 1000) / 1000;
    var p = periodOf(h);
    return {
      r: r, g: g, b: b, a: a, alpha: a,
      css: 'rgba(' + r + ',' + g + ',' + b + ',' + a + ')',
      period: p, isDay: isDay(h), label: periodLabel(p), hour: h,
      phase: h < 12 ? 'am' : 'pm'
    };
  }

  /** 接受 tint 对象或小时数，统一转 CSS 颜色 */
  function tintCss(t) { return (t && typeof t === 'object') ? t.css : timeTint(t).css; }

  /** 直接把昼夜滤镜涂到 2D 画布（w/h 缺省用画布尺寸） */
  function applyTimeTint(ctx, hour, w, h, opts) {
    if (!ctx) return null;
    opts = opts || {};
    var t = (hour && typeof hour === 'object' && hour.css) ? hour : timeTint(hour);
    if (t.a <= 0) return t;                                  // 白天无需绘制
    w = w || ctx.canvas.width; h = h || ctx.canvas.height;
    var prev = ctx.globalCompositeOperation;
    if (opts.mode) ctx.globalCompositeOperation = opts.mode;  // 默认 source-over
    ctx.fillStyle = t.css;
    ctx.fillRect(0, 0, w, h);
    ctx.globalCompositeOperation = prev;
    return t;
  }

  // ─────────────────────────── 3. 日出日落（简化太阳赤纬公式）───────────────────────────

  function dayOfYear(d) {
    var start = Date.UTC(d.getFullYear(), 0, 1);
    var cur = Date.UTC(d.getFullYear(), d.getMonth(), d.getDate());
    return Math.floor((cur - start) / 86400000) + 1;
  }
  /** 太阳赤纬（度）；用简化式 23.44°·sin(2π(doy-81)/365) 替代精确天文算法 */
  function solarDeclination(doy) { return 23.44 * Math.sin(2 * Math.PI * (doy - 81) / 365); }
  function localTzOffset() { return -new Date().getTimezoneOffset() / 60; }

  /**
   * 粗略估算日出/日落（本地钟点小时，含小数）。
   * 说明：忽略均时差（误差约 ±15 分钟）与海拔；时区按经度做太阳时校正。
   * @param {Date}   date      日期（缺省今天）
   * @param {number} lat       纬度
   * @param {number} lng       经度
   * @param {number} [tzHours] 时区（缺省取本机时区）
   * @returns {{sunrise,sunset,daylight,solarNoon,polar,declination}}
   *          polar: null=正常 / 'day'=极昼 / 'night'=极夜
   */
  function sunTimes(date, lat, lng, tzHours) {
    date = date || new Date();
    lat = Number(lat) || 0; lng = Number(lng) || 0;
    var tz = (tzHours == null) ? localTzOffset() : Number(tzHours);
    var doy = dayOfYear(date);
    var decl = solarDeclination(doy) * D2R;      // 赤纬（弧度）
    var phi = lat * D2R;
    var zen = 90.833 * D2R;                       // 含大气折射的日出天顶角
    var cosH = (Math.cos(zen) - Math.sin(phi) * Math.sin(decl)) / (Math.cos(phi) * Math.cos(decl));
    var solarNoon = 12 - (lng - 15 * tz) / 15;    // 地方时正午（经度校正）
    solarNoon = normHour(solarNoon);
    var polar = null, halfDay;
    if (cosH <= -1) { polar = 'day'; halfDay = 12; }        // 极昼
    else if (cosH >= 1) { polar = 'night'; halfDay = 0; }   // 极夜
    else halfDay = R2D * Math.acos(cosH) / 15;              // 半昼长（小时）
    return {
      sunrise: normHour(solarNoon - halfDay),
      sunset: normHour(solarNoon + halfDay),
      daylight: halfDay * 2,
      solarNoon: solarNoon,
      polar: polar,
      declination: decl * R2D,
      doy: doy
    };
  }

  /** 小数小时 → "HH:MM" */
  function fmtHour(h, withSec) {
    if (h == null || isNaN(h)) return '--:--';
    h = normHour(h);
    var m = Math.floor((h % 1) * 60), s = Math.floor(((h * 60) % 1) * 60);
    return pad(Math.floor(h)) + ':' + pad(m) + (withSec ? ':' + pad(s) : '');
  }
  function pad(n) { return (n < 10 ? '0' : '') + n; }

  /** 当前时刻在日出日落之间的进度（0=日出前，0.5=正午，1=日落后） */
  function sunProgress(hour, sun) {
    sun = sun || sunTimes(new Date());
    var h = normHour(hour), rise = sun.sunrise, set = sun.sunset;
    var span = set - rise; if (span <= 0) span += 24;
    var rel = h - rise; if (rel < 0) rel += 24;
    return Math.max(0, Math.min(1, rel / span));
  }

  // ─────────────────────────── 4. 设施营业时段 ───────────────────────────

  /* open/close 为本地钟点（支持小数）；close < open 表示跨夜（如酒吧 18→次日 2）；
   * always:true 表示 24 小时。供 T2-1 生活设施系统直接调用。 */
  var FACILITIES = {
    school:       { label: '学校',    open: 7,    close: 18 },
    kindergarten: { label: '幼儿园',  open: 7.5,  close: 17 },
    shop:         { label: '商铺',    open: 9,    close: 22 },
    supermarket:  { label: '超市',    open: 8,    close: 22 },
    convenience:  { label: '便利店',  always: true },
    market:       { label: '菜市场',  open: 5.5,  close: 13 },
    restaurant:   { label: '餐厅',    open: 10,   close: 21.5 },
    cafe:         { label: '咖啡店',  open: 8,    close: 23 },
    bar:          { label: '酒吧',    open: 18,   close: 2 },
    nightclub:    { label: '夜店',    open: 20,   close: 4 },
    ktv:          { label: 'KTV',     open: 14,   close: 2 },
    cinema:       { label: '电影院',  open: 10,   close: 1 },
    gym:          { label: '健身房',  open: 6,    close: 23 },
    hospital:     { label: '医院',    always: true },
    pharmacy:     { label: '药店',    open: 8,    close: 22 },
    clinic:       { label: '诊所',    open: 8,    close: 20 },
    bank:         { label: '银行',    open: 9,    close: 17 },
    post:         { label: '邮局',    open: 9,    close: 17 },
    library:      { label: '图书馆',  open: 9,    close: 21 },
    park:         { label: '公园',    open: 5,    close: 23 },
    office:       { label: '写字楼',  open: 9,    close: 18 },
    factory:      { label: '工厂',    open: 8,    close: 20 },
    police:       { label: '派出所',  always: true },
    hotel:        { label: '酒店',    always: true },
    subway:       { label: '地铁站',  open: 6,    close: 23 },
    bus:          { label: '公交站',  open: 5.5,  close: 23.5 },
    station:      { label: '火车站',  always: true },
    airport:      { label: '机场',    always: true },
    port:         { label: '码头',    always: true },
    gas:          { label: '加油站',  always: true },
    atm:          { label: 'ATM',     always: true }
  };

  /**
   * 该设施在此钟点是否营业。
   * @param {string} type 设施类型（见 FACILITIES）
   * @param {number} hour 当前钟点，可带小数；缺省用本地时钟
   * @returns {boolean} 未知类型默认 false（宁缺勿滥，避免误开门）
   */
  function isFacilityOpen(type, hour) {
    var f = FACILITIES[type];
    if (!f) return false;
    if (f.always) return true;
    var h = (hour == null) ? clockHour() : normHour(hour);
    if (f.close > f.open) return h >= f.open && h < f.close;   // 同日营业
    return h >= f.open || h < f.close;                          // 跨夜营业
  }

  /** 设施信息（含 "07:00-18:00" 文案），供 UI 展示 */
  function facilityInfo(type) {
    var f = FACILITIES[type];
    if (!f) return null;
    var hours = f.always ? '24 小时' : (fmtHour(f.open) + '-' + (f.close > f.open ? fmtHour(f.close) : fmtHour(f.close) + '(次日)'));
    return { type: type, label: f.label, always: !!f.always, open: f.open, close: f.close, hours: hours };
  }

  /** 当前时刻全部营业/打烊的设施列表 */
  function facilitiesAt(hour) {
    var h = (hour == null) ? clockHour() : normHour(hour);
    var open = [], closed = [];
    Object.keys(FACILITIES).forEach(function (k) {
      (isFacilityOpen(k, h) ? open : closed).push(facilityInfo(k));
    });
    return { hour: h, open: open, closed: closed };
  }

  // ─────────────────────────── 5. 本地时钟推进 ───────────────────────────

  var _api = API_BASE;
  var _loc = { lat: 23.1291, lng: 113.2644, city: '' };  // 默认广州（与本项目演示城市 440100 一致）
  var _ms = Date.now();          // 逻辑时钟（毫秒时间戳），以服务端为基准
  var _baseMs = _ms;             // 最近一次基准
  var _baseReal = Date.now();    // 基准时刻的本机时间
  var _lastReal = Date.now();
  var _timer = null, _syncTimer = null, _running = false;
  var _synced = false, _source = 'local-device';   // server | local-device | manual
  var _driftMs = 0;
  var _handlers = { tick: [], minute: [], hour: [], period: [], sync: [], dayphase: [] };
  var _lastPeriod = null, _lastHour = null;

  function emit(type, payload) {
    var list = _handlers[type]; if (!list) return;
    for (var i = 0; i < list.length; i++) { try { list[i](payload); } catch (e) { /* 单个回调出错不影响时钟 */ } }
  }
  function on(type, fn) { if (_handlers[type] && typeof fn === 'function') _handlers[type].push(fn); return fn; }
  function off(type, fn) {
    var l = _handlers[type]; if (!l) return;
    var i = l.indexOf(fn); if (i >= 0) l.splice(i, 1);
  }

  function _d() { return new Date(_ms); }

  /** 当前钟点（小数小时，逻辑时钟） */
  function clockHour() { var d = _d(); return d.getHours() + d.getMinutes() / 60 + d.getSeconds() / 3600; }

  /**
   * 用一个毫秒时间戳重置逻辑时钟（不改变推进方式）
   * @param {number} ms 时间戳
   * @param {string} [source] 来源标记
   */
  function setTime(ms, source) {
    _ms = Number(ms) || Date.now();
    _baseMs = _ms; _baseReal = Date.now(); _lastReal = Date.now();
    _driftMs = 0;
    if (source) _source = source;
    _emitIfChanged(true);
    return state();
  }

  /** 直接把时钟跳到某个钟点（今天的该时刻），演示用 */
  function setHour(hour, minute) {
    var d = new Date();
    var h = Math.floor(normHour(hour));
    d.setHours(h, Math.floor(minute || (normHour(hour) % 1) * 60), 0, 0);
    return setTime(d.getTime(), 'manual');
  }

  function _emitIfChanged(force) {
    var h = clockHour(), p = periodOf(h), ih = Math.floor(h);
    if (force || ih !== _lastHour) { _lastHour = ih; emit('hour', { hour: ih, clock: state() }); }
    if (force || p !== _lastPeriod) {
      var prev = _lastPeriod; _lastPeriod = p;
      emit('period', { period: p, label: periodLabel(p), prev: prev, clock: state() });
    }
  }

  /** 每秒自增一次；漂移超过 2 秒时按 ±1 秒/次缓慢纠正 */
  function _tick() {
    _ms += 1000;                                   // ← 本地自增，不依赖轮询
    var real = Date.now();
    _lastReal = real;
    var expect = _baseMs + (real - _baseReal);
    _driftMs = _ms - expect;
    if (_lastPeriod === null) { _lastPeriod = periodOf(clockHour()); }
    if (Math.abs(_driftMs) > 2000) { _ms += _driftMs > 0 ? -1000 : 1000; }  // 平滑收敛
    var d = _d();
    emit('tick', { ms: _ms, clock: state(false) });
    if (d.getSeconds() === 0) emit('minute', { minute: d.getMinutes(), clock: state() });
    _emitIfChanged(false);
  }

  /** 与 /api/time 校正一次（默认每 30 分钟自动调用） */
  function sync() {
    if (typeof fetch !== 'function') return Promise.resolve(state());
    return fetch(_api + '/api/time', { cache: 'no-store' })
      .then(function (r) { return r.json(); })
      .then(function (d) {
        if (d && d.timestamp) {
          var before = _driftMs;
          setTime(d.timestamp * 1000, 'server');
          _synced = true;
          d.__driftBefore = before;
          emit('sync', d);
        }
        return state();
      })
      .catch(function () { return state(); });     // 服务不可用时继续用本地推进
  }

  /** 取定位（可选）：/api/location → 用于日出日落纬度 */
  function syncLocation() {
    if (typeof fetch !== 'function') return Promise.resolve(_loc);
    return fetch(_api + '/api/location', { cache: 'no-store' })
      .then(function (r) { return r.json(); })
      .then(function (d) {
        if (d && d.lat != null && d.lng != null) {
          _loc.lat = Number(d.lat); _loc.lng = Number(d.lng);
          _loc.city = (d.leaf && d.leaf.name) || d.area || '';
        }
        return _loc;
      })
      .catch(function () { return _loc; });
  }

  /**
   * 启动时钟：先同步一次服务端时间，然后每秒自增；每 30 分钟再校正一次。
   * @param {object} [opts] {api, syncNow, syncIntervalMs, location}
   */
  function start(opts) {
    opts = opts || {};
    if (opts.api) _api = opts.api;
    if (_running) return state();
    _running = true;
    _lastPeriod = periodOf(clockHour());
    if (opts.location) syncLocation();
    if (opts.syncNow !== false) sync();
    _timer = setInterval(_tick, 1000);
    _syncTimer = setInterval(sync, opts.syncIntervalMs || 30 * 60 * 1000);   // 30 分钟校正
    return state();
  }
  function stop() {
    _running = false;
    if (_timer) clearInterval(_timer); _timer = null;
    if (_syncTimer) clearInterval(_syncTimer); _syncTimer = null;
    return state();
  }
  function isRunning() { return _running; }

  /** 完整时间状态快照（供 UI / 其他系统消费） */
  function state(full) {
    var d = _d();
    var h = clockHour(), p = periodOf(h);
    var out = {
      ms: _ms,
      timestamp: Math.floor(_ms / 1000),
      iso: d.toISOString(),
      date: pad(d.getFullYear()) + '-' + pad(d.getMonth() + 1) + '-' + pad(d.getDate()),
      time: pad(d.getHours()) + ':' + pad(d.getMinutes()) + ':' + pad(d.getSeconds()),
      hour: d.getHours(), minute: d.getMinutes(), second: d.getSeconds(),
      clockHour: h,                       // 带小数的小时，光照/插值用它
      period: p, periodLabel: periodLabel(p), periodIcon: periodIcon(p),
      isDay: isDay(h),
      weekday: d.getDay(),                // 0=周日（JS 口径）
      weekdayName: ['周日', '周一', '周二', '周三', '周四', '周五', '周六'][d.getDay()],
      tint: timeTint(h),
      synced: _synced, source: _source, driftMs: _driftMs, running: _running
    };
    if (full !== false) {
      out.location = { lat: _loc.lat, lng: _loc.lng, city: _loc.city };
      out.sun = sunTimes(d, _loc.lat, _loc.lng);
      out.sun.sunriseText = fmtHour(out.sun.sunrise);
      out.sun.sunsetText = fmtHour(out.sun.sunset);
      out.sun.daylightText = Math.floor(out.sun.daylight) + 'h' + Math.round((out.sun.daylight % 1) * 60) + 'm';
      out.sunProgress = sunProgress(h, out.sun);
    }
    return out;
  }

  /** 让 state() 里的 sun 直接用当前日期与位置（缺参数时） */
  function sun(opts) {
    opts = opts || {};
    return sunTimes(opts.date || new Date(), opts.lat == null ? _loc.lat : opts.lat,
      opts.lng == null ? _loc.lng : opts.lng, opts.tz);
  }

  return {
    // 时段 / 状态
    PERIOD_ORDER: PERIOD_ORDER, PERIOD_LABEL: PERIOD_LABEL,
    periodOf: periodOf, periodLabel: periodLabel, periodIcon: periodIcon, isDay: isDay,
    state: state, clockHour: clockHour, normHour: normHour,
    // 光照
    timeTint: timeTint, tintCss: tintCss, applyTimeTint: applyTimeTint,
    // 日出日落
    sunTimes: sunTimes, sun: sun, sunProgress: sunProgress, fmtHour: fmtHour,
    solarDeclination: solarDeclination, dayOfYear: dayOfYear,
    // 设施
    FACILITIES: FACILITIES, isFacilityOpen: isFacilityOpen,
    facilityInfo: facilityInfo, facilitiesAt: facilitiesAt,
    // 时钟
    start: start, stop: stop, isRunning: isRunning, sync: sync, syncLocation: syncLocation,
    setTime: setTime, setHour: setHour, now: function () { return _ms; },
    on: on, off: off,
    setLocation: function (o) { if (o) { if (o.lat != null) _loc.lat = o.lat; if (o.lng != null) _loc.lng = o.lng; if (o.city) _loc.city = o.city; } return _loc; },
    getLocation: function () { return { lat: _loc.lat, lng: _loc.lng, city: _loc.city }; },
    API_BASE: API_BASE
  };
});
