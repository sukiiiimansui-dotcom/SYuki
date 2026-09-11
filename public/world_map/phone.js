/* phone.js — L-SYuki 悬浮手机 UI (T5-1 ~ T5-8)
 * 纯前端、零依赖、无框架；浏览器与 Node 双端可用（Node 下 require 可直接跑 phone_selftest.js）。
 *
 * 定位：地图上的「悬浮手机」——地图界面角落有一个可拖动/可收起的手机按钮，
 *       点开弹出手机界面；手机既可作半透明叠层浮在对话/地图之上，也可缩成角落小窗。
 *
 * 八个功能（与任务 T5-1~T5-8 一一对应）：
 *   T5-1 悬浮入口     PHONE.mount()           悬浮按钮 + 拖动 + 收起小圆点 + 半透明层/角落小窗双形态
 *   T5-2 地图/导航    renderMapScreen()       当前位置(/api/location) → 目的地 → /api/transport_plan 方案 + 路线图
 *   T5-3 打车         createRide()/rideStep() 叫车状态机（叫车→派单→接单→车辆接近→行程中→完成）
 *   T5-4 公交/地铁    nearbyTransit()         附近站点（/api/facilities 交通节点）+ 公交地铁方案筛选
 *   T5-5 通讯         setContacts()           消息列表 / 会话 / 电话（通话计时）
 *   T5-6 日程待办     setSchedule()           勾选完成 / 新增待办（内存态）
 *   T5-7 天气         setWeather()/normalize  /api/weather → 温度/体感/湿度/风/描述 + 图标
 *   T5-8 音乐         setTracks()             播放器 UI（播放/暂停/上下曲/进度，不播真实音频）
 *
 * 与既有模块的关系（均为只读复用，不修改）：
 *   transport.js   交通工具常量表 + 方案列表 HTML（routeOptionsHTML/modeChipsHTML/stepsHTML）+ 路线绘制
 *   world_coord.js 统一坐标（地理↔世界↔画布），手机小地图用它做投影
 *   facilities.py  /api/facilities 的交通节点（bus_stop/subway/train_station/…）
 *   hier_api.py    /api/location /api/time /api/weather /api/transport_plan
 *
 * 触摸约定：所有可点元素 ≥44px 热区；拖动用 pointer 事件 + 8px 阈值区分「点」与「拖」；
 *          手机本体用 touch-action:none 只给把手，内容区正常滚动，避免误触。
 * 性能约定：PNG 优先（本模块自身不产图片，地图/设施图一律走服务端 PNG 接口）；
 *          不引入大 JS 包；渲染按「屏幕签名」跳过无变化的 DOM 重建，动画只改内联样式。
 */
(function (root, factory) {
  var api = factory();
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  if (root) root.PHONE = api;
})(typeof globalThis !== 'undefined' ? globalThis : this, function () {
  'use strict';

  var DEFAULT_API = 'http://127.0.0.1:8790';
  var R = Math.PI / 180;
  var METERS_PER_DEG_LAT = 110574;
  var METERS_PER_DEG_LNG_EQ = 111320;

  // ══════════════════════════════════════════════════════════════════════
  // 0. 通用工具
  // ══════════════════════════════════════════════════════════════════════

  function isFn(v) { return typeof v === 'function'; }
  function num(v, d) { v = Number(v); return isFinite(v) ? v : (d === undefined ? 0 : d); }
  function clamp(v, a, b) { return v < a ? a : (v > b ? b : v); }
  function pad2(n) { n = Math.floor(Math.abs(num(n))); return (n < 10 ? '0' : '') + n; }

  /** HTML 转义（所有来自接口/外部 set* 接口的文本都必须过这一层） */
  function esc(s) {
    return String(s === null || s === undefined ? '' : s)
      .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;').replace(/'/g, '&#39;');
  }

  function deepCopy(v) {
    if (Array.isArray(v)) return v.map(deepCopy);
    if (v && typeof v === 'object') {
      var o = {}, k;
      for (k in v) if (Object.prototype.hasOwnProperty.call(v, k)) o[k] = deepCopy(v[k]);
      return o;
    }
    return v;
  }

  /** 合并：把后续每个 patch 的字段覆盖到 target 上（可变参数，返回 target 本身） */
  function assign(target) {
    target = target || {};
    for (var i = 1; i < arguments.length; i++) {
      var patch = arguments[i];
      if (!patch) continue;
      for (var k in patch) if (Object.prototype.hasOwnProperty.call(patch, k)) target[k] = patch[k];
    }
    return target;
  }

  /** 两点球面距离（米）——与 transport.py 的 haversine 同口径 */
  function haversineM(a, b) {
    if (!a || !b) return 0;
    var Rm = 6371008.8;
    var dLat = (num(b[1]) - num(a[1])) * R;
    var dLng = (num(b[0]) - num(a[0])) * R;
    var la1 = num(a[1]) * R, la2 = num(b[1]) * R;
    var h = Math.sin(dLat / 2) * Math.sin(dLat / 2) +
            Math.cos(la1) * Math.cos(la2) * Math.sin(dLng / 2) * Math.sin(dLng / 2);
    return 2 * Rm * Math.asin(Math.min(1, Math.sqrt(h)));
  }
  function fmtDistance(m) {
    m = num(m);
    if (m < 1000) return Math.round(m) + '米';
    return (m / 1000).toFixed(m < 10000 ? 1 : 0) + '公里';
  }
  function fmtMinutes(min) {
    var m = Math.round(num(min));
    if (m < 60) return m + '分钟';
    var h = Math.floor(m / 60), r = m % 60;
    return r ? h + '小时' + r + '分' : h + '小时';
  }
  /** 秒 → 00:00 / 1:02:03（通话音/音乐进度共用） */
  function fmtClock(sec) {
    var s = Math.max(0, Math.floor(num(sec)));
    var h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), ss = s % 60;
    return (h ? h + ':' + pad2(m) : String(m)) + ':' + pad2(ss);
  }
  function fmtCost(c) {
    c = num(c);
    if (c <= 0) return '免费';
    return c >= 10 ? '¥' + Math.round(c) : '¥' + c.toFixed(1);
  }
  function pct(v) { return clamp(Math.round(num(v) * 100), 0, 100); }

  // ══════════════════════════════════════════════════════════════════════
  // 1. 状态栏（T5-1）：时间 / 电量 / 信号
  // ══════════════════════════════════════════════════════════════════════

  /** 把 /api/time 的 payload 或本地时钟统一成状态栏要的字段；纯函数可测 */
  function makeStatus(opt) {
    opt = opt || {};
    var d = opt.date instanceof Date ? opt.date : new Date();
    var src = opt.time || {};
    var time = src.time || (pad2(d.getHours()) + ':' + pad2(d.getMinutes()) + ':' + pad2(d.getSeconds()));
    var parts = String(time).split(':');
    var hhmm = parts[0] + ':' + parts[1];
    var hour = src.hour !== undefined && src.hour !== null ? num(src.hour) : d.getHours();
    var period = src.period || periodOf(hour);
    return {
      hhmm: hhmm,
      time: time,
      date: src.date || (d.getFullYear() + '-' + pad2(d.getMonth() + 1) + '-' + pad2(d.getDate())),
      weekday_name: src.weekday_name || '',
      hour: hour,
      minute: src.minute !== undefined && src.minute !== null ? num(src.minute) : d.getMinutes(),
      period: period,
      periodLabel: PERIOD_ZH[period] || PERIOD_ZH.night,
      is_day: src.is_day !== undefined && src.is_day !== null ? !!src.is_day : (hour >= 6 && hour < 18),
      timestamp: src.timestamp || Math.floor(d.getTime() / 1000)
    };
  }
  var PERIOD_ZH = { dawn: '黎明', morning: '早晨', noon: '中午', afternoon: '下午', dusk: '黄昏', evening: '夜晚', night: '深夜' };
  /** 与 hier_api.time_payload() 完全一致的时段判定（仅作离线兜底） */
  function periodOf(h) {
    h = num(h) % 24;
    if (h >= 5 && h < 8) return 'dawn';
    if (h >= 8 && h < 11) return 'morning';
    if (h >= 11 && h < 14) return 'noon';
    if (h >= 14 && h < 17) return 'afternoon';
    if (h >= 17 && h < 19) return 'dusk';
    if (h >= 19 && h < 23) return 'evening';
    return 'night';
  }

  /** 电量：优先浏览器 Battery API，缺失时用配置的模拟值 */
  function batteryInfo(level, charging) {
    var lv = clamp(Math.round(num(level, 82)), 0, 100);
    return {
      level: lv, text: lv + '%', charging: !!charging,
      icon: lv >= 90 ? '🔋' : lv >= 60 ? '🔋' : lv >= 30 ? '🔋' : lv >= 12 ? '🪫' : '🪫',
      color: charging ? '#7dffb0' : lv >= 30 ? '#eaf3ff' : lv >= 15 ? '#ffd166' : '#ff7b7b',
      low: lv <= 15 && !charging
    };
  }

  /** 信号：优先 navigator.connection.effectiveType，缺失用 bars 兜底 */
  function signalInfo(bars, type) {
    var b = clamp(Math.round(num(bars, 4)), 0, 4);
    if (type) {
      b = ({ 'slow-2g': 1, '2g': 2, '3g': 3, '4g': 4, '5g': 4 })[String(type)] || b;
    }
    var label = ['无服务', '2G', '3G', '4G', '5G'][b];
    return { bars: b, label: label, color: b <= 1 ? '#ff9b9b' : '#eaf3ff' };
  }

  // ══════════════════════════════════════════════════════════════════════
  // 2. 默认数据（LingChat 未接入时的示例数据 / 也供自检使用）
  // ══════════════════════════════════════════════════════════════════════

  var DEMO_CONTACTS = [
    {
      id: 'sunxi', name: '孙曦', avatar: '🧑‍🚀', role: '学妹 · LingChat',
      online: true, unread: 2, pinned: true,
      messages: [
        { from: 'them', text: '学长，今晚的观测窗口定在 21:40 吗？', t: '20:12' },
        { from: 'them', text: '我把星图导出来了，等下发你。', t: '20:13' }
      ]
    },
    {
      id: 'lin', name: '林澈', avatar: '🧑‍💻', role: '搭档 · 情报',
      online: true, unread: 0,
      messages: [
        { from: 'me', text: '新的台风路径查到了吗？', t: '19:02' },
        { from: 'them', text: '查到了，明天下午擦过沿海，带伞。', t: '19:04' }
      ]
    },
    {
      id: 'yuki', name: 'Yuki', avatar: '🌸', role: '助手 · 常驻',
      online: true, unread: 1,
      messages: [
        { from: 'them', text: '日程帮你排好了，看看天气再定出门时间。', t: '18:30' }
      ]
    },
    {
      id: 'group', name: '观测小组', avatar: '🔭', role: '群聊 · 6 人',
      online: false, unread: 0,
      messages: [
        { from: 'them', text: '今晚云量 93%，改室内复盘。', t: '17:45' },
        { from: 'me', text: '收到，我带硬盘过来。', t: '17:47' }
      ]
    }
  ];

  var DEMO_SCHEDULE = [
    { id: 's1', time: '09:00', title: '晨会 · 地图层进度同步', detail: '汇报手机 UI 与设施层联调', tag: '工作', done: true },
    { id: 's2', time: '12:30', title: '午饭 + 取快递', detail: '菜鸟驿站 3 号柜', tag: '生活', done: false },
    { id: 's3', time: '15:00', title: '路线规划回归测试', detail: '9 种交通工具全跑一遍', tag: '工作', done: false },
    { id: 's4', time: '21:40', title: '夜间观测窗口', detail: '带星图与保温杯', tag: '兴趣', done: false }
  ];

  var DEMO_TRACKS = [
    { id: 't1', title: '浮游都市', artist: 'L-SYuki', album: 'World Map OST', duration: 214, cover: '🏙️' },
    { id: 't2', title: '夜航西飞', artist: '孙曦', album: '观测日志', duration: 258, cover: '🌙' },
    { id: 't3', title: '玻璃拟态', artist: 'Yuki', album: 'UI Sessions', duration: 187, cover: '🪟' },
    { id: 't4', title: '台风过境', artist: '林澈', album: '气象台', duration: 301, cover: '🌀' }
  ];

  /** 附近站点兜底（无网/接口失败时用；坐标取广州市中心一带） */
  var DEMO_STOPS = [
    { id: 'd1', name: '公园前站', type: 'subway', icon: '🚇', lat: 23.1292, lng: 113.2640, lines: ['1号线', '2号线'] },
    { id: 'd2', name: '北京路口站', type: 'bus_stop', icon: '🚌', lat: 23.1256, lng: 113.2668, lines: ['3路', '10路'] },
    { id: 'd3', name: '中山五路站', type: 'bus_stop', icon: '🚌', lat: 23.1275, lng: 113.2620, lines: ['B8路'] },
    { id: 'd4', name: '广州火车站', type: 'train_station', icon: '🚄', lat: 23.1510, lng: 113.2580, lines: ['2号线', '5号线'] }
  ];

  /** 常用目的地（手机「地图/打车」两屏共用；可由 setPlaces 覆盖） */
  var DEMO_PLACES = [
    { id: 'p1', name: '广州塔', lat: 23.1065, lng: 113.3240 },
    { id: 'p2', name: '天河体育中心', lat: 23.1374, lng: 113.3244 },
    { id: 'p3', name: '广州南站', lat: 22.9890, lng: 113.2690 },
    { id: 'p4', name: '白云机场', lat: 23.3924, lng: 113.2988 }
  ];

  // ══════════════════════════════════════════════════════════════════════
  // 3. 天气（T5-7）：/api/weather → 手机卡片
  // ══════════════════════════════════════════════════════════════════════

  var WEATHER_ICON = {
    clear: '☀️', partly: '🌤️', cloudy: '⛅', overcast: '☁️',
    rain: '🌧️', thunder: '⛈️', snow: '❄️', fog: '🌫️', haze: '😷', unknown: '❔'
  };
  var WEATHER_LABEL = {
    clear: '晴', partly: '局部多云', cloudy: '多云', overcast: '阴',
    rain: '雨', thunder: '雷雨', snow: '雪', fog: '雾', haze: '霾', unknown: '未知'
  };

  /** 与 world_weather.js 的 classify 同口径（仅判定类型，不画粒子） */
  function weatherKind(d) {
    d = d || {};
    var en = String(d.desc_en || '').toLowerCase();
    var zh = String(d.desc || '');
    var cc = num(d.cloudcover, 0);
    if (d.error) return 'unknown';
    if (d.is_thunder || /thunder|雷/.test(en) || zh.indexOf('雷') >= 0) return 'thunder';
    if (d.is_snow || /snow|blizzard|sleet/.test(en) || zh.indexOf('雪') >= 0) return 'snow';
    if (d.is_rain || /rain|drizzle|shower/.test(en) || /雨/.test(zh)) return 'rain';
    if (d.is_fog || /fog|mist/.test(en) || /雾/.test(zh)) return 'fog';
    if (/haze|smoke|sand/.test(en) || /霾|沙尘/.test(zh)) return 'haze';
    if (/overcast/.test(en) || zh.indexOf('阴') >= 0 || cc >= 88) return 'overcast';
    if (/partly|mostly sunny|scattered|few cloud/.test(en) || zh.indexOf('局部') >= 0) return 'partly';
    if (/cloud/.test(en) || zh.indexOf('多云') >= 0 || cc >= 55) return 'cloudy';
    if (/sunny|clear/.test(en) || zh.indexOf('晴') >= 0) return 'clear';
    if (cc >= 25) return 'partly';
    return cc < 25 ? 'clear' : 'cloudy';
  }

  /** /api/weather 原始 payload → 手机天气状态（纯函数，离线可用） */
  function normalizeWeather(raw) {
    raw = raw || {};
    var kind = weatherKind(raw);
    var wind = num(raw.wind_kmph, 0);
    var windLevel = wind < 1 ? 0 : wind < 6 ? 1 : wind < 12 ? 2 : wind < 20 ? 3 :
                    wind < 29 ? 4 : wind < 39 ? 5 : wind < 50 ? 6 : 7;
    return {
      ok: !raw.error,
      error: raw.error || null,
      city: raw.city || '',
      kind: kind,
      kindLabel: WEATHER_LABEL[kind],
      icon: WEATHER_ICON[kind],
      desc: raw.desc || WEATHER_LABEL[kind],
      desc_en: raw.desc_en || '',
      temp_c: raw.temp_c === undefined || raw.temp_c === null ? null : num(raw.temp_c),
      feels_like_c: raw.feels_like_c === undefined || raw.feels_like_c === null ? null : num(raw.feels_like_c),
      humidity: raw.humidity === undefined || raw.humidity === null ? null : num(raw.humidity),
      wind_kmph: wind,
      wind_level: windLevel,
      wind_text: windLevel ? windLevel + '级' : '无风',
      cloudcover: num(raw.cloudcover, 0),
      precip_mm: num(raw.precip_mm, 0),
      visibility_km: raw.visibility_km === undefined || raw.visibility_km === null ? null : num(raw.visibility_km),
      is_rain: kind === 'rain' || kind === 'thunder',
      is_snow: kind === 'snow',
      cached: !!raw.cached,
      fetchedAt: raw.fetchedAt || 0
    };
  }
  function tempText(w) {
    w = w || {};
    return w.temp_c === null || w.temp_c === undefined ? '--' : Math.round(w.temp_c) + '°';
  }

  // ══════════════════════════════════════════════════════════════════════
  // 4. 打车状态机（T5-3）
  // ══════════════════════════════════════════════════════════════════════

  /** 状态流转表（只允许按箭头走，非法流转一律忽略并返回原因） */
  var RIDE_ORDER = ['idle', 'matching', 'accepted', 'arriving', 'onboard', 'done', 'canceled'];
  // 演示加速倍率：每 1 秒真实时间推进 20 秒行程时间（叫车→上车≈12 次 tick，行程≈30 次 tick）
  var RIDE_SPEEDUP = 20;
  var RIDE_NEXT = {
    idle: ['matching', 'canceled'],
    matching: ['accepted', 'canceled'],
    accepted: ['arriving', 'canceled'],
    arriving: ['onboard', 'canceled'],
    onboard: ['done', 'canceled'],
    done: ['idle'],
    canceled: ['idle']
  };
  var RIDE_LABEL = {
    idle: '待叫车', matching: '正在派单', accepted: '司机已接单',
    arriving: '车辆接近中', onboard: '行程中', done: '已到达', canceled: '已取消'
  };
  var RIDE_HINT = {
    idle: '选择目的地后点击「一键叫车」',
    matching: '正在为你寻找附近车辆…',
    accepted: '司机正在赶往上车点，请到路边等候',
    arriving: '车辆即将到达，注意车牌',
    onboard: '行程进行中，请系好安全带',
    done: '行程结束，欢迎再次使用',
    canceled: '本次叫车已取消'
  };
  var DEMO_DRIVERS = [
    { name: '陈师傅', car: '粤A·8F2K9', color: '白色 · 埃安 S', rating: 4.9, plate_emoji: '🚕' },
    { name: '李师傅', car: '粤A·3T7Q1', color: '银色 · 比亚迪 秦', rating: 4.8, plate_emoji: '🚖' },
    { name: '王师傅', car: '粤A·6M4Z8', color: '黑色 · 传祺', rating: 5.0, plate_emoji: '🚘' }
  ];

  /**
   * 新建一个打车状态机实例（纯数据，不碰 DOM，可在 Node 里跑自检）
   * @param {object} o {from:{lat,lng,name}, to:{lat,lng,name}, mode:'taxi', totalMin, seed}
   */
  function createRide(o) {
    o = o || {};
    var from = o.from || null, to = o.to || null;
    var total = Math.max(3, Math.round(num(o.totalMin, 18)));
    return {
      status: 'idle',
      from: from, to: to,
      fromName: (from && from.name) || '当前位置',
      toName: (to && to.name) || '',
      totalMin: total,
      etaMin: 0,            // 剩余分钟（司机接近 / 行程剩余共用）
      etaMaxMin: total,
      progress: 0,          // 0..1 车辆位置进度
      driver: null,
      price: num(o.price, 0),
      priceText: fmtCost(o.price),
      distance_m: num(o.distance_m, 0),
      history: [],          // [{from,to,at}] 便于自检与调试
      seq: 0
    };
  }

  /** 状态唯一入口：非法流转被拒绝并返回 {ok:false, reason} */
  function rideTransition(ride, next, payload) {
    if (!ride) return { ok: false, reason: 'no-ride' };
    var allow = RIDE_NEXT[ride.status] || [];
    if (allow.indexOf(next) < 0) {
      return { ok: false, reason: 'illegal:' + ride.status + '→' + next, status: ride.status };
    }
    var prev = ride.status;
    ride.status = next;
    ride.seq++;
    payload = payload || {};
    if (next === 'matching') {
      ride.etaMin = 4;
      ride.progress = 0;
    }
    if (next === 'accepted') {
      ride.driver = payload.driver || DEMO_DRIVERS[(ride.seq + Math.floor(num(payload.seed, 0))) % DEMO_DRIVERS.length];
      ride.etaMin = Math.max(1, Math.round(num(payload.etaMin, 4)));
      ride.etaMaxMin = ride.etaMin;
      ride.progress = 0;
    }
    if (next === 'onboard') {
      ride.etaMin = ride.totalMin;
      ride.etaMaxMin = ride.totalMin;
      ride.progress = 0;
    }
    if (next === 'done') {
      ride.etaMin = 0; ride.progress = 1;
    }
    if (next === 'canceled') {
      ride.etaMin = 0;
    }
    if (next === 'idle') {
      ride.driver = null; ride.etaMin = 0; ride.progress = 0; ride.history = [];
    }
    ride.history.push({ from: prev, to: next, at: ride.seq });
    return { ok: true, status: ride.status, prev: prev };
  }

  /**
   * 推进一秒（模拟时钟）。返回 {changed:boolean, status, event}
   * event: null | 'accepted' | 'onboard' | 'done'（用于 UI 弹提示）
   */
  function rideStep(ride, dtSec) {
    if (!ride) return { changed: false, status: '', event: null };
    var dt = Math.max(1, Math.round(num(dtSec, 1)));
    var st = ride.status, event = null;

    if (st === 'matching') {
      // 派单 3~5 秒后必有司机接单（确定性的，便于演示与自检）
      ride.seq++;
      if (ride.seq % 4 === 0) {
        rideTransition(ride, 'accepted', { etaMin: 4, seed: 1 });
        event = 'accepted';
      }
      return { changed: true, status: ride.status, event: event };
    }

    if (st === 'accepted' || st === 'arriving') {
      if (st === 'accepted') rideTransition(ride, 'arriving');
      ride.etaMin -= dt * RIDE_SPEEDUP / 60;    // 演示加速：1 秒 ≈ RIDE_SPEEDUP 秒剧情时间
      if (ride.etaMaxMin > 0) ride.progress = clamp(1 - ride.etaMin / ride.etaMaxMin, 0, 1);
      if (ride.etaMin <= 0) {
        ride.etaMin = 0;
        rideTransition(ride, 'onboard');
        event = 'onboard';
      }
      return { changed: true, status: ride.status, event: event };
    }

    if (st === 'onboard') {
      ride.etaMin -= dt * RIDE_SPEEDUP / 60;
      if (ride.etaMaxMin > 0) ride.progress = clamp(1 - ride.etaMin / ride.etaMaxMin, 0, 1);
      if (ride.etaMin <= 0) {
        ride.etaMin = 0; ride.progress = 1;
        rideTransition(ride, 'done');
        event = 'done';
      }
      return { changed: true, status: ride.status, event: event };
    }

    return { changed: false, status: st, event: null };
  }

  /** 行程面板要展示的动态文案（纯函数） */
  function rideView(ride) {
    if (!ride) return null;
    var label = RIDE_LABEL[ride.status] || ride.status;
    var hint = RIDE_HINT[ride.status] || '';
    var etaText = ride.status === 'onboard' ? '剩余 ' + fmtMinutes(ride.etaMin)
      : (ride.status === 'arriving' || ride.status === 'accepted') ? '预计 ' + Math.max(1, Math.round(ride.etaMin)) + ' 分钟到达'
      : ride.status === 'matching' ? '派单中…'
      : ride.status === 'done' ? '已到达目的地' : '—';
    return {
      status: ride.status, label: label, hint: hint, etaText: etaText,
      progress: clamp(num(ride.progress), 0, 1),
      progressPct: pct(ride.progress),
      driver: ride.driver,
      driverLine: ride.driver ? (ride.driver.name + ' · ' + ride.driver.car) : '',
      carLine: ride.driver ? ride.driver.color : '',
      priceText: ride.priceText,
      canCancel: ['matching', 'accepted', 'arriving', 'onboard'].indexOf(ride.status) >= 0,
      canReset: ride.status === 'done' || ride.status === 'canceled',
      steps: RIDE_ORDER.slice(1, 6).map(function (k) {
        var idx = RIDE_ORDER.indexOf(k);
        var cur = RIDE_ORDER.indexOf(ride.status);
        return { key: k, label: RIDE_LABEL[k], state: cur > idx ? 'done' : cur === idx ? 'now' : 'todo' };
      })
    };
  }

  // ══════════════════════════════════════════════════════════════════════
  // 5. 音乐播放器（T5-8）纯状态函数
  // ══════════════════════════════════════════════════════════════════════

  function createMusic(tracks) {
    return {
      tracks: (tracks || DEMO_TRACKS).slice(),
      index: 0,
      playing: false,
      position: 0,        // 秒
      volume: 0.7,
      mode: 'list'        // list | single | shuffle
    };
  }
  function trackAt(m, i) {
    if (!m || !m.tracks || !m.tracks.length) return null;
    var idx = ((i % m.tracks.length) + m.tracks.length) % m.tracks.length;
    return m.tracks[idx];
  }
  function currentTrack(m) { return trackAt(m, m ? m.index : 0); }
  function musicToggle(m) { if (!m) return null; m.playing = !m.playing; return m.playing; }
  function musicNext(m, auto) {
    if (!m || !m.tracks.length) return null;
    if (m.mode === 'single' && auto) { m.position = 0; return currentTrack(m); }
    m.index = (m.index + 1) % m.tracks.length;
    m.position = 0;
    return currentTrack(m);
  }
  function musicPrev(m) {
    if (!m || !m.tracks.length) return null;
    if (m.position > 3) { m.position = 0; return currentTrack(m); }   // 播过 3 秒 → 回开头
    m.index = (m.index - 1 + m.tracks.length) % m.tracks.length;
    m.position = 0;
    return currentTrack(m);
  }
  /** 推进 n 秒；自动续播（返回 {changed, ended}） */
  function musicTick(m, dtSec) {
    if (!m || !m.playing) return { changed: false, ended: false };
    var t = currentTrack(m);
    if (!t) return { changed: false, ended: false };
    m.position += num(dtSec, 1) * 2;   // 演示加速 2×，进度条肉眼可见
    var ended = false;
    if (m.position >= num(t.duration, 200)) { musicNext(m, true); ended = true; }
    return { changed: true, ended: ended };
  }
  function musicProgress(m) {
    var t = currentTrack(m);
    if (!t || !num(t.duration)) return 0;
    return clamp(num(m.position) / num(t.duration), 0, 1);
  }

  // ══════════════════════════════════════════════════════════════════════
  // 6. 数据适配：联系人 / 日程 / 曲目 / 站点（LingChat 预留接口）
  // ══════════════════════════════════════════════════════════════════════

  /** LingChat 角色 → 手机联系人（字段尽量宽松，缺失即兜底） */
  function normalizeContact(c, i) {
    c = c || {};
    var msgs = (c.messages || c.msgs || c.history || []).map(function (m, j) {
      return {
        id: m.id || ('m' + i + '_' + j),
        from: (m.from === 'me' || m.self || m.mine) ? 'me' : 'them',
        text: String(m.text === undefined ? (m.content || '') : m.text),
        t: m.t || m.time || m.at || '',
        state: m.state || 'sent'
      };
    });
    return {
      id: String(c.id || c.uid || c.name || ('c' + i)),
      name: String(c.name || c.nickname || c.title || ('联系人 ' + (i + 1))),
      avatar: c.avatar || c.face || c.emoji || '👤',
      role: c.role || c.desc || c.signature || '',
      online: c.online === undefined ? !!c.active : !!c.online,
      unread: Math.max(0, Math.round(num(c.unread, 0))),
      pinned: !!c.pinned,
      messages: msgs
    };
  }
  function normalizeContacts(list) {
    if (!Array.isArray(list) || !list.length) return null;
    var out = list.map(normalizeContact).filter(function (c) { return c.name; });
    return out.length ? out : null;
  }

  /** LingChat 日程 → 手机待办 */
  function normalizeScheduleItem(s, i) {
    s = s || {};
    return {
      id: String(s.id || ('t' + i + '_' + Math.round(num(s.at, i)))),
      time: s.time || s.at_text || s.hhmm || '',
      title: String(s.title || s.text || s.name || s.content || '未命名事项'),
      detail: String(s.detail || s.desc || s.note || ''),
      tag: String(s.tag || s.type || s.category || ''),
      done: !!(s.done || s.finished || s.checked),
      priority: s.priority || ''
    };
  }
  function normalizeSchedule(list) {
    if (!Array.isArray(list) || !list.length) return null;
    var out = list.map(normalizeScheduleItem).filter(function (s) { return s.title; });
    return out.length ? out : null;
  }

  /** LingChat 网易云曲目 → 播放器曲目 */
  function normalizeTrack(t, i) {
    t = t || {};
    var dur = num(t.duration || t.dt || t.duration_ms, 0);
    if (dur > 1000) dur = Math.round(dur / 1000);          // 毫秒 → 秒
    if (!dur) dur = 200;
    return {
      id: String(t.id || t.song_id || ('k' + i)),
      title: String(t.title || t.name || ('曲目 ' + (i + 1))),
      artist: String(t.artist || t.ar || t.singer || '未知歌手'),
      album: String(t.album || t.al || ''),
      duration: dur,
      cover: t.cover || t.pic || t.emoji || '🎵',
      url: t.url || ''
    };
  }
  function normalizeTracks(list) {
    if (!Array.isArray(list) || !list.length) return null;
    var out = list.map(normalizeTrack).filter(function (t) { return t.title; });
    return out.length ? out : null;
  }

  /**
   * /api/facilities 的交通节点 → 附近站点（带距离，按近到远排序）
   * @param {object} payload {transport:[...], facilities:[...]}
   * @param {object} me {lat,lng} 当前位置；缺省则以站点集合中心为原点
   */
  function nearbyTransit(payload, me, opts) {
    opts = opts || {};
    var list = [];
    var p = payload || {};
    var combined = (p.transport || []).concat(p.facilities || []);
    var TRANS = { bus_stop: 1, subway: 1, train_station: 1, ferry: 1, parking: 0, gas: 0 };
    var i, f;
    for (i = 0; i < combined.length; i++) {
      f = combined[i] || {};
      if (f.group === 'transport' && f.type === 'parking') continue;   // 停车场不算乘车点
      if (!(f.type in TRANS)) continue;
      if (!TRANS[f.type]) continue;
      var ll = stopLatLng(f, opts);
      if (!ll) continue;
      list.push({
        id: String(f.id || ('s' + i)),
        name: String(f.name || (f.type_zh || '站点')),
        type: f.type,
        type_zh: f.type_zh || f.type,
        icon: f.icon || '🚏',
        lat: ll.lat, lng: ll.lng,
        lines: f.lines || [],
        _src: f
      });
    }
    if (!list.length && opts.fallback !== false) {
      list = DEMO_STOPS.map(function (s) { return assign({}, s, { fallback: true }); });   // 标注示例来源
    }

    var origin = me && isFinite(num(me.lat, NaN)) && isFinite(num(me.lng, NaN))
      ? { lat: num(me.lat), lng: num(me.lng) } : null;
    if (!origin) {
      var slat = 0, slng = 0, n = list.length || 1;
      list.forEach(function (s) { slat += s.lat; slng += s.lng; });
      origin = { lat: slat / n, lng: slng / n };
    }
    list.forEach(function (s) {
      s.distance_m = haversineM([origin.lng, origin.lat], [s.lng, s.lat]);
      s.distance_text = fmtDistance(s.distance_m);
      s.origin = origin;
    });
    list.sort(function (a, b) { return a.distance_m - b.distance_m; });
    var max = opts.limit || 12;
    if (list.length > max) list = list.slice(0, max);
    return list;
  }

  /** 设施网格坐标(gx,gy) → 经纬度：用小区锚点换算，缺失则用给定兜底中心 */
  function stopLatLng(f, opts) {
    opts = opts || {};
    if (isFinite(num(f.lat, NaN)) && isFinite(num(f.lng, NaN))) return { lat: num(f.lat), lng: num(f.lng) };
    var anchor = opts.anchor || (opts.origin ? { lat: num(opts.origin.lat), lng: num(opts.origin.lng) } : null);
    var cell = num(f.cell_meters, num(opts.cell_meters, 30));
    if (f.gx === undefined || f.gy === undefined) return null;
    if (!anchor) anchor = { lat: 23.1290, lng: 113.2640 };     // 兜底：广州市中心
    var dLat = (num(f.gy) * cell) / METERS_PER_DEG_LAT;
    var mLng = METERS_PER_DEG_LNG_EQ * Math.cos(anchor.lat * R) || METERS_PER_DEG_LNG_EQ;
    var dLng = (num(f.gx) * cell) / mLng;
    return { lat: anchor.lat - dLat, lng: anchor.lng + dLng };   // y 向下 = 向南
  }

  // ══════════════════════════════════════════════════════════════════════
  // 7. 小地图投影（T5-2 路线图；优先用 world_coord.js，缺失走内置墨卡托兜底）
  // ══════════════════════════════════════════════════════════════════════

  function mercator(lng, lat) {
    var s = Math.sin(clamp(num(lat), -85, 85) * R);
    return { x: (num(lng) + 180) / 360, y: 0.5 - Math.log((1 + s) / (1 - s)) / (4 * Math.PI) };
  }
  /**
   * 生成「经纬度 → 画布像素」投影器（等比 + 居中 + pad 边距）
   * @param {Array} points [[lng,lat], ...] 需要完整显示的点
   * @param {number} w,h 画布 CSS 尺寸
   * @param {number} pad 边距
   */
  function projector(points, w, h, pad) {
    pad = num(pad, 18);
    var pts = (points || []).filter(function (p) { return p && isFinite(num(p[0], NaN)) && isFinite(num(p[1], NaN)); });
    if (!pts.length) pts = [[0, 0], [0.001, 0.001]];
    var minx = Infinity, miny = Infinity, maxx = -Infinity, maxy = -Infinity;
    pts.forEach(function (p) {
      var m = mercator(p[0], p[1]);
      if (m.x < minx) minx = m.x; if (m.x > maxx) maxx = m.x;
      if (m.y < miny) miny = m.y; if (m.y > maxy) maxy = m.y;
    });
    var bw = Math.max(maxx - minx, 1e-7), bh = Math.max(maxy - miny, 1e-7);
    var aw = Math.max(1, w - 2 * pad), ah = Math.max(1, h - 2 * pad);
    var scale = Math.min(aw / bw, ah / bh);
    var ox = pad + (aw - bw * scale) / 2, oy = pad + (ah - bh * scale) / 2;
    var toXY = function (lng, lat) {
      var m = mercator(lng, lat);
      return { x: (m.x - minx) * scale + ox, y: (m.y - miny) * scale + oy };
    };
    toXY.bbox = { minx: minx, miny: miny, maxx: maxx, maxy: maxy, w: w, h: h, pad: pad };
    return toXY;
  }

  /** 收集路线 payload 里所有要显示的点（起终点 + 每段折线 + 车辆位置） */
  function routePoints(payload) {
    var out = [];
    var rt = (payload && (payload.route || payload.option)) || null;
    if (rt) {
      if (rt.from) out.push(rt.from);
      if (rt.to) out.push(rt.to);
      (rt.steps || []).forEach(function (s) {
        if (s.from) out.push(s.from);
        if (s.to) out.push(s.to);
      });
    }
    if (payload && payload.from) out.push([num(payload.from.lng), num(payload.from.lat)]);
    if (payload && payload.to) out.push([num(payload.to.lng), num(payload.to.lat)]);
    if (payload && payload.trip && payload.trip.position) out.push(payload.trip.position);
    return out;
  }

  /**
   * 在给定 canvas 上画「当前位置 → 目的地」路线（复用 transport.js 绘制，缺失则内置简版）
   * @returns {boolean} 是否真的画了
   */
  function drawMiniMap(canvas, payload, opt) {
    if (!canvas || !canvas.getContext) return false;
    opt = opt || {};
    var w = num(opt.width, 300), h = num(opt.height, 190);
    var dpr = clamp(num(opt.dpr, 1), 1, 3);
    var ctx = canvas.getContext('2d');
    if (!ctx) return false;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    if (canvas.style) { canvas.style.width = w + 'px'; canvas.style.height = h + 'px'; }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    // 底：深色玻璃拟态的地图底纹（网格 + 微光），PNG 优先原则下这里只是兜底底纹
    var g = ctx.createLinearGradient(0, 0, 0, h);
    g.addColorStop(0, 'rgba(20,34,52,0.92)');
    g.addColorStop(1, 'rgba(12,20,32,0.96)');
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, w, h);
    ctx.strokeStyle = 'rgba(121,217,255,0.10)';
    ctx.lineWidth = 1;
    for (var x = 0; x <= w; x += 24) {
      ctx.beginPath(); ctx.moveTo(x + 0.5, 0); ctx.lineTo(x + 0.5, h); ctx.stroke();
    }
    for (var y = 0; y <= h; y += 24) {
      ctx.beginPath(); ctx.moveTo(0, y + 0.5); ctx.lineTo(w, y + 0.5); ctx.stroke();
    }

    var pts = routePoints(payload);
    if (pts.length < 2) {
      ctx.fillStyle = 'rgba(190,210,240,0.6)';
      ctx.font = '12px system-ui,sans-serif';
      ctx.textAlign = 'center';
      ctx.fillText('选择目的地后显示路线', w / 2, h / 2);
      return false;
    }
    var toXY = projector(pts, w, h, 20);
    var route = (payload && payload.route) || null;
    var TR = root_transport();

    ctx.save();
    if (TR && isFn(TR.drawRoute) && route) {
      // transport.js 自带：虚线 + 换乘点 + 中段交通工具图标
      try {
        TR.drawRoute(ctx, route, function (lng, lat) { var p = toXY(lng, lat); return { cx: p.x, cy: p.y }; }, {
          iconSize: 18, endpointRadius: 4.5, dashWidth: 3, labelFont: '10px sans-serif',
          startLabel: '起', endLabel: '终'
        });
      } catch (e) { TR = null; }
    }
    if (!TR || !isFn(TR.drawRoute)) {
      // 兜底：自己画一条折线（先过滤掉缺失的端点，避免脏数据导致崩溃）
      var line = [];
      if (route) {
        if (route.from) line.push(route.from);
        (route.steps || []).forEach(function (s) { if (s && s.to) line.push(s.to); });
      } else { line = pts; }
      line = line.filter(function (p) { return p && isFinite(num(p[0], NaN)) && isFinite(num(p[1], NaN)); });
      if (line.length >= 2) {
        ctx.save();
        ctx.setLineDash([7, 6]); ctx.lineWidth = 3; ctx.lineJoin = 'round';
        ctx.strokeStyle = (route && route.color) || '#79d9ff';
        ctx.beginPath();
        line.forEach(function (p, i) {
          var q = toXY(p[0], p[1]);
          if (!i) ctx.moveTo(q.x, q.y); else ctx.lineTo(q.x, q.y);
        });
        ctx.stroke(); ctx.restore();
        var a = toXY(line[0][0], line[0][1]);
        var b = toXY(line[line.length - 1][0], line[line.length - 1][1]);
        pin(ctx, a.x, a.y, '#ffffff', '#5aa9e6', '起');
        pin(ctx, b.x, b.y, '#79d9ff', '#ffffff', '终');
      }
    }

    // 打车状态机里的车辆位置（T5-3 与 T5-2 共享一块画布）
    if (payload && payload.trip && payload.trip.position) {
      var cp = toXY(payload.trip.position[0], payload.trip.position[1]);
      carDot(ctx, cp.x, cp.y, num(opt.pulse, 0));
    }
    ctx.restore();
    return true;
  }

  function pin(ctx, x, y, fill, stroke, label) {
    ctx.save();
    ctx.beginPath();
    ctx.arc(x, y, 6, 0, Math.PI * 2);
    ctx.fillStyle = fill; ctx.fill();
    ctx.lineWidth = 2.5; ctx.strokeStyle = stroke; ctx.stroke();
    if (label) {
      ctx.font = '9px system-ui,sans-serif';
      ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
      ctx.fillStyle = stroke; ctx.fillText(label, x, y + 0.5);
    }
    ctx.restore();
  }
  function carDot(ctx, x, y, pulse) {
    ctx.save();
    if (pulse > 0) {
      ctx.beginPath();
      ctx.arc(x, y, 10 + pulse * 12, 0, Math.PI * 2);
      ctx.fillStyle = 'rgba(245,197,24,' + (0.35 * (1 - pulse)).toFixed(3) + ')';
      ctx.fill();
    }
    ctx.beginPath(); ctx.arc(x, y, 11, 0, Math.PI * 2);
    ctx.fillStyle = '#ffffff'; ctx.fill();
    ctx.lineWidth = 2.5; ctx.strokeStyle = '#f5c518'; ctx.stroke();
    ctx.font = '13px "Noto Color Emoji",system-ui,sans-serif';
    ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
    ctx.fillText('🚕', x, y + 0.5);
    ctx.restore();
  }
  /** 取全局 transport.js（浏览器 window / Node global），没有就返回 null */
  function root_transport() {
    if (typeof TRANSPORT !== 'undefined' && TRANSPORT) return TRANSPORT;
    if (typeof globalThis !== 'undefined' && globalThis.TRANSPORT) return globalThis.TRANSPORT;
    return null;
  }

  // ══════════════════════════════════════════════════════════════════════
  // 8. 屏幕渲染（纯字符串，便于 Node 自检）
  // ══════════════════════════════════════════════════════════════════════

  var APPS = [
    { key: 'map', label: '地图', icon: '🗺️' },
    { key: 'taxi', label: '打车', icon: '🚕' },
    { key: 'transit', label: '公交', icon: '🚇' },
    { key: 'chat', label: '通讯', icon: '💬' },
    { key: 'plan', label: '日程', icon: '🗓️' },
    { key: 'weather', label: '天气', icon: '🌤️' },
    { key: 'music', label: '音乐', icon: '🎵' }
  ];
  var APP_MAP = {};
  APPS.forEach(function (a) { APP_MAP[a.key] = a; });

  /** 状态栏 HTML（T5-1） */
  function statusBarHTML(st) {
    st = st || {};
    var t = makeStatus({ time: st.time, date: st.date });
    var bat = batteryInfo(st.battery && st.battery.level, st.battery && st.battery.charging);
    var sig = signalInfo(st.signal && st.signal.bars, st.signal && st.signal.type);
    var bars = '';
    for (var i = 1; i <= 4; i++) {
      bars += '<i class="ph-sig-bar' + (i <= sig.bars ? ' on' : '') + '" style="height:' + (4 + i * 2.5) + 'px"></i>';
    }
    return '' +
      '<div class="ph-status">' +
        '<span class="ph-status-time">' + esc(t.hhmm) + '</span>' +
        '<span class="ph-status-mid">' + esc(t.periodLabel) + '</span>' +
        '<span class="ph-status-right">' +
          '<span class="ph-sig" title="' + esc(sig.label) + '">' + bars + '</span>' +
          '<span class="ph-bat" title="' + esc(bat.text) + (bat.charging ? ' 充电中' : '') + '" style="color:' + esc(bat.color) + '">' +
            '<span class="ph-bat-body"><i style="width:' + bat.level + '%"></i></span>' +
            (bat.charging ? '<span class="ph-bat-bolt">⚡</span>' : '') +
            '<span class="ph-bat-txt">' + bat.level + '%</span>' +
          '</span>' +
        '</span>' +
      '</div>';
  }

  /** 应用头 + 底部 TAB */
  function appBarHTML(app, title, extraHTML) {
    return '' +
      '<div class="ph-appbar">' +
        '<button class="ph-icon-btn" data-act="app" data-app="home" aria-label="返回桌面">‹</button>' +
        '<span class="ph-appbar-title">' + esc(APP_MAP[app] ? APP_MAP[app].icon : '📱') + ' ' + esc(title || (APP_MAP[app] ? APP_MAP[app].label : '应用')) + '</span>' +
        '<span class="ph-appbar-extra">' + (extraHTML || '') + '</span>' +
      '</div>';
  }
  function tabBarHTML(active) {
    var h = ['<nav class="ph-tabs">'];
    APPS.forEach(function (a) {
      h.push('<button class="ph-tab' + (a.key === active ? ' on' : '') + '" data-act="app" data-app="' + a.key + '">' +
        '<span class="ph-tab-icon">' + a.icon + '</span><span class="ph-tab-label">' + esc(a.label) + '</span></button>');
    });
    h.push('</nav>');
    return h.join('');
  }
  function wrapApp(app, title, bodyHTML, extraHTML) {
    return '<div class="ph-app" data-app="' + esc(app) + '">' + appBarHTML(app, title, extraHTML) +
      '<div class="ph-body">' + bodyHTML + '</div>' + tabBarHTML(app) + '</div>';
  }

  // ── T5-1 桌面 ──
  function renderHomeScreen(st) {
    var w = st.weather || {};
    var time = makeStatus({ time: st.time, date: st.date });
    var contacts = st.contacts || [], unread = 0;
    contacts.forEach(function (c) { unread += num(c.unread, 0); });
    var todos = (st.schedule || []).filter(function (s) { return !s.done; }).length;
    var h = [];
    h.push('<div class="ph-home-head">' +
      '<div class="ph-clock">' + esc(time.hhmm) + '</div>' +
      '<div class="ph-clock-sub">' + esc(time.date) + (time.weekday_name ? ' · ' + esc(time.weekday_name) : '') +
        ' · ' + esc(time.periodLabel) + '</div>' +
      '<div class="ph-home-weather" data-act="app" data-app="weather">' +
        '<span class="ph-hw-icon">' + esc(w.icon || '❔') + '</span>' +
        '<span class="ph-hw-temp">' + esc(tempText(w)) + '</span>' +
        '<span class="ph-hw-desc">' + esc(w.city ? w.city + ' · ' + (w.desc || w.kindLabel || '') : (w.desc || w.kindLabel || '天气未加载')) + '</span>' +
      '</div></div>');

    h.push('<div class="ph-grid">');
    APPS.forEach(function (a) {
      var badge = a.key === 'chat' && unread ? String(unread) : (a.key === 'plan' && todos ? String(todos) : '');
      h.push('<button class="ph-app-icon" data-act="app" data-app="' + a.key + '">' +
        '<span class="ph-ai-ic">' + a.icon + '</span>' +
        '<span class="ph-ai-lb">' + esc(a.label) + '</span>' +
        (badge ? '<span class="ph-ai-badge">' + esc(badge) + '</span>' : '') +
      '</button>');
    });
    h.push('</div>');

    // 快捷卡片：下一项日程 + 最近联系人
    var next = (st.schedule || []).filter(function (s) { return !s.done; })[0];
    h.push('<div class="ph-cards">');
    if (next) {
      h.push('<div class="ph-card" data-act="app" data-app="plan">' +
        '<div class="ph-card-t">下一项日程</div>' +
        '<div class="ph-card-b"><b>' + esc(next.time || '--:--') + '</b> ' + esc(next.title) + '</div></div>');
    }
    var last = contacts[0];
    if (last) {
      h.push('<div class="ph-card" data-act="open-chat" data-id="' + esc(last.id) + '">' +
        '<div class="ph-card-t">最近消息 · ' + esc(last.name) + '</div>' +
        '<div class="ph-card-b">' + esc((last.messages[last.messages.length - 1] || {}).text || '暂无消息') + '</div></div>');
    }
    h.push('</div>');
    return '<div class="ph-app ph-app-home">' + h.join('') + tabBarHTML('home') + '</div>';
  }

  // ── T5-2 地图/导航 ──
  function renderMapScreen(st) {
    var me = st.location || null;
    var plan = st.plan || null;
    var opts = st.planOptions || [];
    var picked = num(st.planPick, 0);
    var h = [];
    h.push('<div class="ph-mapwrap"><canvas class="ph-mapcanvas" width="300" height="190"></canvas>' +
      '<div class="ph-maptag">' + (me ? '📍 ' + esc(me.area || '已定位') : '📍 未定位') + '</div></div>');

    h.push('<div class="ph-row ph-row-gap">' +
      '<button class="ph-btn" data-act="locate">🎯 定位我</button>' +
      '<button class="ph-btn" data-act="route">🧭 规划路线</button>' +
      '</div>');

    h.push('<div class="ph-field"><label>起点</label>' +
      '<input class="ph-input" data-field="from" value="' + esc(st.from ? st.from.name : (me && me.area ? me.area : '当前位置')) + '" placeholder="当前位置"></div>');
    h.push('<div class="ph-field"><label>终点</label>' +
      '<input class="ph-input" data-field="to" value="' + esc(st.to ? st.to.name : '') + '" placeholder="输入目的地名称"></div>');

    h.push('<div class="ph-chips">');
    (st.places || DEMO_PLACES).forEach(function (p) {
      h.push('<button class="ph-chip' + (st.to && st.to.id === p.id ? ' on' : '') + '" data-act="pick-place" data-id="' + esc(p.id) + '">' +
        esc(p.name) + '</button>');
    });
    h.push('</div>');

    if (st.planLoading) h.push('<div class="ph-empty">正在规划…</div>');
    if (st.planError) h.push('<div class="ph-error">⚠ ' + esc(st.planError) + '</div>');

    if (opts.length) {
      // 直接复用 transport.js 的方案列表 HTML（T5-2 要求）
      var TR = root_transport();
      if (TR && isFn(TR.routeOptionsHTML)) {
        h.push('<div class="ph-tpwrap">' + TR.routeOptionsHTML(opts, { selected: picked, showReason: true, showSteps: true }) + '</div>');
      } else {
        h.push('<div class="ph-cards">');
        opts.forEach(function (o, i) {
          h.push('<div class="ph-listitem' + (i === picked ? ' on' : '') + '" data-act="pick-option" data-idx="' + i + '">' +
            '<span class="ph-li-icon">' + esc(o.icon || '🧭') + '</span>' +
            '<span class="ph-li-main"><b>' + esc(o.mode_name || o.mode) + '</b>' +
            '<small>' + esc(o.summary || '') + '</small></span>' +
            '<span class="ph-li-tail"><b>' + esc(o.duration_text || fmtMinutes(o.duration_min)) + '</b>' +
            '<small>' + esc(o.cost_text || fmtCost(o.cost)) + '</small></span></div>');
        });
        h.push('</div>');
      }
      var sel = opts[clamp(picked, 0, opts.length - 1)];
      if (sel) {
        h.push('<div class="ph-sect">行程明细 · ' + esc(sel.mode_name || '') + '</div>');
        h.push('<div class="ph-steps">');
        (sel.steps || []).forEach(function (s) {
          h.push('<div class="ph-step"><span>' + esc(s.icon || '•') + '</span><span class="ph-step-n">' + esc(s.note || s.mode_name || '') + '</span>' +
            '<span class="ph-step-t">' + esc(fmtMinutes(s.duration_min)) + ' · ' + esc(fmtDistance(s.distance_m)) + '</span></div>');
        });
        h.push('</div>');
        h.push('<div class="ph-row ph-row-gap">' +
          '<button class="ph-btn ph-btn-primary" data-act="start-trip">▶ 开始导航</button>' +
          '<button class="ph-btn" data-act="goto-taxi">🚕 打车去这里</button></div>');
      }
    } else if (!st.planLoading) {
      h.push('<div class="ph-empty">选一个常用目的地，或输入终点后点「规划路线」</div>');
    }

    if (st.trip) {
      var TR2 = root_transport();
      h.push('<div class="ph-sect">导航进行中</div>');
      h.push('<div class="ph-tpwrap">' + (TR2 && isFn(TR2.tripPanelHTML) ? TR2.tripPanelHTML(st.trip)
        : '<div class="ph-empty">导航中…</div>') + '</div>');
      h.push('<div class="ph-row ph-row-gap"><button class="ph-btn" data-act="stop-trip">■ 结束导航</button></div>');
    }
    return wrapApp('map', '地图 / 导航', h.join(''), '<span class="ph-pill">PNG 优先</span>');
  }

  // ── T5-3 打车 ──
  function renderTaxiScreen(st) {
    var ride = st.ride || createRide({});
    var v = rideView(ride);
    var h = [];
    h.push('<div class="ph-mapwrap"><canvas class="ph-mapcanvas" width="300" height="190"></canvas>' +
      '<div class="ph-maptag">' + esc(v.label) + '</div></div>');

    h.push('<div class="ph-ride">' +
      '<div class="ph-ride-head"><span class="ph-ride-state ph-st-' + esc(v.status) + '">' + esc(v.label) + '</span>' +
      '<span class="ph-ride-eta">' + esc(v.etaText) + '</span></div>' +
      '<div class="ph-bar"><i style="width:' + v.progressPct + '%"></i></div>' +
      '<div class="ph-ride-hint">' + esc(v.hint) + '</div>' +
      '<div class="ph-ride-steps">');
    v.steps.forEach(function (s) {
      h.push('<span class="ph-rstep ph-rstep-' + s.state + '">' + esc(s.label) + '</span>');
    });
    h.push('</div></div>');

    if (v.driver) {
      h.push('<div class="ph-driver">' +
        '<span class="ph-driver-av">🧑‍✈️</span>' +
        '<span class="ph-driver-main"><b>' + esc(v.driver.name) + '</b>' +
        '<small>' + esc(v.driver.car) + ' · ' + esc(v.driver.color) + '</small>' +
        '<small>评分 ' + esc(v.driver.rating) + '</small></span>' +
        '<button class="ph-btn ph-btn-sm" data-act="call-driver">📞 联系司机</button>' +
      '</div>');
    }

    h.push('<div class="ph-field"><label>去哪</label>' +
      '<input class="ph-input" data-field="rideTo" value="' + esc(ride.toName || (st.to ? st.to.name : '')) + '" placeholder="输入目的地"></div>');
    h.push('<div class="ph-chips">');
    (st.places || DEMO_PLACES).forEach(function (p) {
      h.push('<button class="ph-chip' + (ride.to && ride.to.id === p.id ? ' on' : '') + '" data-act="ride-place" data-id="' + esc(p.id) + '">' +
        esc(p.name) + '</button>');
    });
    h.push('</div>');

    if (ride.status === 'idle') {
      h.push('<button class="ph-btn ph-btn-primary ph-btn-block" data-act="ride-call">🚕 一键叫车（' + esc(ride.toName || '未选目的地') + '）</button>');
    } else if (v.canCancel) {
      h.push('<div class="ph-row ph-row-gap">' +
        '<button class="ph-btn ph-btn-primary" data-act="ride-advance">⏩ 模拟推进 1 步</button>' +
        '<button class="ph-btn ph-btn-danger" data-act="ride-cancel">取消叫车</button></div>');
    } else if (v.canReset) {
      h.push('<div class="ph-row ph-row-gap">' +
        '<button class="ph-btn ph-btn-primary" data-act="ride-reset">再叫一辆</button>' +
        '<button class="ph-btn" data-act="ride-advance">⏩ 模拟推进</button></div>');
    }

    h.push('<div class="ph-sect">历史状态流转</div><div class="ph-log">' +
      (ride.history.length
        ? ride.history.map(function (x) { return '<div>#' + x.at + ' ' + esc(RIDE_LABEL[x.from] || x.from) + ' → ' + esc(RIDE_LABEL[x.to] || x.to) + '</div>'; }).join('')
        : '<div class="ph-empty">暂无记录</div>') +
      '</div>');
    return wrapApp('taxi', '打车', h.join(''), '<span class="ph-pill">' + esc(ride.priceText) + '</span>');
  }

  // ── T5-4 公交/地铁 ──
  function renderTransitScreen(st) {
    var stops = st.stops || [];
    var h = [];
    h.push('<div class="ph-row ph-row-gap">' +
      '<span class="ph-origin">起点：' + esc(st.transitFrom || '当前位置') + '</span>' +
      '</div>');
    h.push('<div class="ph-field"><label>终点</label>' +
      '<input class="ph-input" data-field="transitTo" value="' + esc(st.transitTo || '') + '" placeholder="输入终点后查询公交/地铁"></div>');
    h.push('<div class="ph-row ph-row-gap">' +
      '<button class="ph-btn ph-btn-primary" data-act="transit-plan">🚇 查询公交/地铁方案</button>' +
      '<button class="ph-btn" data-act="stops-refresh">🔄 附近站点</button></div>');
    if (st.transitError) h.push('<div class="ph-error">⚠ ' + esc(st.transitError) + '</div>');

    h.push('<div class="ph-sect">附近站点 · ' + stops.length + ' 个</div>');
    if (!stops.length) h.push('<div class="ph-empty">点「附近站点」从 /api/facilities 拉取交通节点</div>');
    h.push('<div class="ph-stops">');
    stops.forEach(function (s) {
      h.push('<div class="ph-stop">' +
        '<span class="ph-stop-ic">' + esc(s.icon) + '</span>' +
        '<span class="ph-stop-main"><b>' + esc(s.name) + '</b>' +
        '<small>' + esc(s.type_zh || '') + (s.lines && s.lines.length ? ' · ' + esc(s.lines.join('/')) : '') +
        (s.fallback ? ' · 示例' : '') + '</small></span>' +
        '<span class="ph-stop-tail"><b>' + esc(s.distance_text || fmtDistance(s.distance_m)) + '</b>' +
        '<span class="ph-stop-btns">' +
        '<button class="ph-mini" data-act="stop-from" data-id="' + esc(s.id) + '">设为起点</button>' +
        '<button class="ph-mini" data-act="stop-to" data-id="' + esc(s.id) + '">设为终点</button>' +
        '</span></span>' +
      '</div>');
    });
    h.push('</div>');

    var tos = st.transitOptions || [];
    if (tos.length) {
      h.push('<div class="ph-sect">公交/地铁方案（已过滤 ' + tos.length + ' 条）</div>');
      var TR = root_transport();
      h.push('<div class="ph-tpwrap">' + (TR && isFn(TR.routeOptionsHTML)
        ? TR.routeOptionsHTML(tos, { title: false, showReason: true })
        : '<div class="ph-empty">transport.js 未加载，仅显示数量</div>') + '</div>');
    }
    return wrapApp('transit', '公交 / 地铁', h.join(''), '<span class="ph-pill">设施节点</span>');
  }

  // ── T5-5 通讯 ──
  function renderChatScreen(st) {
    var contacts = st.contacts || [];
    var open = st.openChat ? findContact(contacts, st.openChat) : null;
    if (open) return renderConversation(st, open);

    var h = [];
    h.push('<div class="ph-sect">消息</div>');
    h.push('<div class="ph-contacts">');
    contacts.forEach(function (c) {
      var last = c.messages[c.messages.length - 1] || {};
      h.push('<div class="ph-contact' + (c.pinned ? ' pinned' : '') + '" data-act="open-chat" data-id="' + esc(c.id) + '">' +
        '<span class="ph-avatar">' + esc(c.avatar) + (c.online ? '<i class="ph-online"></i>' : '') + '</span>' +
        '<span class="ph-contact-main"><b>' + esc(c.name) + '</b>' +
        '<small>' + esc(last.text || c.role || '打个招呼吧') + '</small></span>' +
        '<span class="ph-contact-tail">' +
        (c.unread ? '<i class="ph-badge">' + c.unread + '</i>' : '') +
        '<button class="ph-mini" data-act="call-chat" data-id="' + esc(c.id) + '">📞</button>' +
        '</span></div>');
    });
    h.push('</div>');
    if (!contacts.length) h.push('<div class="ph-empty">暂无联系人（可 setContacts([...]) 接入 LingChat 角色）</div>');
    return wrapApp('chat', '通讯', h.join(''), '<span class="ph-pill">' + contacts.length + ' 位</span>');
  }

  function renderConversation(st, c) {
    var h = [];
    h.push('<div class="ph-conv-head">' +
      '<button class="ph-icon-btn" data-act="close-chat" aria-label="返回">‹</button>' +
      '<span class="ph-avatar ph-avatar-sm">' + esc(c.avatar) + '</span>' +
      '<span class="ph-conv-name">' + esc(c.name) + '<small>' + esc(c.online ? '在线' : (c.role || '离线')) + '</small></span>' +
      '<button class="ph-icon-btn" data-act="call-chat" data-id="' + esc(c.id) + '" aria-label="打电话">📞</button>' +
      '</div>');
    h.push('<div class="ph-bubbles">');
    c.messages.forEach(function (m) {
      h.push('<div class="ph-bubble ' + (m.from === 'me' ? 'me' : 'them') + '">' +
        '<span class="ph-bubble-t">' + esc(m.text) + '</span>' +
        (m.t ? '<span class="ph-bubble-time">' + esc(m.t) + '</span>' : '') + '</div>');
    });
    h.push('</div>');
    if (st.call) h.push('<div class="ph-calling">📞 通话中 · ' + esc(c.name) + ' · ' + esc(fmtClock(st.call.sec)) +
      ' <button class="ph-mini" data-act="call-hangup">挂断</button></div>');
    h.push('<div class="ph-compose">' +
      '<input class="ph-input" data-field="msg" value="" placeholder="发消息给 ' + esc(c.name) + '…">' +
      '<button class="ph-btn ph-btn-primary" data-act="send-msg">发送</button></div>');
    return wrapApp('chat', c.name, h.join(''), '<span class="ph-pill">会话</span>');
  }

  // ── T5-6 日程/待办 ──
  function renderPlanScreen(st) {
    var list = st.schedule || [];
    var done = list.filter(function (s) { return s.done; }).length;
    var h = [];
    h.push('<div class="ph-sect">今日日程 · ' + done + '/' + list.length + ' 已完成</div>');
    h.push('<div class="ph-todos">');
    list.forEach(function (s) {
      h.push('<div class="ph-todo' + (s.done ? ' done' : '') + '">' +
        '<button class="ph-check" data-act="todo-toggle" data-id="' + esc(s.id) + '" aria-label="完成">' + (s.done ? '✓' : '') + '</button>' +
        '<span class="ph-todo-main"><b>' + esc(s.title) + '</b>' +
        (s.detail ? '<small>' + esc(s.detail) + '</small>' : '') + '</span>' +
        '<span class="ph-todo-tail">' + (s.time ? '<b>' + esc(s.time) + '</b>' : '') +
        (s.tag ? '<i class="ph-tag">' + esc(s.tag) + '</i>' : '') + '</span>' +
      '</div>');
    });
    if (!list.length) h.push('<div class="ph-empty">暂无日程（可 setSchedule([...]) 接入 LingChat 日程）</div>');
    h.push('</div>');
    h.push('<div class="ph-compose">' +
      '<input class="ph-input" data-field="todo" value="" placeholder="新增待办，例如「18:00 取快递」">' +
      '<button class="ph-btn ph-btn-primary" data-act="todo-add">＋ 添加</button></div>');
    h.push('<div class="ph-row ph-row-gap"><span class="ph-hint">勾选与新增为内存态，刷新即还原</span></div>');
    return wrapApp('plan', '日程 / 待办', h.join(''), '<span class="ph-pill">' + (list.length - done) + ' 项待办</span>');
  }

  // ── T5-7 天气 ──
  function renderWeatherScreen(st) {
    var w = normalizeWeather(st.rawWeather || st.weather || {});
    var h = [];
    if (!w.ok) h.push('<div class="ph-error">⚠ ' + esc(w.error || '天气获取失败') + '</div>');
    h.push('<div class="ph-weather-hero">' +
      '<div class="ph-w-icon">' + esc(w.icon) + '</div>' +
      '<div class="ph-w-temp">' + esc(tempText(w)) + '</div>' +
      '<div class="ph-w-desc">' + esc(w.desc || w.kindLabel) + '</div>' +
      '<div class="ph-w-city">' + esc(w.city || '未知城市') + ' · 体感 ' + esc(w.feels_like_c === null ? '--' : Math.round(w.feels_like_c) + '°') + '</div>' +
      '</div>');
    h.push('<div class="ph-metrics">' +
      metric('💧 湿度', w.humidity === null ? '--' : w.humidity + '%') +
      metric('🌬️ 风', esc(w.wind_kmph) + ' km/h · ' + esc(w.wind_text)) +
      metric('☁️ 云量', w.cloudcover + '%') +
      metric('🌧️ 降水', w.precip_mm + ' mm') +
      metric('👁️ 能见度', w.visibility_km === null ? '--' : w.visibility_km + ' km') +
      metric('🕒 更新', w.fetchedAt ? new Date(w.fetchedAt).toTimeString().slice(0, 5) : '未加载') +
      '</div>');
    h.push('<div class="ph-row ph-row-gap">' +
      '<button class="ph-btn ph-btn-primary" data-act="weather-refresh">🔄 刷新天气</button>' +
      '<button class="ph-btn" data-act="weather-city">🏙️ 按定位取城市</button></div>');
    h.push('<div class="ph-field"><label>手动城市</label><input class="ph-input" data-field="city" value="' + esc(st.city || '') + '" placeholder="例如 Guangzhou / 广州"></div>');
    h.push('<div class="ph-hint">数据来源：/api/weather（wttr.in 真实天气，服务端已中文化）' + (w.cached ? ' · 服务端缓存' : '') + '</div>');
    return wrapApp('weather', '天气', h.join(''), '<span class="ph-pill">' + esc(tempText(w)) + '</span>');
  }
  function metric(k, v) {
    return '<div class="ph-metric"><span class="ph-metric-k">' + esc(k) + '</span><span class="ph-metric-v">' + esc(v) + '</span></div>';
  }

  // ── T5-8 音乐 ──
  function renderMusicScreen(st) {
    var m = st.music || createMusic(DEMO_TRACKS);
    var t = currentTrack(m);
    var h = [];
    h.push('<div class="ph-player">' +
      '<div class="ph-cover">' + esc(t ? t.cover : '🎵') + '</div>' +
      '<div class="ph-track-title">' + esc(t ? t.title : '暂无曲目') + '</div>' +
      '<div class="ph-track-artist">' + esc(t ? t.artist + (t.album ? ' · ' + t.album : '') : '') + '</div>' +
      '<div class="ph-progress"><i style="width:' + pct(musicProgress(m)) + '%"></i></div>' +
      '<div class="ph-progress-time"><span>' + esc(fmtClock(m.position)) + '</span>' +
      '<span>' + esc(fmtClock(t ? t.duration : 0)) + '</span></div>' +
      '<div class="ph-controls">' +
        '<button class="ph-ctrl" data-act="music-prev" aria-label="上一曲">⏮</button>' +
        '<button class="ph-ctrl ph-ctrl-main" data-act="music-toggle" aria-label="播放暂停">' + (m.playing ? '⏸' : '▶') + '</button>' +
        '<button class="ph-ctrl" data-act="music-next" aria-label="下一曲">⏭</button>' +
      '</div>' +
      '<div class="ph-row ph-row-gap ph-row-center">' +
        '<button class="ph-mini' + (m.mode === 'list' ? ' on' : '') + '" data-act="music-mode" data-mode="list">列表</button>' +
        '<button class="ph-mini' + (m.mode === 'single' ? ' on' : '') + '" data-act="music-mode" data-mode="single">单曲</button>' +
        '<button class="ph-mini' + (m.mode === 'shuffle' ? ' on' : '') + '" data-act="music-mode" data-mode="shuffle">随机</button>' +
      '</div>' +
      '</div>');
    h.push('<div class="ph-sect">播放列表 · ' + m.tracks.length + ' 首（' + (st.tracksSource === 'lingchat' ? 'LingChat' : '示例') + '）</div>');
    h.push('<div class="ph-playlist">');
    m.tracks.forEach(function (x, i) {
      h.push('<div class="ph-track' + (i === m.index ? ' on' : '') + '" data-act="music-pick" data-idx="' + i + '">' +
        '<span class="ph-track-i">' + (i === m.index && m.playing ? '🎶' : (i + 1)) + '</span>' +
        '<span class="ph-track-main"><b>' + esc(x.title) + '</b><small>' + esc(x.artist) + '</small></span>' +
        '<span class="ph-track-dur">' + esc(fmtClock(x.duration)) + '</span></div>');
    });
    h.push('</div>');
    h.push('<div class="ph-hint">UI 状态演示，不播放真实音频；setTracks(list) 可接 LingChat 网易云曲目</div>');
    return wrapApp('music', '音乐', h.join(''), '<span class="ph-pill">' + (m.playing ? '播放中' : '暂停') + '</span>');
  }

  /** 屏幕选择：返回该 app 的 HTML（纯函数，自检可直接调用） */
  function renderScreen(app, st) {
    st = st || {};
    switch (app) {
      case 'map': return renderMapScreen(st);
      case 'taxi': return renderTaxiScreen(st);
      case 'transit': return renderTransitScreen(st);
      case 'chat': return renderChatScreen(st);
      case 'plan': return renderPlanScreen(st);
      case 'weather': return renderWeatherScreen(st);
      case 'music': return renderMusicScreen(st);
      default: return renderHomeScreen(st);
    }
  }
  function findContact(list, id) {
    for (var i = 0; i < (list || []).length; i++) if (String(list[i].id) === String(id)) return list[i];
    return null;
  }

  // ══════════════════════════════════════════════════════════════════════
  // 9. 样式（深色玻璃拟态 / 主色 #79d9ff / 圆角 14px / backdrop blur）
  // ══════════════════════════════════════════════════════════════════════

  var CSS = [
    /* 悬浮入口 */
    '.ph-launcher{position:fixed;z-index:9000;width:56px;height:56px;border-radius:50%;border:1px solid rgba(121,217,255,.45);',
    'background:rgba(16,26,40,.72);backdrop-filter:blur(12px);-webkit-backdrop-filter:blur(12px);color:#eaf3ff;',
    'display:flex;align-items:center;justify-content:center;font-size:24px;cursor:grab;touch-action:none;',
    'box-shadow:0 8px 26px rgba(0,0,0,.5),0 0 0 0 rgba(121,217,255,.4);transition:box-shadow .2s,transform .18s,width .18s,height .18s,font-size .18s}',
    '.ph-launcher:active{cursor:grabbing;transform:scale(.94)}',
    '.ph-launcher:hover{box-shadow:0 8px 26px rgba(0,0,0,.5),0 0 0 6px rgba(121,217,255,.12)}',
    '.ph-launcher.ph-collapsed{width:20px;height:20px;font-size:0;opacity:.62;border-color:rgba(121,217,255,.6)}',
    '.ph-launcher.ph-collapsed:hover{width:40px;height:40px;font-size:16px;opacity:1}',
    '.ph-launcher .ph-lb-badge{position:absolute;top:-3px;right:-3px;min-width:18px;height:18px;border-radius:9px;',
    'background:#79d9ff;color:#08131f;font:700 11px/18px system-ui,sans-serif;text-align:center;padding:0 4px}',
    '.ph-launcher.ph-collapsed .ph-lb-badge{display:none}',
    /* 手机壳 */
    '.ph-shell{position:fixed;z-index:9001;font-family:system-ui,"PingFang SC",sans-serif;color:#eaf3ff;',
    'display:flex;flex-direction:column;overflow:hidden;',
    'background:rgba(12,20,32,.82);backdrop-filter:blur(18px) saturate(1.15);-webkit-backdrop-filter:blur(18px) saturate(1.15);',
    'border:1px solid rgba(121,217,255,.28);box-shadow:0 18px 60px rgba(0,0,0,.6)}',
    /* 形态一：半透明叠层（不透明背景层，可看穿） */
    '.ph-shell.ph-mode-sheet{right:14px;bottom:14px;width:330px;height:min(660px,calc(100vh - 28px));border-radius:26px}',
    /* 形态二：角落小窗 */
    '.ph-shell.ph-mode-corner{right:14px;bottom:14px;width:242px;height:392px;border-radius:20px;opacity:.96}',
    '.ph-shell.ph-mode-corner .ph-tab-label{display:none}',
    '.ph-shell.ph-mode-corner .ph-tabs{overflow-x:auto}',
    '.ph-shell.ph-min{cursor:pointer}',
    '.ph-shell.ph-min .ph-body,.ph-shell.ph-min .ph-tabs,.ph-shell.ph-min .ph-appbar{display:none}',
    '.ph-min-card{display:none}',
    '.ph-shell.ph-min .ph-min-card{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:6px;flex:1;text-align:center;padding:10px}',
    '.ph-min-card .c1{font-size:30px}.ph-min-card .c2{font-size:19px;font-weight:700}',
    '.ph-min-card .c3{font-size:11.5px;color:rgba(190,210,240,.85)}',
    '.ph-grip{cursor:grab;touch-action:none}',
    /* 状态栏 */
    '.ph-status{display:flex;align-items:center;gap:8px;padding:7px 14px 5px;font-size:12px;color:#d8e8ff;flex:0 0 auto;cursor:grab;touch-action:none}',
    '.ph-status:active{cursor:grabbing}',
    '.ph-status-time{font-weight:700;letter-spacing:.4px}',
    '.ph-status-mid{flex:1;text-align:center;font-size:11px;color:rgba(180,205,235,.8)}',
    '.ph-status-right{display:flex;align-items:flex-end;gap:7px}',
    '.ph-sig{display:flex;align-items:flex-end;gap:2px;height:12px}',
    '.ph-sig-bar{display:block;width:3px;border-radius:1px;background:rgba(234,243,255,.28)}',
    '.ph-sig-bar.on{background:#eaf3ff}',
    '.ph-bat{display:flex;align-items:center;gap:3px;font-size:10.5px}',
    '.ph-bat-body{display:block;width:22px;height:11px;border:1px solid currentColor;border-radius:3px;padding:1px;position:relative}',
    '.ph-bat-body i{display:block;height:100%;background:currentColor;border-radius:1px}',
    '.ph-bat-bolt{font-size:9px}',
    '.ph-bat-txt{opacity:.85}',
    /* 应用容器 */
    '.ph-app{flex:1;min-height:0;display:flex;flex-direction:column}',
    '.ph-appbar{display:flex;align-items:center;gap:8px;padding:6px 12px 7px;flex:0 0 auto;border-bottom:1px solid rgba(121,217,255,.14)}',
    '.ph-appbar-title{font-size:13.5px;font-weight:700;flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}',
    '.ph-appbar-extra{flex:0 0 auto;display:flex;gap:6px;align-items:center}',
    '.ph-pill{font-size:10px;padding:2px 8px;border-radius:9px;background:rgba(121,217,255,.16);color:#9fe4ff;border:1px solid rgba(121,217,255,.28)}',
    '.ph-icon-btn{width:34px;height:34px;flex:0 0 34px;border-radius:11px;border:1px solid rgba(121,217,255,.22);',
    'background:rgba(255,255,255,.07);color:#eaf3ff;font-size:17px;line-height:1;cursor:pointer;display:flex;align-items:center;justify-content:center}',
    '.ph-icon-btn:hover{background:rgba(121,217,255,.16)}',
    '.ph-body{flex:1;min-height:0;overflow-y:auto;overflow-x:hidden;padding:10px 12px 12px;-webkit-overflow-scrolling:touch}',
    '.ph-body::-webkit-scrollbar{width:5px}.ph-body::-webkit-scrollbar-thumb{background:rgba(121,217,255,.28);border-radius:3px}',
    /* 底部 TAB（触摸友好：高 54px） */
    '.ph-tabs{flex:0 0 auto;display:flex;gap:2px;padding:5px 6px 7px;border-top:1px solid rgba(121,217,255,.14);background:rgba(10,17,27,.5)}',
    '.ph-tab{flex:1;min-width:38px;height:48px;border:none;background:transparent;color:rgba(200,218,240,.72);',
    'border-radius:12px;cursor:pointer;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:2px;font-family:inherit}',
    '.ph-tab:hover{background:rgba(121,217,255,.1)}',
    '.ph-tab.on{background:rgba(121,217,255,.18);color:#eaf3ff}',
    '.ph-tab-icon{font-size:17px;line-height:1}',
    '.ph-tab-label{font-size:9.5px;line-height:1}',
    /* 桌面 */
    '.ph-app-home .ph-body{display:block}',
    '.ph-home-head{padding:4px 2px 12px}',
    '.ph-clock{font-size:38px;font-weight:300;letter-spacing:1px;line-height:1.1}',
    '.ph-clock-sub{font-size:11.5px;color:rgba(180,205,235,.85);margin-top:2px}',
    '.ph-home-weather{margin-top:12px;display:flex;align-items:center;gap:10px;padding:10px 12px;border-radius:14px;',
    'background:rgba(255,255,255,.08);border:1px solid rgba(121,217,255,.18);cursor:pointer}',
    '.ph-home-weather:hover{background:rgba(121,217,255,.14)}',
    '.ph-hw-icon{font-size:24px}',
    '.ph-hw-temp{font-size:24px;font-weight:600}',
    '.ph-hw-desc{font-size:11.5px;color:rgba(190,214,240,.9)}',
    '.ph-grid{display:grid;grid-template-columns:repeat(4,1fr);gap:10px}',
    '.ph-app-icon{position:relative;height:74px;border:1px solid rgba(121,217,255,.14);border-radius:14px;',
    'background:rgba(255,255,255,.08);color:#eaf3ff;cursor:pointer;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:5px;font-family:inherit}',
    '.ph-app-icon:hover{background:rgba(121,217,255,.16);border-color:rgba(121,217,255,.4)}',
    '.ph-ai-ic{font-size:23px;line-height:1}',
    '.ph-ai-lb{font-size:10.5px;color:rgba(210,228,248,.92)}',
    '.ph-ai-badge{position:absolute;top:5px;right:7px;min-width:17px;height:17px;border-radius:9px;background:#79d9ff;',
    'color:#08131f;font:700 10px/17px system-ui,sans-serif;padding:0 4px}',
    '.ph-cards{margin-top:12px;display:flex;flex-direction:column;gap:8px}',
    '.ph-card{padding:10px 12px;border-radius:14px;background:rgba(255,255,255,.08);border:1px solid rgba(121,217,255,.16);cursor:pointer}',
    '.ph-card:hover{background:rgba(121,217,255,.14)}',
    '.ph-card-t{font-size:10.5px;color:rgba(180,205,235,.85)}',
    '.ph-card-b{font-size:12.5px;margin-top:3px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}',
    /* 通用控件（触摸友好 ≥44px） */
    '.ph-btn{min-height:44px;flex:1;padding:10px 13px;border-radius:13px;border:1px solid rgba(121,217,255,.25);',
    'background:rgba(255,255,255,.09);color:#eaf3ff;font:600 12.5px/1.2 inherit;cursor:pointer}',
    '.ph-btn:hover{background:rgba(121,217,255,.16)}',
    '.ph-btn-primary{background:linear-gradient(135deg,#4a8fd8,#79d9ff);color:#08131f;border-color:transparent}',
    '.ph-btn-danger{background:rgba(255,110,110,.16);border-color:rgba(255,110,110,.4);color:#ffd0d0}',
    '.ph-btn-block{width:100%;flex:none}',
    '.ph-btn-sm{flex:none;min-height:36px;padding:7px 11px;font-size:11.5px;border-radius:11px}',
    '.ph-mini{min-height:30px;padding:5px 9px;border-radius:10px;border:1px solid rgba(121,217,255,.22);',
    'background:rgba(255,255,255,.07);color:#cfe4ff;font:600 10.5px/1 inherit;cursor:pointer}',
    '.ph-mini:hover{background:rgba(121,217,255,.18)}',
    '.ph-mini.on{background:rgba(121,217,255,.28);color:#fff}',
    '.ph-row{display:flex;align-items:center}.ph-row-gap{gap:8px;margin:8px 0}.ph-row-center{justify-content:center}',
    '.ph-row-gap .ph-btn{flex:1}',
    '.ph-field{margin:8px 0}',
    '.ph-field label{display:block;font-size:10.5px;color:rgba(180,205,235,.85);margin-bottom:4px}',
    '.ph-input{width:100%;box-sizing:border-box;min-height:44px;padding:11px 12px;border-radius:13px;',
    'border:1px solid rgba(255,255,255,.16);background:rgba(255,255,255,.08);color:#eaf3ff;font:13px inherit;outline:none}',
    '.ph-input:focus{border-color:rgba(121,217,255,.55);background:rgba(121,217,255,.1)}',
    '.ph-input::placeholder{color:rgba(180,205,235,.55)}',
    '.ph-chips{display:flex;flex-wrap:wrap;gap:6px;margin:8px 0}',
    '.ph-chip{min-height:34px;padding:7px 11px;border-radius:16px;border:1px solid rgba(121,217,255,.2);',
    'background:rgba(255,255,255,.07);color:#cfe4ff;font:600 11px/1 inherit;cursor:pointer}',
    '.ph-chip:hover{background:rgba(121,217,255,.16)}.ph-chip.on{background:#79d9ff;color:#08131f;border-color:transparent}',
    '.ph-sect{font:700 11.5px/1.6 inherit;color:#9fe4ff;margin:12px 0 6px;letter-spacing:.3px}',
    '.ph-empty{padding:12px;border-radius:13px;background:rgba(255,255,255,.05);color:rgba(190,210,240,.8);font-size:11.5px;text-align:center}',
    '.ph-error{padding:10px 12px;border-radius:13px;background:rgba(255,110,110,.14);border:1px solid rgba(255,110,110,.3);color:#ffd0d0;font-size:11.5px;margin:8px 0}',
    '.ph-hint{font-size:10.5px;color:rgba(170,195,225,.75);line-height:1.6}',
    '.ph-origin{font-size:11.5px;color:rgba(200,220,245,.9);padding:6px 10px;border-radius:11px;background:rgba(255,255,255,.06)}',
    /* 地图 */
    '.ph-mapwrap{position:relative;margin-bottom:8px}',
    '.ph-mapcanvas{display:block;width:100%;height:190px;border-radius:14px;border:1px solid rgba(121,217,255,.2);background:#0e1926}',
    '.ph-maptag{position:absolute;left:9px;top:9px;font-size:10.5px;padding:3px 9px;border-radius:10px;',
    'background:rgba(10,18,28,.75);border:1px solid rgba(121,217,255,.25);color:#cfe4ff;backdrop-filter:blur(6px)}',
    /* transport.js 的浅色卡片在深色手机里重新配色（只覆盖颜色，不动结构） */
    '.ph-tpwrap{margin:8px 0;font-size:12px}',
    '.ph-tpwrap .tp-title{color:#cfe4ff}',
    '.ph-tpwrap .tp-title .tp-sub{color:rgba(180,205,235,.75)}',
    '.ph-tpwrap .tp-card{background:rgba(255,255,255,.08);border-color:rgba(121,217,255,.18);color:#eaf3ff}',
    '.ph-tpwrap .tp-card:hover{border-color:rgba(121,217,255,.5);box-shadow:0 2px 10px rgba(0,0,0,.3)}',
    '.ph-tpwrap .tp-card-sel{background:rgba(121,217,255,.16);border-color:#79d9ff;box-shadow:0 0 0 2px rgba(121,217,255,.2)}',
    '.ph-tpwrap .tp-name{color:#eaf3ff}.ph-tpwrap .tp-dur{color:#eaf3ff}.ph-tpwrap .tp-dist{color:rgba(180,205,235,.8)}',
    '.ph-tpwrap .tp-cost{color:#ffd166}.ph-tpwrap .tp-reason{color:rgba(190,210,240,.8)}.ph-tpwrap .tp-steps{color:rgba(190,210,240,.8)}',
    '.ph-tpwrap .tp-steps .tp-arrow{color:rgba(180,205,235,.6)}',
    '.ph-tpwrap .tp-badge{background:rgba(255,255,255,.12);color:#cfe4ff}',
    '.ph-tpwrap .tp-badge-rec{background:#79d9ff;color:#08131f}',
    '.ph-tpwrap .tp-badge-ok{background:rgba(76,175,132,.3);color:#b6f0cf}',
    '.ph-tpwrap .tp-empty{color:rgba(190,210,240,.8)}',
    '.ph-tpwrap .tp-trip,.ph-tpwrap .tp-card,.ph-tpwrap .tp-steps-list{border-color:rgba(121,217,255,.18)}',
    '.ph-tpwrap .tp-trip{background:rgba(255,255,255,.08);border-color:rgba(121,217,255,.18)}',
    '.ph-tpwrap .tp-trip-title{color:#eaf3ff}.ph-tpwrap .tp-trip-rows{color:rgba(200,220,245,.85)}',
    '.ph-tpwrap .tp-trip-rows b{color:#eaf3ff}.ph-tpwrap .tp-trip-note,.ph-tpwrap .tp-trip-sum{color:rgba(180,205,235,.75)}',
    '.ph-tpwrap .tp-bar{background:rgba(255,255,255,.14)}',
    '.ph-tpwrap .tp-step{color:rgba(200,220,245,.85);border-top-color:rgba(121,217,255,.14)}',
    '.ph-tpwrap .tp-step-name{color:#eaf3ff}.ph-tpwrap .tp-step-dur{color:#eaf3ff}.ph-tpwrap .tp-step-cost{color:#ffd166}',
    '.ph-tpwrap .tp-step-note{color:rgba(170,195,225,.75)}',
    '.ph-steps{margin-top:4px}',
    '.ph-step{display:flex;align-items:center;gap:8px;padding:6px 2px;font-size:11.5px;border-top:1px dashed rgba(121,217,255,.14)}',
    '.ph-step-n{flex:1;color:rgba(210,228,248,.95);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}',
    '.ph-step-t{color:rgba(180,205,235,.8);flex:0 0 auto}',
    /* 打车 */
    '.ph-ride{padding:10px 12px;border-radius:14px;background:rgba(255,255,255,.08);border:1px solid rgba(121,217,255,.18)}',
    '.ph-ride-head{display:flex;align-items:center;gap:8px}',
    '.ph-ride-state{font-size:11.5px;font-weight:700;padding:3px 9px;border-radius:10px;background:rgba(121,217,255,.18);color:#9fe4ff}',
    '.ph-st-arriving,.ph-st-onboard{background:rgba(245,197,24,.2);color:#ffe08a}',
    '.ph-st-done{background:rgba(76,175,132,.24);color:#b6f0cf}',
    '.ph-st-canceled{background:rgba(255,110,110,.2);color:#ffd0d0}',
    '.ph-ride-eta{margin-left:auto;font-size:11.5px;color:rgba(210,228,248,.95)}',
    '.ph-bar{height:6px;border-radius:3px;background:rgba(255,255,255,.14);margin:9px 0;overflow:hidden}',
    '.ph-bar i{display:block;height:100%;border-radius:3px;background:linear-gradient(90deg,#4a8fd8,#79d9ff);transition:width .3s linear}',
    '.ph-ride-hint{font-size:11px;color:rgba(190,210,240,.85)}',
    '.ph-ride-steps{display:flex;gap:5px;margin-top:9px;flex-wrap:wrap}',
    '.ph-rstep{font-size:9.5px;padding:3px 7px;border-radius:9px;background:rgba(255,255,255,.07);color:rgba(190,210,240,.75)}',
    '.ph-rstep-now{background:rgba(121,217,255,.28);color:#eaf3ff;font-weight:700}',
    '.ph-rstep-done{background:rgba(76,175,132,.22);color:#b6f0cf}',
    '.ph-driver{display:flex;align-items:center;gap:9px;padding:9px 11px;margin:8px 0;border-radius:14px;background:rgba(255,255,255,.08);border:1px solid rgba(121,217,255,.18)}',
    '.ph-driver-av{font-size:24px}',
    '.ph-driver-main{flex:1;min-width:0;display:flex;flex-direction:column}',
    '.ph-driver-main b{font-size:12.5px}.ph-driver-main small{font-size:10.5px;color:rgba(180,205,235,.85)}',
    '.ph-log{margin-top:4px;font:11px/1.8 ui-monospace,monospace;color:rgba(180,205,235,.85);background:rgba(0,0,0,.22);border-radius:12px;padding:8px 10px}',
    /* 站点 */
    '.ph-stops{display:flex;flex-direction:column;gap:7px}',
    '.ph-stop{display:flex;align-items:center;gap:9px;padding:9px 11px;border-radius:14px;background:rgba(255,255,255,.07);border:1px solid rgba(121,217,255,.14)}',
    '.ph-stop-ic{font-size:18px}',
    '.ph-stop-main{flex:1;min-width:0;display:flex;flex-direction:column}',
    '.ph-stop-main b{font-size:12.5px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}',
    '.ph-stop-main small{font-size:10.5px;color:rgba(180,205,235,.8);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}',
    '.ph-stop-tail{display:flex;flex-direction:column;align-items:flex-end;gap:4px}',
    '.ph-stop-tail b{font-size:11.5px;color:#9fe4ff}',
    '.ph-stop-btns{display:flex;gap:4px}',
    /* 通讯 */
    '.ph-contacts{display:flex;flex-direction:column;gap:7px}',
    '.ph-contact{display:flex;align-items:center;gap:10px;padding:9px 11px;border-radius:14px;background:rgba(255,255,255,.07);',
    'border:1px solid rgba(121,217,255,.14);cursor:pointer}',
    '.ph-contact:hover{background:rgba(121,217,255,.14)}',
    '.ph-contact.pinned{border-color:rgba(121,217,255,.34)}',
    '.ph-avatar{position:relative;width:40px;height:40px;flex:0 0 40px;border-radius:13px;background:rgba(121,217,255,.14);',
    'display:flex;align-items:center;justify-content:center;font-size:20px}',
    '.ph-avatar-sm{width:30px;height:30px;flex:0 0 30px;border-radius:10px;font-size:16px}',
    '.ph-online{position:absolute;right:-1px;bottom:-1px;width:10px;height:10px;border-radius:50%;background:#7dffb0;border:2px solid #0c1420}',
    '.ph-contact-main{flex:1;min-width:0;display:flex;flex-direction:column}',
    '.ph-contact-main b{font-size:12.5px}.ph-contact-main small{font-size:10.5px;color:rgba(180,205,235,.82);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}',
    '.ph-contact-tail{display:flex;align-items:center;gap:6px}',
    '.ph-badge{min-width:18px;height:18px;border-radius:9px;background:#79d9ff;color:#08131f;font:700 10px/18px system-ui,sans-serif;text-align:center;padding:0 4px}',
    '.ph-conv-head{display:flex;align-items:center;gap:8px;padding:2px 0 8px;border-bottom:1px solid rgba(121,217,255,.14)}',
    '.ph-conv-name{flex:1;display:flex;flex-direction:column;font-size:13px;font-weight:700}',
    '.ph-conv-name small{font-size:10px;font-weight:400;color:rgba(180,205,235,.8)}',
    '.ph-bubbles{display:flex;flex-direction:column;gap:7px;padding:10px 0}',
    '.ph-bubble{max-width:80%;padding:8px 11px;border-radius:14px;font-size:12.5px;line-height:1.5;position:relative}',
    '.ph-bubble.them{align-self:flex-start;background:rgba(255,255,255,.1);border:1px solid rgba(121,217,255,.16)}',
    '.ph-bubble.me{align-self:flex-end;background:linear-gradient(135deg,#2f6ea8,#4a9fd8);color:#f2fbff}',
    '.ph-bubble-time{display:block;font-size:9.5px;opacity:.7;margin-top:2px}',
    '.ph-calling{margin:6px 0;padding:9px 11px;border-radius:13px;background:rgba(125,255,176,.14);border:1px solid rgba(125,255,176,.34);font-size:12px;display:flex;align-items:center;gap:8px}',
    '.ph-compose{display:flex;gap:7px;align-items:center;margin-top:8px}',
    '.ph-compose .ph-btn{flex:0 0 auto;min-height:44px}',
    /* 日程 */
    '.ph-todos{display:flex;flex-direction:column;gap:7px}',
    '.ph-todo{display:flex;align-items:center;gap:10px;padding:9px 11px;border-radius:14px;background:rgba(255,255,255,.07);border:1px solid rgba(121,217,255,.14)}',
    '.ph-todo.done{opacity:.55}',
    '.ph-todo.done .ph-todo-main b{text-decoration:line-through}',
    '.ph-check{width:30px;height:30px;flex:0 0 30px;border-radius:10px;border:1px solid rgba(121,217,255,.35);',
    'background:rgba(255,255,255,.06);color:#08131f;font-size:15px;font-weight:700;cursor:pointer}',
    '.ph-check:hover{background:rgba(121,217,255,.2)}',
    '.ph-todo.done .ph-check{background:#79d9ff;border-color:#79d9ff;color:#08131f}',
    '.ph-todo-main{flex:1;min-width:0;display:flex;flex-direction:column}',
    '.ph-todo-main b{font-size:12.5px}.ph-todo-main small{font-size:10.5px;color:rgba(180,205,235,.8)}',
    '.ph-todo-tail{display:flex;flex-direction:column;align-items:flex-end;gap:3px}.ph-todo-tail b{font-size:11.5px;color:#9fe4ff}',
    '.ph-tag{font-size:9.5px;padding:2px 7px;border-radius:8px;background:rgba(121,217,255,.16);color:#9fe4ff;font-style:normal}',
    /* 天气 */
    '.ph-weather-hero{text-align:center;padding:12px 0 8px}',
    '.ph-w-icon{font-size:52px;line-height:1.1}',
    '.ph-w-temp{font-size:44px;font-weight:300;letter-spacing:1px}',
    '.ph-w-desc{font-size:13px;color:rgba(210,228,248,.95)}',
    '.ph-w-city{font-size:11px;color:rgba(180,205,235,.8);margin-top:3px}',
    '.ph-metrics{display:grid;grid-template-columns:1fr 1fr;gap:7px;margin:6px 0}',
    '.ph-metric{display:flex;flex-direction:column;gap:2px;padding:9px 11px;border-radius:13px;background:rgba(255,255,255,.07);border:1px solid rgba(121,217,255,.14)}',
    '.ph-metric-k{font-size:10.5px;color:rgba(180,205,235,.85)}.ph-metric-v{font-size:13px;font-weight:600}',
    /* 音乐 */
    '.ph-player{padding:6px 2px 10px;text-align:center}',
    '.ph-cover{width:132px;height:132px;margin:6px auto 10px;border-radius:22px;display:flex;align-items:center;justify-content:center;',
    'font-size:62px;background:linear-gradient(140deg,rgba(121,217,255,.22),rgba(74,143,216,.28));border:1px solid rgba(121,217,255,.3);',
    'box-shadow:inset 0 0 40px rgba(121,217,255,.14)}',
    '.ph-track-title{font-size:15px;font-weight:700}',
    '.ph-track-artist{font-size:11.5px;color:rgba(180,205,235,.85);margin-top:2px}',
    '.ph-progress{height:5px;border-radius:3px;background:rgba(255,255,255,.16);margin:12px 2px 4px;overflow:hidden}',
    '.ph-progress i{display:block;height:100%;background:linear-gradient(90deg,#4a8fd8,#79d9ff);border-radius:3px}',
    '.ph-progress-time{display:flex;justify-content:space-between;font-size:10px;color:rgba(180,205,235,.85)}',
    '.ph-controls{display:flex;align-items:center;justify-content:center;gap:16px;margin-top:10px}',
    '.ph-ctrl{width:48px;height:48px;border-radius:16px;border:1px solid rgba(121,217,255,.22);background:rgba(255,255,255,.08);',
    'color:#eaf3ff;font-size:19px;cursor:pointer}',
    '.ph-ctrl:hover{background:rgba(121,217,255,.18)}',
    '.ph-ctrl-main{width:60px;height:60px;border-radius:20px;font-size:24px;background:linear-gradient(135deg,#4a8fd8,#79d9ff);color:#08131f;border-color:transparent}',
    '.ph-playlist{display:flex;flex-direction:column;gap:6px;margin-top:4px}',
    '.ph-track{display:flex;align-items:center;gap:9px;padding:8px 10px;border-radius:12px;background:rgba(255,255,255,.06);cursor:pointer}',
    '.ph-track:hover{background:rgba(121,217,255,.14)}',
    '.ph-track.on{background:rgba(121,217,255,.2);border:1px solid rgba(121,217,255,.34)}',
    '.ph-track-i{width:20px;text-align:center;font-size:11px;color:rgba(180,205,235,.85)}',
    '.ph-track-main{flex:1;min-width:0;display:flex;flex-direction:column}',
    '.ph-track-main b{font-size:12px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}',
    '.ph-track-main small{font-size:10px;color:rgba(180,205,235,.8)}',
    '.ph-track-dur{font-size:10.5px;color:rgba(180,205,235,.85)}',
    /* 提示条 */
    '.ph-toast{position:fixed;left:50%;transform:translateX(-50%);bottom:22px;z-index:9500;padding:10px 16px;border-radius:14px;',
    'background:rgba(10,18,28,.92);border:1px solid rgba(121,217,255,.35);color:#eaf3ff;font:12.5px/1.4 system-ui,sans-serif;',
    'backdrop-filter:blur(10px);opacity:0;transition:opacity .22s,transform .22s;pointer-events:none}',
    '.ph-toast.on{opacity:1;transform:translateX(-50%) translateY(-4px)}'
  ].join('\n');

  /** 注入样式（幂等） */
  function injectCSS(doc) {
    doc = doc || (typeof document !== 'undefined' ? document : null);
    if (!doc || !doc.createElement) return false;
    if (doc.getElementById && doc.getElementById('ph-css')) return true;
    var st = doc.createElement('style');
    st.id = 'ph-css';
    st.textContent = CSS;
    (doc.head || doc.documentElement).appendChild(st);
    return true;
  }

  // ══════════════════════════════════════════════════════════════════════
  // 10. 悬浮手机实例（T5-1：入口 / 拖动 / 收起 / 双形态）
  // ══════════════════════════════════════════════════════════════════════

  var APP_TITLES = {
    home: '桌面', map: '地图 / 导航', taxi: '打车', transit: '公交 / 地铁',
    chat: '通讯', plan: '日程 / 待办', weather: '天气', music: '音乐'
  };

  /**
   * 创建悬浮手机。
   * @param {object} opt
   *   api     接口地址，默认 http://127.0.0.1:8790
   *   mode    'sheet' 半透明叠层（默认） | 'corner' 角落小窗
   *   open    初始是否展开，默认 false（只显示悬浮按钮）
   *   collapsed 是否收起成小圆点，默认 false
   *   contacts/schedule/tracks/places  初始数据（等价于 setContacts 等）
   *   autoFetch 是否自动拉取 time/weather/location，默认 true
   */
  function createPhone(opt) {
    opt = opt || {};
    var state = {
      api: opt.api || DEFAULT_API,
      app: 'home',
      mode: opt.mode === 'corner' ? 'corner' : 'sheet',
      open: !!opt.open,
      collapsed: !!opt.collapsed,
      minimized: false,           // 手机缩成角落小窗里的「迷你卡」
      time: null,                 // /api/time
      weather: normalizeWeather(opt.weather || {}),
      rawWeather: opt.weather || null,
      city: opt.city || '',
      location: null,             // /api/location
      from: null, to: null,
      places: (opt.places || DEMO_PLACES).slice(),
      plan: null, planOptions: [], planPick: 0, planLoading: false, planError: '',
      trip: null,
      ride: createRide({}),
      stops: [], transitOptions: [], transitFrom: '当前位置', transitTo: '', transitError: '',
      contacts: normalizeContacts(opt.contacts) || deepCopy(DEMO_CONTACTS),
      openChat: null, draft: {},
      call: null,                 // {sec, contactId, timer}
      schedule: normalizeSchedule(opt.schedule) || deepCopy(DEMO_SCHEDULE),
      music: createMusic(normalizeTracks(opt.tracks) || DEMO_TRACKS),
      tracksSource: normalizeTracks(opt.tracks) ? 'lingchat' : 'demo',
      battery: { level: 82, charging: false },
      signal: { bars: 4, type: '' },
      network: 'ok',
      toast: '',
      ready: { time: false, weather: false, location: false }
    };
    if (opt.autoFetch !== false) {
      // 不主动打网络，等 mount() 时按需拉取（Node 自检下 createPhone 是纯的）
    }
    var dom = { root: null, launcher: null, shell: null, body: null, status: null, toast: null, canvas: null };
    var timers = { clock: null, fast: null, slow: null, toast: null };
    var sigs = {};
    var dragging = null;
    var bound = false;
    var destroyed = false;
    var listeners = {};

    // ── 事件总线（给宿主页面用） ──
    function on(ev, fn) {
      (listeners[ev] = listeners[ev] || []).push(fn);
      return function () { off(ev, fn); };
    }
    function off(ev, fn) {
      var l = listeners[ev] || [];
      var i = l.indexOf(fn);
      if (i >= 0) l.splice(i, 1);
    }
    function emit(ev, payload) {
      (listeners[ev] || []).slice().forEach(function (f) { try { f(payload); } catch (e) { } });
    }

    // ── 状态更新 ──
    function patch(p) { assign(state, p); scheduleRender(); return state; }
    function setApp(app) { state.app = app; state.openChat = null; scheduleRender(); }
    function toast(msg) {
      state.toast = String(msg || '');
      scheduleRender();
      if (timers.toast) clearTimeout(timers.toast);
      timers.toast = setTimeout(function () { state.toast = ''; refreshToast(); }, 1900);
    }
    function refreshToast() {
      if (!dom.toast) return;
      dom.toast.textContent = state.toast;
      dom.toast.className = 'ph-toast' + (state.toast ? ' on' : '');
    }

    /** 渲染节流：同一帧内多次 patch 只重画一次 */
    var pending = false;
    function scheduleRender() {
      if (pending || destroyed) return;
      pending = true;
      var run = function () { pending = false; render(); };
      if (typeof requestAnimationFrame === 'function') requestAnimationFrame(run);
      else setTimeout(run, 16);
    }

    /**
     * 渲染：按「屏幕签名」判断是否需要重建 DOM。
     * 打字（输入框）不会触发重建，避免光标丢失。
     */
    function render() {
      if (!dom.shell) return;
      var s = state;
      dom.launcher.className = 'ph-launcher' + (s.collapsed ? ' ph-collapsed' : '');
      dom.launcher.innerHTML = '📱' + (unreadTotal() ? '<i class="ph-lb-badge">' + unreadTotal() + '</i>' : '');
      dom.launcher.title = s.open ? '收起手机' : '打开手机（可拖动）';

      dom.shell.className = 'ph-shell ph-mode-' + s.mode + (s.minimized ? ' ph-min' : '');
      dom.shell.style.display = s.open ? 'flex' : 'none';
      if (s.open) layoutShell();

      var sigTime = makeStatus({ time: s.time }).hhmm;
      var sig = [s.app, s.openChat, sigTime, s.battery.level, s.signal.bars, s.network,
        s.planOptions.length, s.planPick, s.planLoading, s.planError, s.trip ? s.trip.progress : '',
        s.ride.status, Math.round(s.ride.etaMin), s.ride.seq,
        s.stops.length, s.transitOptions.length, s.transitFrom, s.transitTo, s.transitError,
        s.contacts.map(function (c) { return c.id + ':' + c.messages.length + ':' + c.unread; }).join(','),
        s.schedule.map(function (t) { return (t.done ? '1' : '0') + t.id + t.title; }).join('|'),
        s.music.index, s.music.playing, Math.round(s.music.position), s.music.tracks.length, s.music.mode,
        normalizeWeather(s.rawWeather || s.weather).kind, normalizeWeather(s.rawWeather || s.weather).temp_c, s.city,
        s.location ? (s.location.area || s.location.lat) : 'no-loc',
        s.to ? s.to.name : '', s.from ? s.from.name : '',
        s.minimized, s.mode, s.toast, s.call ? Math.round(s.call.sec) : ''
      ].join('~');
      if (sigs.main === sig) { refreshStatusOnly(); return; }
      sigs.main = sig;

      dom.status.innerHTML = statusBarHTML(s);
      if (s.minimized) {
        dom.body.innerHTML = minCardHTML(s);
      } else {
        dom.body.innerHTML = renderScreen(s.app === 'home' ? 'home' : s.app, s);
      }
      if (!s.minimized) {
        dom.canvas = dom.body.querySelector('.ph-mapcanvas');
        drawCanvas();
      }
    }

    /** 只刷新状态栏时间（每秒，避免整屏重建） */
    function refreshStatusOnly() {
      if (!dom.status) return;
      var hhmm = makeStatus({ time: state.time }).hhmm;
      var el = dom.status.querySelector('.ph-status-time');
      if (el && el.textContent !== hhmm) el.textContent = hhmm;
    }

    function minCardHTML(s) {
      var w = normalizeWeather(s.rawWeather || s.weather);
      var next = (s.schedule || []).filter(function (t) { return !t.done; })[0];
      return '<div class="ph-min-card">' +
        '<div class="c1">' + esc(w.icon) + '</div>' +
        '<div class="c2">' + esc(tempText(w)) + ' ' + esc(w.kindLabel) + '</div>' +
        '<div class="c3">' + esc(makeStatus({ time: s.time }).hhmm) + ' · ' + esc(next ? next.title : '暂无待办') + '</div>' +
        '<div class="c3">点一下展开</div></div>';
    }

    function drawCanvas() {
      if (!dom.canvas) return;
      var payload = null;
      if (state.app === 'map' && state.plan) {
        payload = { route: (state.planOptions[state.planPick] || state.plan), from: state.from, to: state.to, trip: state.trip };
      } else if (state.app === 'taxi') {
        var r = state.ride;
        payload = { route: state.planOptions[state.planPick] || state.plan || (r.to ? { from: r.from, to: r.to, mode: 'taxi' } : null),
          from: r.from || state.from, to: r.to || state.to };
        if (r.status === 'arriving' || r.status === 'accepted' || r.status === 'onboard' || r.status === 'done') {
          // 车辆位置：在起点→终点之间按 progress 插值（演示用）
          var a = r.from || state.from, b = r.to || state.to;
          if (a && b) {
            var k = clamp(r.progress, 0, 1);
            payload.trip = { position: [num(a.lng) + (num(b.lng) - num(a.lng)) * k, num(a.lat) + (num(b.lat) - num(a.lat)) * k] };
          }
        }
      } else if (state.app === 'transit' && (state.transitOptions.length || state.stops.length)) {
        var src = state.transitOptions[0] || null;
        var pts = state.stops.slice(0, 6).map(function (s) { return [s.lng, s.lat]; });
        payload = src ? { route: src, from: state.from || { lng: 113.2640, lat: 23.1290 }, to: state.transitToPoint || state.to } : null;
        if (!payload && pts.length > 1) {
          dom.canvas = dom.body.querySelector('.ph-mapcanvas');
        }
      }
      drawMiniMap(dom.canvas, payload, { width: dom.canvas.clientWidth || 300, height: 190, dpr: (typeof devicePixelRatio === 'number' ? devicePixelRatio : 1), pulse: pulsePhase() });
      state._canvasPayload = payload;
    }
    function pulsePhase() { return (Date.now() % 1600) / 1600; }

    // ── 布局：把壳放到右下角（小窗 / 叠层共用），拖动后按拖动结果定位 ──
    function layoutShell() {
      var el = dom.shell;
      if (!el) return;
      if (el._dragged) return;   // 用户拖过就不再自动定位
      el.style.left = '';
      el.style.top = '';
      el.style.right = '14px';
      el.style.bottom = '14px';
    }

    // ── 拖动（悬浮按钮 & 手机壳把手共用） ──
    function startDrag(e, el, kind) {
      if (e.button !== undefined && e.button !== 0) return;
      var rect = el.getBoundingClientRect();
      dragging = {
        kind: kind, el: el, id: e.pointerId,
        startX: e.clientX, startY: e.clientY,
        offX: e.clientX - rect.left, offY: e.clientY - rect.top,
        moved: false
      };
      if (el.setPointerCapture) { try { el.setPointerCapture(e.pointerId); } catch (err) { } }
    }
    function moveDrag(e) {
      if (!dragging || e.pointerId !== dragging.id) return;
      var dx = e.clientX - dragging.startX, dy = e.clientY - dragging.startY;
      if (!dragging.moved && Math.abs(dx) < 8 && Math.abs(dy) < 8) return;   // 8px 阈值：不误触
      dragging.moved = true;
      var el = dragging.el;
      var w = el.offsetWidth || 56, h = el.offsetHeight || 56;
      var x = clamp(e.clientX - dragging.offX, 4, Math.max(4, window.innerWidth - w - 4));
      var y = clamp(e.clientY - dragging.offY, 4, Math.max(4, window.innerHeight - h - 4));
      el.style.left = Math.round(x) + 'px';
      el.style.top = Math.round(y) + 'px';
      el.style.right = 'auto';
      el.style.bottom = 'auto';
      if (dragging.kind === 'shell') el._dragged = true;
    }
    function endDrag(e) {
      if (!dragging) return;
      var d = dragging; dragging = null;
      if (d.el.releasePointerCapture && e && e.pointerId !== undefined) {
        try { d.el.releasePointerCapture(e.pointerId); } catch (err) { }
      }
      if (!d.moved) {
        // 视为点击
        if (d.kind === 'launcher') toggleOpen();
        else if (d.kind === 'shell') { /* 点击把手不做事，避免误触 */ }
      } else {
        saveLauncherPos();
      }
    }
    function saveLauncherPos() {
      if (!dom.launcher || typeof localStorage === 'undefined') return;
      try {
        localStorage.setItem('ph-launcher-pos', JSON.stringify({
          x: dom.launcher.style.left, y: dom.launcher.style.top
        }));
      } catch (e) { }
    }
    function restoreLauncherPos() {
      if (!dom.launcher || typeof localStorage === 'undefined') return false;
      try {
        var raw = localStorage.getItem('ph-launcher-pos');
        if (!raw) return false;
        var p = JSON.parse(raw);
        if (p && p.x && p.y) {
          dom.launcher.style.left = p.x; dom.launcher.style.top = p.y;
          dom.launcher.style.right = 'auto'; dom.launcher.style.bottom = 'auto';
          return true;
        }
      } catch (e) { }
      return false;
    }

    // ── 交互：事件委托 ──
    function readInput(field) {
      if (!dom.body) return '';
      var el = dom.body.querySelector('[data-field="' + field + '"]');
      return el ? String(el.value || '') : '';
    }
    function clearInput(field) {
      var el = dom.body && dom.body.querySelector('[data-field="' + field + '"]');
      if (el) el.value = '';
    }

    function onClick(e) {
      var el = e.target;
      while (el && el !== dom.root && !(el.getAttribute && el.getAttribute('data-act'))) el = el.parentNode;
      if (!el || el === dom.root || !el.getAttribute) return;
      var act = el.getAttribute('data-act');
      var app = el.getAttribute('data-app');
      var id = el.getAttribute('data-id');
      var idx = num(el.getAttribute('data-idx'), 0);
      e.preventDefault();
      e.stopPropagation();
      handle(act, { app: app, id: id, idx: idx, mode: el.getAttribute('data-mode'), el: el });
    }

    function handle(act, ctx) {
      var s = state;
      ctx = ctx || {};
      switch (act) {
        case 'app':
          if (ctx.app === 'home') { setApp('home'); return; }
          setApp(ctx.app);
          if (ctx.app === 'weather' && !s.ready.weather) fetchWeather();
          if (ctx.app === 'transit' && !s.stops.length) fetchStops();
          return;
        case 'locate': fetchLocation(); return;
        case 'pick-place': {
          var p = findById(s.places, ctx.id);
          if (!p) return;
          if (s.app === 'taxi') { s.ride.to = p; s.ride.toName = p.name; patch({}); }
          else { s.to = p; patch({}); planRoute(); }
          return;
        }
        case 'route': planRoute(); return;
        case 'pick-option':
          patch({ planPick: ctx.idx });
          render();
          return;
        case 'start-trip': startTrip(); return;
        case 'stop-trip': patch({ trip: null }); toast('已结束导航'); return;
        case 'goto-taxi': {
          if (s.to) { s.ride.to = s.to; s.ride.toName = s.to.name; }
          setApp('taxi');
          return;
        }
        /* 打车 */
        case 'ride-place': {
          var rp = findById(s.places, ctx.id); if (!rp) return;
          s.ride.to = rp; s.ride.toName = rp.name; patch({});
          return;
        }
        case 'ride-call': callRide(); return;
        case 'ride-advance': stepRide(true); return;
        case 'ride-cancel': cancelRide(); return;
        case 'ride-reset': patch({ ride: createRide({}) }); toast('已重置'); return;
        case 'call-driver': toast('正在呼叫 ' + (s.ride.driver ? s.ride.driver.name : '司机') + '…'); return;
        /* 公交地铁 */
        case 'stops-refresh': fetchStops(); return;
        case 'stop-from': {
          var sf = findById(s.stops, ctx.id); if (!sf) return;
          s.transitFrom = sf.name; s.from = { id: sf.id, name: sf.name, lat: sf.lat, lng: sf.lng };
          s.ride.from = s.from; patch({}); toast('起点：' + sf.name);
          return;
        }
        case 'stop-to': {
          var stt = findById(s.stops, ctx.id); if (!stt) return;
          s.transitTo = stt.name; s.to = { id: stt.id, name: stt.name, lat: stt.lat, lng: stt.lng };
          s.ride.to = s.to; s.ride.toName = s.to.name; patch({}); toast('终点：' + stt.name);
          return;
        }
        case 'transit-plan': planTransit(); return;
        /* 通讯 */
        case 'open-chat': openChat(ctx.id); return;
        case 'close-chat': patch({ openChat: null }); return;
        case 'send-msg': sendMessage(); return;
        case 'call-chat': startCall(ctx.id); return;
        case 'call-hangup': hangup(); return;
        /* 日程 */
        case 'todo-toggle': toggleTodo(ctx.id); return;
        case 'todo-add': addTodo(); return;
        /* 天气 */
        case 'weather-refresh': fetchWeather(readInput('city')); return;
        case 'weather-city': fetchWeather(''); return;
        /* 音乐 */
        case 'music-toggle': musicToggle(s.music); patch({}); return;
        case 'music-next': musicNext(s.music); patch({}); return;
        case 'music-prev': musicPrev(s.music); patch({}); return;
        case 'music-pick': s.music.index = clamp(ctx.idx, 0, s.music.tracks.length - 1); s.music.position = 0; s.music.playing = true; patch({}); return;
        case 'music-mode': s.music.mode = ctx.mode || 'list'; patch({}); return;
        /* 形态切换（供宿主页面/演示按钮调用） */
        case 'toggle-mode': cycleMode(); return;
        case 'toggle-min': minimize(!s.minimized); return;
        case 'close': close(); return;
        default:
          return;
      }
    }

    function findById(list, id) {
      for (var i = 0; i < (list || []).length; i++) if (String(list[i].id) === String(id)) return list[i];
      return null;
    }

    // ── 打开 / 收起 / 形态 ──
    function open() {
      state.open = true; state.minimized = false;
      if (dom.shell) dom.shell._dragged = false;    // 未 mount 时（Node 自检）允许纯状态调用
      patch({}); emit('open', state);
    }
    function close() { state.open = false; patch({}); emit('close', state); }
    function toggleOpen() { state.open ? close() : open(); }
    function minimize(v) {
      state.minimized = v === undefined ? !state.minimized : !!v;
      if (!state.minimized) state.open = true;
      patch({});
    }
    function cycleMode() {
      state.mode = state.mode === 'sheet' ? 'corner' : 'sheet';
      if (dom.shell) dom.shell._dragged = false;
      patch({});
      toast(state.mode === 'sheet' ? '半透明叠层形态' : '角落小窗形态');
    }
    function setMode(m) {
      state.mode = m === 'corner' ? 'corner' : 'sheet';
      if (dom.shell) dom.shell._dragged = false;
      patch({});
    }
    function setCollapsed(v) { state.collapsed = v === undefined ? !state.collapsed : !!v; patch({}); }

    // ── 数据接口（LingChat 预留） ──
    function setContacts(list) {
      var n = normalizeContacts(list);
      if (!n) { toast('联系人数据为空，保留示例'); return false; }
      state.contacts = n;
      if (state.openChat && !findContact(n, state.openChat)) state.openChat = null;
      patch({});
      toast('已接入 ' + n.length + ' 位联系人');
      return true;
    }
    function setSchedule(list) {
      var n = normalizeSchedule(list);
      if (!n) { toast('日程数据为空，保留示例'); return false; }
      state.schedule = n; patch({}); toast('已接入 ' + n.length + ' 项日程'); return true;
    }
    function setTracks(list) {
      var n = normalizeTracks(list);
      if (!n) { toast('曲目数据为空，保留示例'); return false; }
      state.music.tracks = n; state.music.index = 0; state.music.position = 0;
      state.tracksSource = 'lingchat';
      patch({}); toast('已接入 ' + n.length + ' 首曲目'); return true;
    }
    function setPlaces(list) {
      if (!Array.isArray(list) || !list.length) return false;
      state.places = list.map(function (p, i) {
        return { id: String(p.id || ('p' + i)), name: String(p.name || ('地点' + i)), lat: num(p.lat), lng: num(p.lng) };
      });
      patch({}); return true;
    }
    function setWeather(raw) {
      state.rawWeather = raw || null;
      state.weather = normalizeWeather(raw || {});
      state.ready.weather = true;
      patch({}); return state.weather;
    }
    function setStatus(part) {
      patch(part || {});
    }
    /** 屏内提示（演示用，不改业务状态） */
    function notify(msg) { toast(msg); }

    // ── 网络 ──
    function api(path, params) {
      var url = state.api + path;
      if (params) {
        var qs = [];
        for (var k in params) if (params[k] !== undefined && params[k] !== null && params[k] !== '') {
          qs.push(encodeURIComponent(k) + '=' + encodeURIComponent(params[k]));
        }
        if (qs.length) url += (url.indexOf('?') >= 0 ? '&' : '?') + qs.join('&');
      }
      if (typeof fetch !== 'function') return Promise.reject(new Error('no-fetch'));
      return fetch(url, { cache: 'no-store' }).then(function (r) { return r.json(); });
    }

    function fetchTime() {
      return api('/api/time').then(function (d) {
        state.time = makeStatus({ time: d });
        state.ready.time = true;
        patch({});
        return state.time;
      }).catch(function () {
        state.time = makeStatus({ time: {} });
        state.network = 'time-fail';
        patch({});
      });
    }
    function fetchWeather(city) {
      var c = city === undefined ? state.city : city;
      state.city = c || '';
      return api('/api/weather', c ? { city: c } : null).then(function (d) {
        setWeather(d);
        if (d && d.error) toast('天气获取失败');
        return state.weather;
      }).catch(function (e) {
        setWeather({ error: String(e && e.message || e) });
      });
    }
    function fetchLocation() {
      toast('正在定位…');
      return api('/api/location').then(function (d) {
        if (!d || d.error || !isFinite(num(d.lat, NaN))) {
          state.network = 'loc-fail';
          toast('定位失败：' + ((d && d.error) || '无坐标'));
          return null;
        }
        state.location = d;
        state.ready.location = true;
        state.from = { id: 'me', name: d.area || '当前位置', lat: num(d.lat), lng: num(d.lng) };
        if (!state.to) state.ride.from = state.from;
        patch({});
        toast('已定位：' + (d.area || (d.lat + ',' + d.lng)));
        return d;
      }).catch(function (e) {
        state.network = 'loc-fail';
        toast('定位失败：' + (e && e.message || e));
        return null;
      });
    }
    /**
     * 保证有起点：优先 /api/location；定位失败或网络异常时，退回第一个常用地点。
     * 任何情况下都 resolve 一个带坐标的起点——否则叫车/规划会静默卡死。
     */
    function ensureLocation() {
      if (state.from && isFinite(num(state.from.lat, NaN))) return Promise.resolve(state.from);
      return fetchLocation().catch(function () { return null; }).then(function () {
        if (state.from && isFinite(num(state.from.lat, NaN))) return state.from;
        var p = state.places[0] || DEMO_PLACES[0];
        state.from = { id: p.id, name: '（兜底）' + p.name, lat: p.lat, lng: p.lng };
        toast('未取到定位，暂用 ' + p.name + ' 作为起点');
        return state.from;
      });
    }
    function planRoute() {
      var name = readInput('to');
      var to = null;
      if (name) {
        var byName = null;
        for (var i = 0; i < state.places.length; i++) if (state.places[i].name === name) byName = state.places[i];
        to = byName || state.to || null;
        if (byName) state.to = byName;
        if (!byName && state.to && state.to.name !== name) to = null;
      } else {
        to = state.to;
      }
      if (!to) { toast('请先选择或输入目的地'); return Promise.resolve(null); }
      state.planLoading = true; state.planError = '';
      patch({});
      return ensureLocation().then(function (from) {
        return api('/api/transport_plan', {
          from_lng: from.lng, from_lat: from.lat, to_lng: to.lng, to_lat: to.lat
        });
      }).then(function (d) {
        state.planLoading = false;
        if (!d || d.ok === false || !d.route) {
          state.planError = (d && d.error) || '规划失败';
          patch({});
          return null;
        }
        state.plan = d.route;
        state.planOptions = d.options || [];
        state.planPick = 0;
        patch({});
        toast('已生成 ' + state.planOptions.length + ' 条方案');
        return d;
      }).catch(function (e) {
        state.planLoading = false;
        state.planError = String(e && e.message || e);
        patch({});
        return null;
      });
    }
    function startTrip() {
      var sel = state.planOptions[state.planPick] || state.plan;
      if (!sel) { toast('请先规划路线'); return; }
      state.trip = {
        from: sel.from, to: sel.to, mode: sel.mode, mode_name: sel.mode_name,
        distance_m: sel.distance_m, distance_text: sel.distance_text,
        duration_min: sel.duration_min, duration_text: sel.duration_text,
        remaining_m: sel.distance_m, remaining_min: sel.duration_min,
        remaining_text: sel.duration_text, status: 'moving', progress: 0.02,
        current_speed_kmh: sel.speed_kmh || 20, cost: sel.cost, cost_text: sel.cost_text,
        current_phase: 0, phases: (sel.steps || []).map(function (s) {
          return { kind: s.mode === 'walk' ? 'walk' : 'ride', mode: s.mode, mode_name: s.mode_name, from: s.from, to: s.to, note: s.note };
        }),
        position: sel.from, label: '导航中', summary: sel.summary
      };
      patch({});
      toast('开始导航');
    }
    function tickTrip() {
      var t = state.trip;
      if (!t || t.status !== 'moving') return;
      var total = num(t.duration_min, 1);
      var step = Math.max(0.5, total / 60);      // 1 秒推进约 1/60 行程（演示加速）
      t.progress = clamp(num(t.progress) + 1 / 60, 0, 1);
      t.remaining_min = Math.max(0, total * (1 - t.progress));
      t.remaining_text = fmtMinutes(t.remaining_min);
      t.remaining_m = num(t.distance_m) * (1 - t.progress);
      var a = t.from, b = t.to;
      if (a && b) t.position = [num(a[0]) + (num(b[0]) - num(a[0])) * t.progress, num(a[1]) + (num(b[1]) - num(a[1])) * t.progress];
      t.current_phase = Math.min((t.phases || []).length - 1, Math.floor(t.progress * Math.max(1, (t.phases || []).length)));
      if (t.progress >= 1) { t.status = 'arrived'; t.remaining_text = '已到达'; toast('已到达目的地'); }
      scheduleRender();
    }

    // ── 打车流程 ──
    function callRide() {
      var name = readInput('rideTo');
      var to = state.ride.to;
      if (name) {
        var byName = null;
        for (var i = 0; i < state.places.length; i++) if (state.places[i].name === name) byName = state.places[i];
        if (byName) to = byName;
      }
      if (!to || !isFinite(num(to.lat, NaN))) { toast('请先选择目的地'); return; }
      // 记住目的地（输入框里可能只有名字；schedule 里的对象若没有坐标则不可用）
      state.ride.to = to;
      state.ride.toName = to.name || state.ride.toName;
      if (state.app === 'taxi') { /* 立即回显目的地，避免等待异步定位时界面无变化 */ patch({}); }

      // 派单：拿到起点后计算里程/时长/预估价并进入 matching
      function dispatch(from) {
        var dist = haversineM([from.lng, from.lat], [to.lng, to.lat]);
        var min = Math.max(4, Math.round(dist / 1000 / 28 * 60 + 4));
        var price = 10 + dist / 1000 * 2.6;
        state.ride = createRide({
          from: from, to: to, totalMin: min, price: price, distance_m: dist,
          priceText: fmtCost(price)
        });
        state.from = from;
        rideTransition(state.ride, 'matching');
        patch({});
        toast('正在为你寻找附近车辆…');
      }
      // 已有起点就不再打一次 /api/location（省一次网络往返）
      if (state.from && isFinite(num(state.from.lat, NaN))) { dispatch(state.from); return; }
      ensureLocation().then(dispatch).catch(function (e) {   // 兜底：任何异常都不能让叫车静默卡死
        toast('叫车失败：' + (e && e.message || e));
        patch({});
      });
    }
    /**
     * 推进打车流程。
     * 自动（manual=false）：每 1.2 秒走一次 rideStep（派单倒计时 / 车辆接近 / 行程推进）。
     * 手动（manual=true）：必须保证「点一下就有变化」——派单态连点几次必定接单，
     *   其余状态直接跨到下一状态，便于演示与人工验证，绝不出现「点了没反应」。
     */
    function stepRide(manual) {
      var r = state.ride;
      if (!r) return;
      var res = rideStep(r, 1);
      // 手动推进：必须保证「点一下就有变化」
      if (manual) {
        if (r.status === 'matching') {
          // 派单态：连点到司机接单（最多 4 次，等价于 3~5 秒的派单过程）
          for (var i = 0; i < 4 && r.status === 'matching'; i++) res = rideStep(r, 1);
        } else if (!res.changed) {
          // 其余状态：直接跨到下一状态，便于演示与人工验证
          var cur = RIDE_ORDER.indexOf(r.status);
          if (cur >= 0 && cur < RIDE_ORDER.indexOf('done')) {
            var nxt = RIDE_ORDER[cur + 1];
            var tr = rideTransition(r, nxt, {});
            if (tr.ok) {
              res = { changed: true, status: nxt, event: nxt === 'onboard' ? 'onboard' : (nxt === 'done' ? 'done' : null) };
            }
          }
        }
      }
      if (res.event === 'accepted') toast('司机已接单：' + (r.driver ? r.driver.name + ' ' + r.driver.car : ''));
      if (res.event === 'onboard') toast('车辆已到达，行程开始');
      if (res.event === 'done') toast('已到达目的地，' + r.priceText);
      patch({});
    }
    function cancelRide() {
      var tr = rideTransition(state.ride, 'canceled');
      if (!tr.ok) { toast('当前状态不可取消'); return; }
      patch({}); toast('已取消叫车');
    }

    // ── 公交/地铁 ──
    function fetchStops() {
      toast('正在获取附近站点…');
      return ensureLocation().then(function (from) {
        return api('/api/facilities', { area: (state.location && state.location.area) || '越秀区', count: 20 }).then(function (d) {
          state.stops = nearbyTransit(d, { lat: from.lat, lng: from.lng }, { limit: 10 });
          state.stopsFallback = !!(state.stops[0] && state.stops[0].fallback);
          patch({});
          toast('附近站点 ' + state.stops.length + ' 个');
          return state.stops;
        });
      }).catch(function (e) {
        state.stops = nearbyTransit(null, null, {});
        patch({});
        toast('站点接口失败，用示例数据');
        return state.stops;
      });
    }
    function planTransit() {
      var name = readInput('transitTo');
      var to = null;
      if (name) {
        for (var i = 0; i < state.stops.length; i++) if (state.stops[i].name === name) to = state.stops[i];
        for (var j = 0; j < state.places.length; j++) if (state.places[j].name === name) to = state.places[j];
      }
      if (!to) to = state.to || state.stops[0];
      if (!to) { toast('请选择终点（或先拉取附近站点）'); return Promise.resolve(null); }
      state.transitTo = to.name;
      state.to = { id: to.id, name: to.name, lat: to.lat, lng: to.lng };
      state.transitError = '';
      patch({});
      return ensureLocation().then(function (from) {
        return api('/api/transport_plan', { from_lng: from.lng, from_lat: from.lat, to_lng: to.lng, to_lat: to.lat });
      }).then(function (d) {
        if (!d || d.ok === false) {
          state.transitError = (d && d.error) || '查询失败';
          patch({});
          return null;
        }
        var all = (d.options || []).concat(d.route ? [d.route] : []);
        var seen = {};
        var pub = all.filter(function (o) {
          var k = o.mode + '|' + Math.round(o.duration_min);
          if (PUBLIC_MODES.indexOf(o.mode) < 0) return false;
          if (seen[k]) return false;
          seen[k] = 1; return true;
        });
        state.transitOptions = pub;
        state.plan = d.route; state.planOptions = d.options || []; state.planPick = 0;
        patch({});
        toast(pub.length ? ('公交/地铁方案 ' + pub.length + ' 条') : '本次行程无公交/地铁直达方案');
        return pub;
      }).catch(function (e) {
        state.transitError = String(e && e.message || e);
        patch({});
        return null;
      });
    }
    var PUBLIC_MODES = ['bus', 'subway', 'walk', 'train', 'ferry'];

    // ── 通讯 ──
    function openChat(id) {
      var c = findContact(state.contacts, id);
      if (!c) return;
      var before = state.openChat;
      state.openChat = c.id;
      if (c.unread) c.unread = 0;
      patch({});
      if (before !== c.id) emit('chat-open', c);
    }
    function sendMessage() {
      var c = findContact(state.contacts, state.openChat);
      if (!c) return;
      var text = readInput('msg').trim();
      if (!text) { toast('消息不能为空'); return; }
      c.messages.push({ id: 'm' + Date.now(), from: 'me', text: text, t: makeStatus({ time: state.time }).hhmm });
      clearInput('msg');
      patch({});
      emit('message-sent', { contact: c, text: text });
      // 演示：2 秒后给一条自动回复（宿主可用 setContacts 覆盖真实数据）
      setTimeout(function () {
        if (destroyed) return;
        if (findContact(state.contacts, c.id) !== c) return;
        var replies = ['收到～', '好的，记下了。', '我看一下，稍后回复你。', '嗯嗯，就这样办。'];
        c.messages.push({ id: 'm' + Date.now(), from: 'them', text: replies[c.messages.length % replies.length], t: makeStatus({ time: state.time }).hhmm });
        if (state.openChat !== c.id) c.unread = num(c.unread) + 1;
        patch({});
      }, 2000);
    }
    function startCall(id) {
      var c = findContact(state.contacts, id);
      if (!c) return;
      state.call = { contactId: c.id, name: c.name, sec: 0 };
      patch({});
      toast('正在呼叫 ' + c.name + '…');
      emit('call-start', { contact: c });
    }
    function hangup() {
      if (!state.call) return;
      var sec = state.call.sec, name = state.call.name;
      state.call = null;
      patch({});
      toast('通话结束 · ' + name + ' · ' + fmtClock(sec));
      emit('call-end', { name: name, sec: sec });
    }
    function tickCall() {
      if (!state.call) return;
      state.call.sec += 1;
      scheduleRender();
    }

    // ── 日程 ──
    function toggleTodo(id) {
      var t = null;
      for (var i = 0; i < state.schedule.length; i++) if (String(state.schedule[i].id) === String(id)) t = state.schedule[i];
      if (!t) return;
      t.done = !t.done;
      patch({});
      emit('todo-toggle', t);
    }
    function addTodo() {
      var text = readInput('todo').trim();
      if (!text) { toast('请输入待办内容'); return; }
      var m = text.match(/^\s*(\d{1,2}[:：]\d{2})\s*(.*)$/);
      var item = {
        id: 't' + Date.now(), time: m ? m[1].replace('：', ':') : '', title: m ? (m[2] || text) : text,
        detail: '', tag: '新增', done: false
      };
      state.schedule.push(item);
      clearInput('todo');
      patch({});
      toast('已添加：' + item.title);
      emit('todo-add', item);
    }

    // ── 挂载 / 卸载 ──
    function mount(target) {
      var doc = (target && target.ownerDocument) || (typeof document !== 'undefined' ? document : null);
      if (!doc) return null;
      injectCSS(doc);
      var host = target || doc.body;
      if (dom.root) return apiPublic;

      var launcher = doc.createElement('div');
      launcher.className = 'ph-launcher';
      launcher.setAttribute('role', 'button');
      launcher.setAttribute('tabindex', '0');
      launcher.innerHTML = '📱';
      launcher.addEventListener('pointerdown', function (e) { startDrag(e, launcher, 'launcher'); });
      launcher.addEventListener('pointermove', moveDrag);
      launcher.addEventListener('pointerup', endDrag);
      launcher.addEventListener('pointercancel', endDrag);
      launcher.addEventListener('keydown', function (e) {
        if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggleOpen(); }
      });
      launcher.addEventListener('dblclick', function (e) { e.preventDefault(); setCollapsed(!state.collapsed); });

      var shell = doc.createElement('div');
      shell.className = 'ph-shell ph-mode-' + state.mode;
      shell.innerHTML = '<div class="ph-status ph-grip" data-grip="1"></div><div class="ph-body"></div>';
      var status = shell.querySelector('.ph-status');
      var body = shell.querySelector('.ph-body');
      status.addEventListener('pointerdown', function (e) { startDrag(e, shell, 'shell'); });
      status.addEventListener('pointermove', moveDrag);
      status.addEventListener('pointerup', endDrag);
      status.addEventListener('pointercancel', endDrag);
      shell.addEventListener('click', onClick, true);
      shell.addEventListener('keydown', function (e) {
        if (e.key === 'Enter' && e.target && e.target.getAttribute && e.target.getAttribute('data-field')) {
          var f = e.target.getAttribute('data-field');
          if (f === 'msg') sendMessage();
          if (f === 'todo') addTodo();
          if (f === 'to') planRoute();
          if (f === 'transitTo') planTransit();
          if (f === 'rideTo') callRide();
          if (f === 'city') fetchWeather(readInput('city'));
        }
      });

      host.appendChild(launcher);
      host.appendChild(shell);

      var toastEl = doc.createElement('div');
      toastEl.className = 'ph-toast';
      host.appendChild(toastEl);

      dom.root = host; dom.launcher = launcher; dom.shell = shell;
      dom.status = status; dom.body = body; dom.toast = toastEl;
      restoreLauncherPos();
      render();

      // 时钟：每秒刷新状态栏；每 60 秒与 /api/time 对表
      if (typeof setInterval === 'function') {
        timers.clock = setInterval(function () {
          if (destroyed) return;
          state.time = makeStatus({ time: state.time || {} });
          tickCall();
          tickTrip();
          refreshStatusOnly();
          if (state.app === 'music' && state.music.playing) {
            musicTick(state.music, 2);
            // 只更新进度条，避免整屏重建打断点击
            var bar = dom.body.querySelector('.ph-progress i');
            var tm = dom.body.querySelector('.ph-progress-time');
            if (bar) bar.style.width = pct(musicProgress(state.music)) + '%';
            if (tm) tm.innerHTML = '<span>' + esc(fmtClock(state.music.position)) + '</span><span>' + esc(fmtClock(currentTrack(state.music) ? currentTrack(state.music).duration : 0)) + '</span>';
            else scheduleRender();
          }
          if (state.open && (state.app === 'map' || state.app === 'taxi') && state.ride.status !== 'idle') drawCanvas();
        }, 1000);
        timers.fast = setInterval(function () { if (!destroyed) stepRide(false); }, 1200);
        timers.slow = setInterval(function () {
          if (destroyed) return;
          if (state.time) {
            // 与服务器对表
            api('/api/time').then(function (d) { state.time = makeStatus({ time: d }); refreshStatusOnly(); }).catch(function () { });
          }
        }, 60000);
      }

      // 电量（有 Battery API 就用真实的）
      if (typeof navigator !== 'undefined' && navigator.getBattery) {
        navigator.getBattery().then(function (b) {
          var sync = function () {
            state.battery = { level: Math.round(b.level * 100), charging: !!b.charging };
            patch({});
          };
          sync();
          b.addEventListener && b.addEventListener('levelchange', sync);
          b.addEventListener && b.addEventListener('chargingchange', sync);
        }).catch(function () { });
      }
      if (typeof navigator !== 'undefined' && navigator.connection && navigator.connection.effectiveType) {
        state.signal = { bars: 4, type: navigator.connection.effectiveType };
      }
      bound = true;
      return apiPublic;
    }

    function destroy() {
      destroyed = true;
      for (var k in timers) if (timers[k]) { clearInterval(timers[k]); clearTimeout(timers[k]); timers[k] = null; }
      if (dom.launcher && dom.launcher.parentNode) dom.launcher.parentNode.removeChild(dom.launcher);
      if (dom.shell && dom.shell.parentNode) dom.shell.parentNode.removeChild(dom.shell);
      if (dom.toast && dom.toast.parentNode) dom.toast.parentNode.removeChild(dom.toast);
      dom = { root: null, launcher: null, shell: null, body: null, status: null, toast: null, canvas: null };
      bound = false;
    }

    function unreadTotal() {
      var n = 0;
      (state.contacts || []).forEach(function (c) { n += num(c.unread, 0); });
      return n;
    }

    var apiPublic = {
      // 状态
      state: state,
      get app() { return state.app; },
      get mode() { return state.mode; },
      get isOpen() { return state.open; },
      // 生命周期
      mount: mount, destroy: destroy, render: render,
      // 开关与形态
      open: open, close: close, toggle: toggleOpen, minimize: minimize,
      cycleMode: cycleMode, setMode: setMode, setCollapsed: setCollapsed,
      // 数据（LingChat 预留）
      setContacts: setContacts, setSchedule: setSchedule, setTracks: setTracks,
      setPlaces: setPlaces, setWeather: setWeather, setStatus: setStatus, notify: notify,
      // 业务动作（宿主页面可直接调，演示页按钮用）
      fetchTime: fetchTime, fetchWeather: fetchWeather, fetchLocation: fetchLocation,
      planRoute: planRoute, startTrip: startTrip,
      callRide: callRide, stepRide: stepRide, cancelRide: cancelRide,
      fetchStops: fetchStops, planTransit: planTransit,
      openChat: openChat, sendMessage: sendMessage, startCall: startCall, hangup: hangup, tickCall: tickCall,
      toggleTodo: toggleTodo, addTodo: addTodo,
      music: musicToggle, musicNext: musicNext, musicPrev: musicPrev, currentTrack: currentTrack,
      // 事件
      on: on, off: off,
      // 供高级用法
      setApp: setApp, handle: handle
    };
    return apiPublic;
  }

  // ══════════════════════════════════════════════════════════════════════
  // 11. 导出
  // ══════════════════════════════════════════════════════════════════════

  return {
    version: 'T5-1~T5-8',
    DEFAULT_API: DEFAULT_API,
    APPS: APPS, APP_TITLES: APP_TITLES,
    DEMO_CONTACTS: DEMO_CONTACTS, DEMO_SCHEDULE: DEMO_SCHEDULE,
    DEMO_TRACKS: DEMO_TRACKS, DEMO_STOPS: DEMO_STOPS, DEMO_PLACES: DEMO_PLACES,
    DEMO_DRIVERS: DEMO_DRIVERS,
    // 工具
    esc: esc, fmtDistance: fmtDistance, fmtMinutes: fmtMinutes, fmtClock: fmtClock,
    fmtCost: fmtCost, haversineM: haversineM, clamp: clamp, pad2: pad2,
    // 状态栏
    makeStatus: makeStatus, periodOf: periodOf, batteryInfo: batteryInfo, signalInfo: signalInfo,
    PERIOD_ZH: PERIOD_ZH,
    // 天气
    weatherKind: weatherKind, normalizeWeather: normalizeWeather, tempText: tempText,
    WEATHER_ICON: WEATHER_ICON, WEATHER_LABEL: WEATHER_LABEL,
    // 打车
    RIDE_ORDER: RIDE_ORDER, RIDE_NEXT: RIDE_NEXT, RIDE_LABEL: RIDE_LABEL,
    createRide: createRide, rideTransition: rideTransition, rideStep: rideStep, rideView: rideView,
    // 音乐
    createMusic: createMusic, musicToggle: musicToggle, musicNext: musicNext, musicPrev: musicPrev,
    musicTick: musicTick, musicProgress: musicProgress, currentTrack: currentTrack, trackAt: trackAt,
    // 数据适配
    normalizeContact: normalizeContact, normalizeContacts: normalizeContacts,
    normalizeScheduleItem: normalizeScheduleItem, normalizeSchedule: normalizeSchedule,
    normalizeTrack: normalizeTrack, normalizeTracks: normalizeTracks,
    nearbyTransit: nearbyTransit, stopLatLng: stopLatLng,
    // 地图
    projector: projector, mercator: mercator, routePoints: routePoints, drawMiniMap: drawMiniMap,
    // 渲染
    renderScreen: renderScreen, renderHomeScreen: renderHomeScreen, renderMapScreen: renderMapScreen,
    renderTaxiScreen: renderTaxiScreen, renderTransitScreen: renderTransitScreen,
    renderChatScreen: renderChatScreen, renderConversation: renderConversation,
    renderPlanScreen: renderPlanScreen, renderWeatherScreen: renderWeatherScreen,
    renderMusicScreen: renderMusicScreen, statusBarHTML: statusBarHTML, findContact: findContact,
    // 实例
    createPhone: createPhone,
    CSS: CSS, injectCSS: injectCSS
  };
});
