// ═══════════════════════════════════════════════════════════════════════════
// L-SYuki · NPC 系统 + 角色互动 (T4-3 / T4-4)
//
// 设计取向（严格遵循用户需求）：
//   1. 普通路人 = 「点」。几百上千个 2~4px 小圆点，canvas 批量绘制 + 视口裁剪，
//      不分性别不做头像；按性别/年龄段用极浅的灰/彩色区分。
//   2. 只有「重大事件聚焦」时才把相关 NPC 从点升级为「详情卡」（DOM 层，含名字/头像/气泡），
//      聚焦到期自动降级回点（focusNpc / unfocus / getFocused）。
//   3. 只有 LingChat 角色列表里的角色才拥有头像（setNamedCharacters），
//      它们「始终」显示头像且比路人大一圈，与聚焦机制完全独立。
//
// 依赖：无。可选配合 world_coord.js（网格→画布换算由调用方传入 toCanvasFn）。
// 导出：window.NPC_SYS
// ═══════════════════════════════════════════════════════════════════════════
window.NPC_SYS = (function () {
  'use strict';

  var VERSION = 'T4-3/T4-4 v1.0';
  var TAU = Math.PI * 2;
  var DAY = 1440; // 一天的分钟数

  // ─────────────────────────── 0. 配置 ───────────────────────────
  var config = {
    metersPerCell: 5,            // 每格米数（与 district_gen 的网格约定一致）
    walkSpeed: 1.4,              // 步行速度 m/s → 换算成 格/秒
    interactionDist: 2,          // 网格距离 < 2 触发互动
    interactionProb: 0.10,       // 陌生人：每秒互动概率
    interactionProbKnown: 0.45,  // 有关系的人：每秒互动概率
    interactionCooldownMs: 45000,// 同一对 NPC 的互动冷却
    maxInteractionsPerTick: 2,   // 单 tick 最多产生几条互动（保持事件流可读）
    focusDurationMs: 20000,      // 聚焦持续时间
    maxFocus: 4,                 // 同时聚焦上限
    maxEvents: 400,              // 事件环形缓冲上限
    autoInteract: true,          // tickNpcs 内自动跑互动检测
    crowdShape: 'auto',          // 'auto' | 'circle' | 'rect'（圆点/方块批量）
    circleShapeMax: 3000,        // auto 模式下超过此数量改用 fillRect（更快）
    viewportMargin: 24,          // 视口裁剪外扩像素（避免贴边闪烁）
    placeJitter: 1.2,            // 到达目的地后在原地小范围游荡的半径（格）
    maxTickDtSec: 2,             // 单次 tick 最多推进的游戏时间（防切后台回来瞬移）
    trackLength: 0,              // >0 时记录足迹点（默认关，省内存）
    overlayMinIntervalMs: 0,     // DOM 覆盖层最小刷新间隔（0=每帧）
  };
  function setConfig(o) {
    if (!o) return config;
    for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) config[k] = o[k];
    return config;
  }
  function getConfig() { return config; }

  // ─────────────────── 1. 随机数（可复现，便于测试） ───────────────────
  function makeRng(seed) {
    var s = (seed == null ? Date.now() : seed) >>> 0;
    return function () {
      s = (s + 0x6D2B79F5) >>> 0;
      var t = s;
      t = Math.imul(t ^ (t >>> 15), t | 1);
      t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }
  var _rng = makeRng(20260410);
  function rand() { return _rng(); }
  function randInt(a, b) { return a + Math.floor(_rng() * (b - a + 1)); }
  function pick(arr) { return arr[Math.floor(_rng() * arr.length)]; }
  function setSeed(s) { _rng = makeRng(s); }

  // ─────────────────────── 2. 路人点样式表 ───────────────────────
  // 都是「很小」的点：直径 2~4px。颜色极浅、低饱和，只在整体上形成人群色块差异。
  var DOT = {
    male_adult:   { color: '#7d9ab4', r: 2.0, label: '成年男性' },
    female_adult: { color: '#b08fa4', r: 2.0, label: '成年女性' },
    male_teen:    { color: '#8fb2c9', r: 1.8, label: '少年男性' },
    female_teen:  { color: '#c49cb2', r: 1.8, label: '少女' },
    male_child:   { color: '#a3c6dc', r: 1.5, label: '男童' },
    female_child: { color: '#d3b4c4', r: 1.5, label: '女童' },
    elder:        { color: '#9aa3ad', r: 1.7, label: '长者' },
    staff:        { color: '#8fd6ff', r: 2.1, label: '服务人员' },
    named:        { color: '#79d9ff', r: 2.6, label: 'LingChat 角色' },
    focused:      { color: '#ffc24d', r: 2.6, label: '聚焦中' },
  };
  function dotGroup(npc) {
    if (isFocused(npc.id)) return 'focused';
    if (npc.kind === 'named') return 'named';
    if (npc.role === 'staff') return 'staff';
    if (npc.ageGroup === 'elder') return 'elder';
    if (npc.ageGroup === 'child') return npc.gender === 'female' ? 'female_child' : 'male_child';
    if (npc.ageGroup === 'teen') return npc.gender === 'female' ? 'female_teen' : 'male_teen';
    return npc.gender === 'female' ? 'female_adult' : 'male_adult';
  }

  // ─────────────────────── 3. 时间工具 ───────────────────────
  // 与 hier_api.py 的 /api/time 字段完全兼容：{hour, minute, period, weekday_name, ...}
  function parseHM(s) {
    if (typeof s === 'number') return s;
    var p = String(s == null ? '0:00' : s).split(':');
    return (parseInt(p[0], 10) || 0) * 60 + (parseInt(p[1], 10) || 0);
  }
  function hm(mins) {
    var m = ((mins % DAY) + DAY) % DAY;
    var h = Math.floor(m / 60), mm = Math.floor(m % 60);
    return (h < 10 ? '0' : '') + h + ':' + (mm < 10 ? '0' : '') + mm;
  }
  function periodOf(h) {
    if (h >= 5 && h < 8) return 'dawn';
    if (h >= 8 && h < 11) return 'morning';
    if (h >= 11 && h < 14) return 'noon';
    if (h >= 14 && h < 17) return 'afternoon';
    if (h >= 17 && h < 19) return 'dusk';
    if (h >= 19 && h < 23) return 'evening';
    return 'night';
  }
  var PERIOD_ZH = {
    dawn: '拂晓', morning: '上午', noon: '正午', afternoon: '下午',
    dusk: '黄昏', evening: '夜晚', night: '深夜',
  };
  /** 把 数字小时 / /api/time 返回对象 / {hour,minute} 统一成 timeState */
  function normalizeTimeState(t) {
    var hour, minute, extra = {};
    if (t == null) {
      var d = new Date();
      hour = d.getHours(); minute = d.getMinutes();
      extra.weekday = d.getDay(); extra.source = 'local';
    } else if (typeof t === 'number') {
      hour = Math.floor(t); minute = Math.round((t - hour) * 60);
    } else if (typeof t === 'object') {
      if (t.hour != null) { hour = t.hour; minute = t.minute || 0; }
      else if (t.minutes != null) { hour = Math.floor(t.minutes / 60); minute = t.minutes % 60; }
      else { var n = new Date(); hour = n.getHours(); minute = n.getMinutes(); }
      for (var k in t) if (Object.prototype.hasOwnProperty.call(t, k)) extra[k] = t[k];
    } else { hour = 12; minute = 0; }
    var minutes = (hour * 60 + Math.min(59, Math.max(0, minute))) % DAY;
    var st = {
      hour: Math.floor(minutes / 60), minute: minutes % 60, minutes: minutes,
      time: hm(minutes), period: periodOf(minutes / 60), is_day: minutes / 60 >= 6 && minutes / 60 < 18,
      period_zh: PERIOD_ZH[periodOf(minutes / 60)],
    };
    for (var k2 in extra) if (Object.prototype.hasOwnProperty.call(extra, k2)) st[k2] = extra[k2];
    return st;
  }
  /** 简易 timeState 构造器（demo / 单测用） */
  function timeStateOf(hour, minute) { return normalizeTimeState({ hour: hour, minute: minute || 0 }); }
  /** 拉取 hier_api 的 /api/time，失败则退回本地时间 */
  function fetchTimeState(apiBase, timeoutMs) {
    var base = apiBase || 'http://127.0.0.1:8790';
    return new Promise(function (resolve) {
      var done = false, timer = setTimeout(function () {
        if (!done) { done = true; resolve(normalizeTimeState(null)); }
      }, timeoutMs || 4000);
      try {
        fetch(base + '/api/time').then(function (r) { return r.json(); }).then(function (d) {
          if (done) return; done = true; clearTimeout(timer);
          var st = normalizeTimeState(d); st.source = 'api';
          resolve(st);
        })['catch'](function () {
          if (done) return; done = true; clearTimeout(timer);
          resolve(normalizeTimeState(null));
        });
      } catch (e) {
        if (!done) { done = true; clearTimeout(timer); resolve(normalizeTimeState(null)); }
      }
    });
  }

  // ─────────────────────── 4. 地名 / 小区布局 ───────────────────────
  /** 内置默认小区布局：与 district_gen.gen_layout 返回结构一致，便于无 LLM 时演示 */
  function defaultLayout(size, seed) {
    size = size || 20;
    var rng = makeRng(seed == null ? 7 : seed);
    var ri = function (a, b) { return a + Math.floor(rng() * (b - a + 1)); };
    var L = { name: '示范小区', size: size, buildings: [], roads: [], parks: [], water: [] };
    // 主路：十字 + 每 6 格一条支路
    L.roads.push({ x1: 0, y1: Math.round(size / 2), x2: size, y2: Math.round(size / 2), type: 'main' });
    L.roads.push({ x1: Math.round(size / 2), y1: 0, x2: Math.round(size / 2), y2: size, type: 'main' });
    for (var i = 1; i * 6 < size; i++) {
      L.roads.push({ x1: 0, y1: i * 6, x2: size, y2: i * 6, type: 'secondary' });
      L.roads.push({ x1: i * 6, y1: 0, x2: i * 6, y2: size, type: 'secondary' });
    }
    // 公园 + 水体
    L.parks.push({ x: 2, y: 2, w: 4, h: 3, name: '中心公园' });
    L.water.push({ x: size - 6, y: size - 6, w: 4, h: 3, name: '景观湖' });
    // 建筑：住宅为主 + 写字楼/商铺/学校/医院
    var types = ['residential', 'residential', 'residential', 'commercial', 'shop', 'office'];
    for (var y = 1; y < size - 2; y += 3) {
      for (var x = 1; x < size - 2; x += 4) {
        if (x % 6 <= 1 && y % 6 <= 1) continue;              // 让开道路
        if (x >= 2 && x <= 6 && y >= 2 && y <= 5) continue;  // 让开公园
        if (x >= size - 6 && y >= size - 6) continue;        // 让开湖面
        var w = ri(2, 3), h = ri(2, 3);
        if (x + w >= size || y + h >= size) continue;
        var type = types[ri(0, types.length - 1)];
        L.buildings.push({ x: x, y: y, w: w, h: h, type: type, name: '' });
      }
    }
    // 关键设施（固定几栋，保证作息有去处）
    L.buildings.push({ x: 2, y: Math.round(size / 2) + 1, w: 3, h: 2, type: 'office', name: '云澜写字楼' });
    L.buildings.push({ x: Math.round(size / 2) + 1, y: 2, w: 3, h: 2, type: 'school', name: '示范小学' });
    L.buildings.push({ x: Math.round(size / 2) + 1, y: Math.round(size / 2) + 1, w: 3, h: 2, type: 'shop', name: '街角食堂' });
    L.buildings.push({ x: size - 6, y: 2, w: 3, h: 2, type: 'hospital', name: '社区卫生站' });
    // 给住宅编号
    var idx = 0;
    L.buildings.forEach(function (b) {
      if (!b.name) {
        if (b.type === 'residential') { idx++; b.name = idx + '号楼'; }
        else if (b.type === 'commercial') b.name = '沿街商住';
      }
    });
    return L;
  }

  /** 把 district_gen 的布局转成「地点池」：作息里 place 关键字 → 具体坐标 */
  function placesFromLayout(layout, size) {
    layout = layout || defaultLayout(size || 20);
    var sz = layout.size || size || 20;
    var pools = { home: [], work: [], food: [], shop: [], school: [], hospital: [], park: [], street: [] };
    var center = function (b) {
      return { gx: b.x + (b.w || 1) / 2, gy: b.y + (b.h || 1) / 2, name: b.name || '某处' };
    };
    (layout.buildings || []).forEach(function (b) {
      var p = center(b);
      p.type = b.type;
      var nm = b.name || '某处';
      if (b.type === 'residential') { p.name = nm; pools.home.push(p); }
      else if (b.type === 'office') { p.name = nm; pools.work.push(p); }
      else if (b.type === 'commercial') { p.name = nm; pools.food.push(p); }
      else if (b.type === 'shop') { p.name = nm; pools.food.push(p); pools.shop.push(p); }
      else if (b.type === 'school') { p.name = nm; pools.school.push(p); }
      else if (b.type === 'hospital') { p.name = nm; pools.hospital.push(p); }
    });
    (layout.parks || []).forEach(function (k) {
      pools.park.push({
        gx: k.x + (k.w || 2) / 2, gy: k.y + (k.h || 2) / 2,
        name: k.name || '公园', type: 'park',
      });
    });
    (layout.water || []).forEach(function (k) {
      pools.park.push({
        gx: k.x + (k.w || 2) / 2, gy: k.y + (k.h || 2) / 2,
        name: k.name || '水边', type: 'water',
      });
    });
    // 街道点：沿路取样（行人常在的地方）
    (layout.roads || []).forEach(function (r, i) {
      var x1 = r.x1, y1 = r.y1, x2 = r.x2, y2 = r.y2;
      for (var s = 0; s <= 4; s++) {
        pools.street.push({
          gx: x1 + (x2 - x1) * s / 4, gy: y1 + (y2 - y1) * s / 4,
          name: '街道', type: 'street', road: i,
        });
      }
    });
    if (!pools.home.length) pools.home.push({ gx: sz / 2, gy: sz / 2, name: '临时住处' });
    if (!pools.work.length) pools.work.push({ gx: sz / 2 + 1, gy: sz / 2, name: '写字楼' });
    if (!pools.food.length) pools.food.push({ gx: sz / 2 - 1, gy: sz / 2, name: '小餐馆' });
    if (!pools.shop.length) pools.shop = pools.food.slice();
    if (!pools.school.length) pools.school.push({ gx: sz / 2, gy: 3, name: '学校' });
    if (!pools.park.length) pools.park.push({ gx: sz / 2 + 3, gy: sz / 2 + 3, name: '绿地' });
    if (!pools.street.length) pools.street.push({ gx: sz / 2, gy: sz / 2, name: '街道' });
    pools.size = sz;
    return pools;
  }
  function pickPlace(pools, key) {
    var arr = pools[key];
    if (!arr || !arr.length) arr = pools.street || pools.home;
    return arr[Math.floor(rand() * arr.length)];
  }

  // ─────────────────────── 5. 姓名 / 日程模板 ───────────────────────
  var SURNAMES = ('李王张刘陈杨黄赵吴周徐孙马朱胡郭何高林罗郑梁谢宋唐许韩冯邓曹彭曾肖' +
    '田董袁潘蒋蔡余杜叶程苏魏吕丁任沈姚卢姜崔钟谭陆汪范金石廖贾夏韦傅方白邹孟熊秦邱江尹薛段雷侯龙史陶黎贺顾毛郝龚邵万钱严覃武戴莫孔向汤').split('');
  var GIVEN_M = ['伟', '强', '磊', '洋', '勇', '军', '杰', '涛', '明', '超', '浩然', '子轩', '宇航',
    '嘉诚', '文博', '思远', '天宇', '俊豪', '鹏飞', '立诚', '一鸣', '知远', '仲卿', '砚舟'];
  var GIVEN_F = ['芳', '娜', '敏', '静', '丽', '娟', '艳', '霞', '雪', '婷', '雨欣', '梦琪', '思彤',
    '若曦', '雅静', '美琳', '子涵', '可欣', '心怡', '诗涵', '攸宁', '清越', '知微', '南枝'];
  function randomName(gender) {
    var g = gender === 'female' ? GIVEN_F : GIVEN_M;
    return pick(SURNAMES) + pick(g);
  }
  var MOODS = {
    睡觉: ['安睡', '困倦'], 上班: ['专注', '忙碌', '有点累'], 上课: ['认真', '走神'],
    午餐: ['满足', '放松'], 晚餐: ['惬意', '放松'], 早餐: ['清醒', '慵懒'],
    买菜: ['悠闲', '精打细算'], 晨练: ['精神', '舒畅'], 散步: ['放松', '惬意'],
    闲逛: ['好奇', '悠闲'], 休闲: ['愉快', '放松'], 回家: ['安心', '疲惫'],
    通勤: ['匆忙', '平静'], 逛街: ['兴奋', '愉快'], 值班: ['专注', '疲惫'],
    待业: ['平静', '有点迷茫'], 遛狗: ['轻松', '愉悦'],
  };
  function moodFor(activity) {
    var m = MOODS[activity];
    return m ? m[Math.floor(rand() * m.length)] : '平静';
  }
  function ageGroupOf(age) {
    if (age < 15) return 'child';
    if (age < 23) return 'teen';
    if (age < 60) return 'adult';
    return 'elder';
  }
  /**
   * 作息模板：schedule[{from,to,activity,place}]，from/to 为 'HH:MM'，
   * place ∈ home/work/food/shop/school/park/street/hospital。
   * 支持跨夜（22:00→06:30 自动按环形区间匹配）。
   */
  function scheduleTemplate(npc) {
    var g = npc.ageGroup, kind = npc.kind;
    if (kind === 'passerby') return [
      { from: '06:30', to: '09:00', activity: '通勤', place: 'street' },
      { from: '09:00', to: '11:30', activity: '逛街', place: 'shop' },
      { from: '11:30', to: '13:00', activity: '午餐', place: 'food' },
      { from: '13:00', to: '17:00', activity: '闲逛', place: 'street' },
      { from: '17:00', to: '19:00', activity: '散步', place: 'park' },
      { from: '19:00', to: '22:30', activity: '逛街', place: 'shop' },
      { from: '22:30', to: '06:30', activity: '睡觉', place: 'home' },
    ];
    if (g === 'child') return [
      { from: '06:30', to: '07:30', activity: '早餐', place: 'home' },
      { from: '07:30', to: '12:00', activity: '上课', place: 'school' },
      { from: '12:00', to: '14:00', activity: '午餐', place: 'food' },
      { from: '14:00', to: '17:00', activity: '上课', place: 'school' },
      { from: '17:00', to: '19:00', activity: '散步', place: 'park' },
      { from: '19:00', to: '21:30', activity: '回家', place: 'home' },
      { from: '21:30', to: '06:30', activity: '睡觉', place: 'home' },
    ];
    if (g === 'teen') return [
      { from: '07:00', to: '12:00', activity: '上课', place: 'school' },
      { from: '12:00', to: '13:30', activity: '午餐', place: 'food' },
      { from: '13:30', to: '17:30', activity: '上课', place: 'school' },
      { from: '17:30', to: '19:30', activity: '闲逛', place: 'street' },
      { from: '19:30', to: '22:30', activity: '回家', place: 'home' },
      { from: '22:30', to: '07:00', activity: '睡觉', place: 'home' },
    ];
    if (g === 'elder') return [
      { from: '05:30', to: '07:30', activity: '晨练', place: 'park' },
      { from: '07:30', to: '09:00', activity: '早餐', place: 'food' },
      { from: '09:00', to: '11:00', activity: '买菜', place: 'shop' },
      { from: '11:00', to: '15:00', activity: '回家', place: 'home' },
      { from: '15:00', to: '17:30', activity: '散步', place: 'park' },
      { from: '17:30', to: '20:00', activity: '晚餐', place: 'home' },
      { from: '20:00', to: '21:30', activity: '散步', place: 'street' },
      { from: '21:30', to: '05:30', activity: '睡觉', place: 'home' },
    ];
    // 成年人
    var s = [
      { from: '06:30', to: '07:40', activity: '早餐', place: 'home' },
      { from: '07:40', to: '08:20', activity: '通勤', place: 'street' },
      { from: '08:20', to: '12:00', activity: '上班', place: 'work' },
      { from: '12:00', to: '13:30', activity: '午餐', place: 'food' },
      { from: '13:30', to: '18:00', activity: '上班', place: 'work' },
      { from: '18:00', to: '19:30', activity: '晚餐', place: 'food' },
      { from: '19:30', to: '22:00', activity: '休闲', place: 'park' },
      { from: '22:00', to: '06:30', activity: '睡觉', place: 'home' },
    ];
    if (npc.role === 'staff') {
      s[2] = { from: '08:00', to: '13:00', activity: '值班', place: 'work' };
      s[3] = { from: '13:00', to: '14:00', activity: '午餐', place: 'food' };
      s[4] = { from: '14:00', to: '21:00', activity: '值班', place: 'work' };
      s[5] = { from: '21:00', to: '22:30', activity: '晚餐', place: 'food' };
      s.splice(6, 1);
    }
    if (npc.occupation === '自由职业' || npc.occupation === '待业') {
      s[2] = { from: '09:30', to: '12:00', activity: '闲逛', place: 'street' };
      s[4] = { from: '14:00', to: '18:00', activity: '闲逛', place: 'park' };
    }
    if (npc.occupation === '学生') return scheduleTemplate({ ageGroup: 'teen', kind: kind });
    return s;
  }

  // ─────────────────────── 6. NPC 生成 ───────────────────────
  var OCCUPATIONS = {
    adult: ['公司职员', '教师', '医生', '护士', '程序员', '设计师', '店员', '厨师', '司机',
      '公务员', '会计', '快递员', '保安', '自由职业', '待业', '工程师', '销售'],
    elder: ['退休', '退休', '退休', '返聘顾问'],
    teen: ['学生', '学生', '学生', '实习生'],
    child: ['小学生', '小学生', '幼儿园'],
  };
  var _npcSeq = 0;

  /**
   * 生成 NPC 群体
   * opts: {count=400, size=20, layout, seed, area, namedRatio}
   * 返回 npc 数组，元素模型：
   * {id,name,kind,gridX,gridY,mood,activity,schedule[],home,work,gender,age,ageGroup,occupation,role,avatarUrl}
   */
  function generateNpcs(opts) {
    opts = opts || {};
    var count = Math.max(0, opts.count == null ? 400 : opts.count);
    if (opts.seed != null) setSeed(opts.seed);
    var layout = opts.layout || defaultLayout(opts.size || 20, opts.seed);
    var pools = opts.pools || placesFromLayout(layout, opts.size);
    var sz = pools.size || opts.size || 20;
    var list = [];
    for (var i = 0; i < count; i++) {
      // 路人多、居民少：居民有稳定家/工作，路人只是「路过这个世界」
      var kind = rand() < 0.78 ? 'passerby' : 'resident';
      var gender = rand() < 0.5 ? 'male' : 'female';
      var age;
      var roll = rand();
      if (roll < 0.14) age = randInt(4, 14);
      else if (roll < 0.30) age = randInt(15, 22);
      else if (roll < 0.86) age = randInt(23, 59);
      else age = randInt(60, 82);
      var ag = ageGroupOf(age);
      var npc = {
        id: 'npc-' + (++_npcSeq),
        name: randomName(gender),
        kind: kind,
        gender: gender,
        age: age,
        ageGroup: ag,
        occupation: pick(OCCUPATIONS[ag] || OCCUPATIONS.adult),
        role: 'citizen',
        gridX: 0, gridY: 0,
        targetX: 0, targetY: 0,
        dest: null,
        mood: '平静',
        activity: '闲逛',
        speedScale: ag === 'elder' ? 0.72 : (ag === 'child' ? 1.15 : 1),
        schedule: [],
        home: null, work: null,
        waitUntil: 0,
        _cooldown: {},
      };
      // 起点：居民从家附近出现，路人从街道/边缘出现
      npc.home = pickPlace(pools, 'home');
      npc.work = pickPlace(pools, 'work');
      if (kind === 'resident') {
        npc.gridX = npc.home.gx + (rand() - 0.5) * 2;
        npc.gridY = npc.home.gy + (rand() - 0.5) * 2;
      } else {
        if (rand() < 0.5) {
          var edge = randInt(0, 3);
          if (edge === 0) { npc.gridX = rand() * sz; npc.gridY = 0.5; }
          else if (edge === 1) { npc.gridX = rand() * sz; npc.gridY = sz - 0.5; }
          else if (edge === 2) { npc.gridX = 0.5; npc.gridY = rand() * sz; }
          else { npc.gridX = sz - 0.5; npc.gridY = rand() * sz; }
        } else {
          var sp = pickPlace(pools, 'street');
          npc.gridX = sp.gx + (rand() - 0.5) * 3;
          npc.gridY = sp.gy + (rand() - 0.5) * 3;
        }
      }
      npc.gridX = clamp(npc.gridX, 0.2, sz - 0.2);
      npc.gridY = clamp(npc.gridY, 0.2, sz - 0.2);
      npc.targetX = npc.gridX; npc.targetY = npc.gridY;
      npc.schedule = scheduleTemplate(npc);
      if (opts.area) npc.area = opts.area;
      npc._pools = pools;          // 绑定地点池：作息里的 place 关键字才能解析成具体坐标
      indexNpc(npc);               // 入索引：聚焦 / 关系 / 互动都要靠它按 id 找人
      list.push(npc);
    }
    // 初始按当前时间对齐作息（决定每个人此刻在哪、干什么）
    updateNpcActivity(list, opts.timeState || null);
    return list;
  }
  function clamp(v, a, b) { return v < a ? a : (v > b ? b : v); }

  // ─────────────────── 7. 作息：updateNpcActivity ───────────────────
  /** 在 schedule 里找当前时段（支持跨夜环形区间） */
  function slotAt(schedule, mins) {
    if (!schedule || !schedule.length) return null;
    for (var i = 0; i < schedule.length; i++) {
      var s = schedule[i];
      var from = parseHM(s.from), to = parseHM(s.to);
      var dur = (to - from + DAY) % DAY;
      if (dur === 0) dur = DAY;
      if (((mins - from + DAY) % DAY) < dur) return s;
    }
    return schedule[0];
  }
  /**
   * 按时间决定每个 NPC 现在在哪、干什么。
   * @param {Array} npcs
   * @param {Object|number} timeState /api/time 的返回或数字小时；空则用本地时间
   * @returns {{time:Object, changed:Array}} changed = 本次活动发生变化的 NPC
   */
  function updateNpcActivity(npcs, timeState) {
    var st = normalizeTimeState(timeState);
    var changed = [];
    for (var i = 0; i < npcs.length; i++) {
      var n = npcs[i];
      var slot = slotAt(n.schedule, st.minutes);
      if (!slot) continue;
      var placeKey = slot.place;
      var changedNow = n.activity !== slot.activity || n.placeKey !== placeKey;
      n.activity = slot.activity;
      n.mood = changedNow ? moodFor(slot.activity) : n.mood;
      if (changedNow) {
        n.placeKey = placeKey;
        var p = resolvePlace(n, placeKey);
        n.place = { gx: p.gx, gy: p.gy, name: p.name, type: p.type || placeKey };
        // 目的地 = 地点 + 少量抖动，避免几百人挤在同一个像素上
        var lim = (n._pools && n._pools.size) || 20;
        var jit = function (v) { return clamp(v + (rand() - 0.5) * config.placeJitter * 2, 0.2, lim - 0.2); };
        n.dest = {
          gx: jit(p.gx), gy: jit(p.gy),
          name: p.name, placeKey: placeKey,
        };
        n.targetX = n.dest.gx; n.targetY = n.dest.gy;
        n.arrived = false;
        changed.push(n);
      }
      n.timeLabel = st.time;
    }
    return { time: st, changed: changed };
  }
  /** 把作息里的 place 关键字解析成具体地点 */
  function resolvePlace(npc, key) {
    var pools = npc._pools;
    if (key === 'home' && npc.home) return npc.home;
    if (key === 'work' && npc.work) return npc.work;
    if (pools) {
      if (key === 'park' && pools.park && pools.park.length) return pick(pools.park);
      if (key === 'street' && pools.street && pools.street.length) return pick(pools.street);
      if (key === 'food' && pools.food && pools.food.length) return pick(pools.food);
      if (key === 'shop' && pools.shop && pools.shop.length) return pick(pools.shop);
      if (key === 'school' && pools.school && pools.school.length) return pick(pools.school);
      if (key === 'hospital' && pools.hospital && pools.hospital.length) return pick(pools.hospital);
    }
    if (npc.home) return npc.home;
    return { gx: npc.gridX, gy: npc.gridY, name: '街上', type: 'street' };
  }
  /** 给一批 NPC 绑定地点池（可选，用于 resolvePlace 更丰富） */
  function bindPools(npcs, pools) {
    (npcs || []).forEach(function (n) { n._pools = pools; });
    return npcs;
  }

  // ─────────────────── 8. 移动：tickNpcs ───────────────────
  /**
   * 平滑移动。速度按步行 1.4 m/s 换算到网格（格/秒 = walkSpeed / metersPerCell）。
   * @param {Array} npcs
   * @param {number} dt 秒（>100 视为毫秒自动换算）
   * @param {Object} opts {now, pools, size}
   * @returns {{moved:number, arrived:number, dtSec:number}}
   */
  function tickNpcs(npcs, dt, opts) {
    opts = opts || {};
    var dtSec = dt > 100 ? dt / 1000 : dt;              // 容错：传毫秒也能用
    if (!(dtSec > 0)) dtSec = 0;
    if (dtSec > (config.maxTickDtSec || 2)) dtSec = config.maxTickDtSec || 2;  // 切后台回来不瞬移
    var now = opts.now == null ? Date.now() : opts.now;
    var perCell = config.metersPerCell || 5;
    var stepBase = (config.walkSpeed / perCell) * dtSec;
    var size = opts.size || (opts.pools && opts.pools.size) || 20;
    var moved = 0, arrived = 0;
    for (var i = 0; i < npcs.length; i++) {
      var n = npcs[i];
      if (n.frozen) continue;
      var dx = n.targetX - n.gridX, dy = n.targetY - n.gridY;
      var dist = Math.sqrt(dx * dx + dy * dy);
      var step = stepBase * (n.speedScale || 1);
      if (dist <= step || dist < 1e-4) {
        if (dist > 1e-4) { n.gridX = n.targetX; n.gridY = n.targetY; moved++; arrived++; n.arrived = true; }
        // 路人：到达后随机歇一会儿，再挑新的目的地（持续流动）
        if (n.kind === 'passerby' && now >= (n.waitUntil || 0)) {
          var pool = (n._pools && n._pools.street && n._pools.street.length) ? n._pools.street : null;
          var tx, ty;
          if (pool && rand() < 0.55) {
            var p = pool[Math.floor(rand() * pool.length)];
            tx = p.gx + (rand() - 0.5) * 3; ty = p.gy + (rand() - 0.5) * 3;
          } else {
            tx = rand() * size; ty = rand() * size;
          }
          n.targetX = clamp(tx, 0.2, size - 0.2);
          n.targetY = clamp(ty, 0.2, size - 0.2);
          n.waitUntil = now + randInt(1500, 9000);
          n.arrived = false;
          if (n.activity === '睡觉') { n.waitUntil = now + randInt(20000, 60000); }
        }
        if (n.kind !== 'passerby' && n.arrived && rand() < 0.02) {
          // 居民到达目的地后小幅游荡一下（看起来更自然）
          n.targetX = clamp(n.gridX + (rand() - 0.5) * config.placeJitter * 2, 0.2, size - 0.2);
          n.targetY = clamp(n.gridY + (rand() - 0.5) * config.placeJitter * 2, 0.2, size - 0.2);
          n.arrived = false;
        }
      } else {
        n.gridX += (dx / dist) * step;
        n.gridY += (dy / dist) * step;
        moved++;
        if (n.track) n.track.push([n.gridX, n.gridY]);
      }
      if (config.trackLength > 0) {
        if (!n.track) n.track = [];
        n.track.push([n.gridX, n.gridY]);
        while (n.track.length > config.trackLength) n.track.shift();
      }
    }
    var res = { moved: moved, arrived: arrived, dtSec: dtSec };
    if (config.autoInteract) res.interactions = tickInteractions(npcs, dtSec, opts);
    return res;
  }

  // ─────────────────── 9. 路人点渲染：drawCrowd ───────────────────
  // 批量方案：① 投影+视口裁剪 ② 按颜色分桶 ③ 每种颜色一次 fillStyle，
  // 圆点用「一条 Path 装下所有 arc」再一次性 fill，避免逐个 beginPath/fill 的开销。
  var _buckets = {};
  var _bucketUsed = [];
  function _bucket(key) {
    var b = _buckets[key];
    if (!b) b = _buckets[key] = [];
    return b;
  }
  function _normCamera(camera, ctx) {
    var cam = camera || {};
    var cv = ctx && ctx.canvas ? ctx.canvas : { width: 0, height: 0 };
    var w = cam.width || cam.w || cv.width || 0;
    var h = cam.height || cam.h || cv.height || 0;
    var scale = cam.scale == null ? 1 : cam.scale;
    return { scale: scale, tx: cam.tx || 0, ty: cam.ty || 0, width: w, height: h };
  }
  function _project(fn, gx, gy) {
    var p = fn(gx, gy);
    if (!p) return null;
    if (typeof p.length === 'number') return { cx: p[0], cy: p[1] };
    var cx = (p.cx !== undefined ? p.cx : p.x), cy = (p.cy !== undefined ? p.cy : p.y);
    if (cx == null || cy == null) return null;
    return { cx: cx, cy: cy };
  }
  /**
   * 绘制路人点（大量小圆点 + 视口裁剪）
   * @param {CanvasRenderingContext2D} ctx
   * @param {Array} npcs
   * @param {Function} toCanvasFn (gridX, gridY) → [cx,cy] | {cx,cy}（未叠加相机变换的画布坐标）
   * @param {Object} camera {scale, tx, ty, width, height}；屏幕坐标 = cx*scale+tx
   * @returns {{total:number, drawn:number, culled:number, ms:number, shape:string}}
   */
  function drawCrowd(ctx, npcs, toCanvasFn, camera) {
    var t0 = nowMs();
    var cam = _normCamera(camera, ctx);
    var m = config.viewportMargin;
    var x0 = -m, y0 = -m, x1 = cam.width + m, y1 = cam.height + m;
    var total = 0, drawn = 0, culled = 0;
    var i, k;
    for (k in _buckets) _buckets[k].length = 0;
    _bucketUsed.length = 0;
    var marks = [];   // 聚焦/头像角色的强调环（数量很少，单独描边）
    for (i = 0; i < npcs.length; i++) {
      var n = npcs[i];
      if (n.hidden) continue;
      total++;
      var grp = dotGroup(n);
      var p = _project(toCanvasFn, n.gridX, n.gridY);
      if (!p) { culled++; continue; }
      var sx = p.cx * cam.scale + cam.tx;
      var sy = p.cy * cam.scale + cam.ty;
      // 视口裁剪：只画屏幕内的
      if (sx < x0 || sx > x1 || sy < y0 || sy > y1) { culled++; continue; }
      var style = DOT[grp] || DOT.male_adult;
      var arr = _bucket(grp);
      if (arr.length === 0) _bucketUsed.push(grp);
      arr.push(sx, sy, style.r);
      drawn++;
      if (grp === 'focused' || grp === 'named') marks.push(sx, sy, style.r, grp);
    }
    // 小点数量巨大时用 fillRect（快很多），视觉上 2~4px 的方圆差异几乎看不出
    var shape = config.crowdShape;
    if (shape === 'auto') shape = drawn > config.circleShapeMax ? 'rect' : 'circle';
    for (i = 0; i < _bucketUsed.length; i++) {
      var g = _bucketUsed[i];
      var st = DOT[g] || DOT.male_adult;
      var a = _buckets[g];
      ctx.fillStyle = st.color;
      if (shape === 'rect') {
        for (k = 0; k < a.length; k += 3) {
          ctx.fillRect(a[k] - a[k + 2], a[k + 1] - a[k + 2], a[k + 2] * 2, a[k + 2] * 2);
        }
      } else {
        ctx.beginPath();
        for (k = 0; k < a.length; k += 3) {
          ctx.moveTo(a[k] + a[k + 2], a[k + 1]);
          ctx.arc(a[k], a[k + 1], a[k + 2], 0, TAU);
        }
        ctx.fill();
      }
    }
    // 强调环：聚焦中（金）与 LingChat 角色（青）——让人一眼看出「谁被升级了」
    if (marks.length) {
      for (i = 0; i < marks.length; i += 4) {
        ctx.beginPath();
        ctx.arc(marks[i], marks[i + 1], marks[i + 2] + 4, 0, TAU);
        ctx.strokeStyle = marks[i + 3] === 'focused' ? 'rgba(255,194,77,.85)' : 'rgba(121,217,255,.75)';
        ctx.lineWidth = 1.5;
        ctx.stroke();
      }
    }
    return {
    renderEventFeed: renderEventFeed,
    fmtTs: fmtTs,
      total: total, drawn: drawn, culled: culled, shape: shape,
      ms: Math.round((nowMs() - t0) * 100) / 100,
    };
  }
  function nowMs() {
    return (typeof performance !== 'undefined' && performance.now) ? performance.now() : Date.now();
  }

  // ─────────────────── 10. 事件系统 ───────────────────
  var EVENT_STYLE = {
    major:    { icon: '⚠️', color: '#ffc24d', label: '重大事件', major: true },
    incident: { icon: '🚨', color: '#ff8296', label: '突发事件', major: true },
    rescue:   { icon: '🆘', color: '#ff8296', label: '救援', major: true },
    meet:     { icon: '🤝', color: '#4caf84', label: '相遇', major: true },
    romance:  { icon: '💗', color: '#ff9ec4', label: '心动', major: true },
    milestone:{ icon: '🎯', color: '#79d9ff', label: '里程碑', major: true },
    chat:     { icon: '💬', color: '#79d9ff', label: '聊天' },
    greet:    { icon: '👋', color: '#8fd6ff', label: '打招呼' },
    walk_together: { icon: '🚶', color: '#4caf84', label: '一起走' },
    activity: { icon: '📍', color: '#79d9ff', label: '动向' },
    focus:    { icon: '🔍', color: '#ffc24d', label: '聚焦' },
  };
  var events = [];
  var eventSeq = 0;
  var eventListeners = [];
  function eventStyle(type) {
    return EVENT_STYLE[type] || { icon: '•', color: '#79d9ff', label: type || '事件' };
  }
  function esc(s) {
    return String(s == null ? '' : s).replace(/[&<>"']/g, function (c) {
      return ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c];
    });
  }
  function onEvent(cb) {
    if (typeof cb !== 'function') return function () {};
    eventListeners.push(cb);
    return function () {
      var i = eventListeners.indexOf(cb);
      if (i >= 0) eventListeners.splice(i, 1);
    };
  }
  function _emit(ev) {
    for (var i = 0; i < eventListeners.length; i++) {
      try { eventListeners[i](ev); } catch (e) {}
    }
  }
  /**
   * 触发一个事件。
   * @param {Object} e {type, npcId, peerId, title, desc, gridX, gridY, major, focusMs, payload}
   *   major 未显式给出时，按事件类型判断（chat/greet/walk_together 等日常互动不算重大）
   * @returns {Object} 事件对象（含 id/ts/时间戳/major/focused 列表）
   */
  function triggerEvent(e) {
    e = e || {};
    var npc = e.npcId ? getNpc(e.npcId) : null;
    var peer = e.peerId ? getNpc(e.peerId) : null;
    var st = eventStyle(e.type);
    var major = e.major != null ? !!e.major : !!st.major;
    var gx = e.gridX != null ? e.gridX : (npc ? npc.gridX : (peer ? peer.gridX : 0));
    var gy = e.gridY != null ? e.gridY : (npc ? npc.gridY : (peer ? peer.gridY : 0));
    var ev = {
      id: 'ev-' + (++eventSeq),
      ts: Date.now(),
      time: hm(normalizeTimeState(null).minutes),
      type: e.type || 'activity',
      typeLabel: st.label,
      icon: st.icon,
      color: st.color,
      major: major,
      npcId: e.npcId || null,
      npcName: e.npcName || (npc ? npc.name : ''),
      peerId: e.peerId || null,
      peerName: peer ? peer.name : (e.peerName || ''),
      title: e.title || (st.label + (npc ? '：' + npc.name : '')),
      desc: e.desc || '',
      gridX: gx, gridY: gy,
      placeName: e.placeName || (npc && npc.place ? npc.place.name : ''),
      payload: e.payload || null,
      focusedIds: [],
    };
    // 重大事件 → 自动聚焦相关 NPC（点 → 详情卡），一段时间后自动降级
    if (major) {
      var dur = e.focusMs || config.focusDurationMs;
      if (ev.npcId) { var f1 = focusNpc(ev.npcId, dur, { eventId: ev.id, reason: 'major-event' }); if (f1) ev.focusedIds.push(ev.npcId); }
      if (ev.peerId) { var f2 = focusNpc(ev.peerId, dur, { eventId: ev.id, reason: 'major-event' }); if (f2) ev.focusedIds.push(ev.peerId); }
    }
    events.push(ev);
    while (events.length > config.maxEvents) events.shift();
    _emit(ev);
    return ev;
  }
  function getEvents(limit) {
    if (limit == null || limit <= 0) return events.slice();
    return events.slice(Math.max(0, events.length - limit));
  }
  function clearEvents() { events.length = 0; }
  /** 随机挑一个「重大事件」——演示/自主剧情用 */
  function randomIncident(npcs, opts) {
    opts = opts || {};
    if (!npcs || !npcs.length) return null;
    var templates = opts.templates || [
      { type: 'incident', title: '街口突发争执', desc: '两个路人在路口因为一点小事吵了起来，围了一圈人。' },
      { type: 'rescue', title: '有人晕倒了', desc: '一位老人在公园长椅旁晕倒，热心人围上去帮忙。' },
      { type: 'meet', title: '久别重逢', desc: '两个人在街角认出了对方，都愣了几秒。' },
      { type: 'romance', title: '街头心动', desc: '一次擦肩，目光停了一下。' },
      { type: 'milestone', title: '小区公告', desc: '公告栏贴出了新的通知，路过的人都停下来看。' },
      { type: 'incident', title: '走失的猫', desc: '一只橘猫蹲在树上下不来，人群在下面商量办法。' },
    ];
    var t = templates[Math.floor(rand() * templates.length)];
    var a = npcs[Math.floor(rand() * npcs.length)];
    var b = null;
    if (t.type === 'meet' || t.type === 'romance' || t.type === 'incident') {
      for (var i = 0; i < 12 && !b; i++) {
        var c = npcs[Math.floor(rand() * npcs.length)];
        if (c !== a) b = c;
      }
    }
    return triggerEvent({
      type: t.type, title: t.title, desc: t.desc,
      npcId: a.id, peerId: b ? b.id : null,
      gridX: a.gridX, gridY: a.gridY, major: true,
      focusMs: opts.focusMs,
    });
  }

  // ─────────────────── 11. 聚焦机制 ───────────────────
  // 「点 → 详情卡 → 点」：聚焦表按到期时间自动过期，getFocused() 每次调用都会先清理过期项。
  var focused = {};
  function focusNpc(id, durationMs, opts) {
    var n = getNpc(id);
    if (!n) return null;
    var now = Date.now();
    var dur = durationMs == null ? config.focusDurationMs : durationMs;
    var rec = {
      npcId: n.id, npc: n, since: now, until: now + dur,
      durationMs: dur, eventId: (opts && opts.eventId) || null,
      reason: (opts && opts.reason) || 'manual',
    };
    focused[n.id] = rec;
    n.focused = true;
    return rec;
  }
  function unfocus(id) {
    var n = getNpc(id);
    var rec = focused[id];
    delete focused[id];
    if (n) n.focused = false;
    return rec || null;
  }
  function clearFocus() {
    for (var k in focused) if (Object.prototype.hasOwnProperty.call(focused, k)) {
      var n = getNpc(k); if (n) n.focused = false;
    }
    focused = {};
  }
  /** 返回当前仍有效的聚焦列表（顺带清理过期项 = 自动降级回点） */
  function getFocused(opts) {
    var now = Date.now();
    var out = [];
    for (var k in focused) {
      if (!Object.prototype.hasOwnProperty.call(focused, k)) continue;
      var rec = focused[k];
      if (rec.until <= now) {
        // 到期：降级回「点」，并往事件流里留一条记录（静默模式不打扰）
        var n0 = rec.npc;
        if (n0) n0.focused = false;
        delete focused[k];
        if (!(opts && opts.silent)) {
          var endEv = {
            id: 'focus-end-' + rec.npcId, ts: now, type: 'focus',
            typeLabel: EVENT_STYLE.focus.label, icon: '🔙', color: '#8fd6ff',
            major: false, npcId: rec.npcId, npcName: rec.npc ? rec.npc.name : '',
            title: (rec.npc ? rec.npc.name : rec.npcId) + ' 的聚焦已结束',
            desc: '已从详情卡自动降级回路人点',
            focusEnd: true, gridX: rec.npc ? rec.npc.gridX : 0, gridY: rec.npc ? rec.npc.gridY : 0,
          };
          events.push(endEv);
          while (events.length > config.maxEvents) events.shift();
          _emit(endEv);
        }
        continue;
      }
      rec.remainMs = rec.until - now;
      rec.event = rec.eventId ? events.filter(function (e) { return e.id === rec.eventId; })[0] : null;
      out.push(rec);
    }
    out.sort(function (a, b) { return b.since - a.since; });
    return out;
  }
  function isFocused(id) { return !!focused[id]; }

  // ─────────────────── 12. LingChat 角色头像 ───────────────────
  // 用户需求：只有 LingChat 角色列表里的角色才拥有头像显示，且「始终显示」（不依赖聚焦）。
  var namedMap = {};
  var npcIndex = {};
  function indexNpc(n) { if (n && n.id) npcIndex[n.id] = n; return n; }
  function getNpc(id) { return npcIndex[id] || null; }
  function clearNpcIndex() { npcIndex = {}; namedMap = {}; }
  function getAllNpcs() {
    var out = [];
    for (var k in npcIndex) if (Object.prototype.hasOwnProperty.call(npcIndex, k)) out.push(npcIndex[k]);
    return out;
  }
  /** 注册 NPC 到索引（generateNpcs 之外手工建的角色也要能被聚焦/互动找到） */
  function registerNpc(n) {
    if (!n) return null;
    if (!n.id) n.id = 'npc-' + (++_npcSeq);
    if (n.gridX == null) n.gridX = 0;
    if (n.gridY == null) n.gridY = 0;
    if (n.targetX == null) n.targetX = n.gridX;
    if (n.targetY == null) n.targetY = n.gridY;
    if (!n.schedule) n.schedule = scheduleTemplate({
      ageGroup: n.ageGroup || ageGroupOf(n.age == null ? 30 : n.age),
      kind: n.kind || 'resident', occupation: n.occupation, role: n.role,
    });
    if (!n.kind) n.kind = 'resident';
    if (n.age == null) n.age = 30;
    if (!n.ageGroup) n.ageGroup = ageGroupOf(n.age);
    if (!n.gender) n.gender = 'male';
    if (!n.activity) n.activity = '闲逛';
    if (!n.mood) n.mood = '平静';
    if (n.speedScale == null) n.speedScale = 1;
    indexNpc(n);
    return n;
  }
  function registerNpcs(list) { (list || []).forEach(registerNpc); return list; }
  /**
   * 设置 LingChat 角色列表（有头像的人）。
   * @param {Array} list [{id,name,avatarUrl, ...}]；已存在的 NPC 会被升级为 named，
   *   不存在则按网格坐标/中心生成一个新的 named 角色。
   * @param {Object} opts {size, pools, spread, kind:'named'}
   * @returns {Array} 处理后的角色 npc 列表
   */
  function setNamedCharacters(list, opts) {
    opts = opts || {};
    var pools = opts.pools || null;
    var size = opts.size || (pools && pools.size) || 20;
    var out = [];
    var clean = {}; // 本次传入的 id 集合，用于撤销「已不在列表里」的角色标记
    (list || []).forEach(function (c, i) {
      if (!c) return;
      var id = c.id || ('lingchat-' + (i + 1));
      clean[id] = true;
      var n = npcIndex[id];
      if (!n) {
        var base = pools ? pickPlace(pools, 'street') : { gx: size / 2, gy: size / 2 };
        n = registerNpc({
          id: id, name: c.name || id, kind: 'named',
          gridX: base.gx + (rand() - 0.5) * (opts.spread || 3),
          gridY: base.gy + (rand() - 0.5) * (opts.spread || 3),
          gender: c.gender || 'female', age: c.age || 20,
          occupation: c.occupation || 'LingChat 角色', mood: c.mood || '平静',
          activity: c.activity || '闲逛',
        });
        if (pools) n._pools = pools;
      }
      // 升级为 named：始终带头像，且比路人大
      n.kind = 'named';
      n.name = c.name || n.name;
      n.avatarUrl = c.avatarUrl || n.avatarUrl || '';
      n.lingsChat = true;
      n.persona = c.persona || n.persona || '';
      n.raw = c;
      namedMap[id] = n;
      out.push(n);
    });
    // 从 named 名单里移除的 id：降级回普通居民（头像不再显示）
    var stale = [];
    for (var k in namedMap) {
      if (Object.prototype.hasOwnProperty.call(namedMap, k) && !clean[k]) stale.push(k);
    }
    stale.forEach(function (k) {
      var old = namedMap[k];
      if (old) { old.kind = 'resident'; old.lingsChat = false; old.avatarUrl = ''; }
      delete namedMap[k];
    });
    return out;
  }
  function getNamedCharacters() {
    var out = [];
    for (var k in namedMap) if (Object.prototype.hasOwnProperty.call(namedMap, k)) out.push(namedMap[k]);
    return out;
  }
  function getAvatarUrl(id) {
    var n = getNpc(id);
    return (n && n.kind === 'named' && n.avatarUrl) ? n.avatarUrl : '';
  }

  // ─────────────────── 13. 关系图（T4-4） ───────────────────
  var REL_TYPES = {
    '认识': { color: '#8fd6ff', weight: 0.5 },
    '朋友': { color: '#79d9ff', weight: 1.0 },
    '家人': { color: '#ff9ec4', weight: 1.6 },
    '同事': { color: '#4caf84', weight: 0.9 },
  };
  var relations = [];
  var relIndex = {};
  function relKey(a, b) { return a < b ? a + '|' + b : b + '|' + a; }
  /**
   * 建立关系。type ∈ 认识/朋友/家人/同事（可用英文别名 acquaintance/friend/family/colleague）
   * @returns {Object} 关系记录
   */
  function addRelation(a, b, type, opts) {
    var idA = (a && a.id) || a, idB = (b && b.id) || b;
    if (!idA || !idB || idA === idB) return null;
    var alias = { acquaintance: '认识', friend: '朋友', family: '家人', colleague: '同事' };
    var t = alias[type] || type || '认识';
    if (!REL_TYPES[t]) t = '认识';
    var key = relKey(idA, idB);
    var rel = relIndex[key];
    if (rel) { rel.type = t; rel.updated = Date.now(); }
    else {
      rel = {
        id: 'rel-' + (relations.length + 1),
        a: idA, b: idB, type: t,
        strength: (opts && opts.strength) != null ? opts.strength : (REL_TYPES[t].weight || 0.5),
        since: Date.now(), updated: Date.now(), source: (opts && opts.source) || 'manual',
        encounters: 0,
      };
      relations.push(rel);
      relIndex[key] = rel;
    }
    return rel;
  }
  function getRelation(a, b) { return relIndex[relKey((a && a.id) || a, (b && b.id) || b)] || null; }
  /** 某人的全部关系，附带对方信息 */
  function getRelations(id) {
    var out = [];
    for (var i = 0; i < relations.length; i++) {
      var r = relations[i];
      if (r.a !== id && r.b !== id) continue;
      var otherId = r.a === id ? r.b : r.a;
      var other = getNpc(otherId);
      out.push({
        type: r.type, color: (REL_TYPES[r.type] || {}).color || '#79d9ff',
        strength: r.strength, since: r.since, encounters: r.encounters,
        otherId: otherId, other: other,
        otherName: other ? other.name : otherId,
        otherKind: other ? other.kind : 'unknown',
      });
    }
    out.sort(function (x, y) { return y.strength - x.strength; });
    return out;
  }
  function relationTypeBetween(a, b) {
    var r = getRelation(a, b);
    return r ? r.type : null;
  }
  /** 关系图 {nodes, edges}，方便以后画关系网 */
  function getRelationGraph() {
    var nodes = {}, edges = [];
    relations.forEach(function (r) {
      nodes[r.a] = nodes[r.a] || { id: r.a, npc: getNpc(r.a) };
      nodes[r.b] = nodes[r.b] || { id: r.b, npc: getNpc(r.b) };
      edges.push({ source: r.a, target: r.b, type: r.type, strength: r.strength });
    });
    var nodeArr = [];
    for (var k in nodes) if (Object.prototype.hasOwnProperty.call(nodes, k)) nodeArr.push(nodes[k]);
    return { nodes: nodeArr, edges: edges };
  }
  /**
   * 自动给一群 NPC 建立关系：同住一栋楼=家人，同单位=同事，随机=朋友/认识。
   * 让「互动」有社会结构，而不是纯随机。
   */
  function buildRelations(npcs, opts) {
    opts = opts || {};
    var byHome = {}, byWork = {}, byName = {};
    (npcs || []).forEach(function (n) {
      if (!n.home) return;
      var hk = fmtKey(n.home), wk = n.work ? fmtKey(n.work) : null;
      (byHome[hk] = byHome[hk] || []).push(n);
      if (wk) (byWork[wk] = byWork[wk] || []).push(n);
      // 同姓 + 同住 = 更可能是家人
      if (n.name) {
        var sk = n.name.charAt(0) + '@' + hk;
        (byName[sk] = byName[sk] || []).push(n);
      }
    });
    function pairUp(bucketMap, type, maxPerGroup) {
      var made = 0;
      for (var k in bucketMap) {
        if (!Object.prototype.hasOwnProperty.call(bucketMap, k)) continue;
        var arr = bucketMap[k];
        if (arr.length > (opts.maxGroupSize || 24)) arr = arr.slice(0, opts.maxGroupSize || 24);
        for (var i = 0; i < arr.length; i++) {
          var links = Math.min(maxPerGroup, arr.length - i - 1);
          for (var j = 0; j < links; j++) {
            var other = arr[i + 1 + j];
            if (!other) break;
            if (addRelation(arr[i], other, type, { source: 'auto' })) made++;
          }
        }
      }
      return made;
    }
    var family = pairUp(byName, '家人', 2);
    var colleague = pairUp(byWork, '同事', 3);
    // 随机友情/认识
    var friendTarget = Math.round((npcs || []).length * (opts.friendRatio == null ? 0.15 : opts.friendRatio));
    var acquaintTarget = Math.round((npcs || []).length * (opts.acquaintRatio == null ? 0.25 : opts.acquaintRatio));
    var n = npcs || [];
    for (var f = 0; f < friendTarget && n.length > 1; f++) {
      addRelation(n[Math.floor(rand() * n.length)], n[Math.floor(rand() * n.length)], '朋友', { source: 'auto' });
    }
    for (var q = 0; q < acquaintTarget && n.length > 1; q++) {
      addRelation(n[Math.floor(rand() * n.length)], n[Math.floor(rand() * n.length)], '认识', { source: 'auto' });
    }
    return { total: relations.length, family: family, colleague: colleague };
  }
  function fmtKey(p) { return (p.name || '') + '@' + Math.round(p.gx * 10) / 10 + ',' + Math.round(p.gy * 10) / 10; }
  function clearRelations() { relations = []; relIndex = {}; }
  function getAllRelations() { return relations.slice(); }

  // ─────────────────── 14. 互动触发（T4-4） ───────────────────
  var INTERACTIONS = [
    { min: 0, type: 'greet', title: '打了个招呼', desc: '两人点头示意，脚步没停。' },
    { min: 0, type: 'chat', title: '停下来说了几句', desc: '在路边聊了几句家常。' },
    { min: 0.6, type: 'chat', title: '聊得挺投机', desc: '两人聊了一会儿才各走各的。' },
    { min: 1.2, type: 'walk_together', title: '并肩走了一段', desc: '顺路，于是一起走了一段。' },
  ];
  /**
   * 检测「靠近」并触发互动。空间哈希分桶，O(n)。
   * @param {Array} npcs
   * @param {number} dt 秒
   * @param {Object} opts {now, maxPerTick, pools}
   * @returns {{pairs:number, events:Array}}
   */
  function tickInteractions(npcs, dt, opts) {
    opts = opts || {};
    var dtSec = dt > 100 ? dt / 1000 : dt;
    if (!(dtSec > 0)) return { pairs: 0, events: [] };
    var now = opts.now == null ? Date.now() : opts.now;
    var dist = config.interactionDist;
    var size = opts.size || (opts.pools && opts.pools.size) || 20;
    var maxOut = opts.maxPerTick == null ? config.maxInteractionsPerTick : opts.maxPerTick;
    var cells = {}, i, n;
    for (i = 0; i < npcs.length; i++) {
      n = npcs[i];
      if (n.frozen || n.hidden) continue;
      n.__i = i;
      var key = Math.floor(n.gridX / dist) + ':' + Math.floor(n.gridY / dist);
      (cells[key] = cells[key] || []).push(i);
    }
    var out = [], pairs = 0;
    for (var ck in cells) {
      if (!Object.prototype.hasOwnProperty.call(cells, ck)) continue;
      var parts = ck.split(':');
      var bx = parseInt(parts[0], 10), by = parseInt(parts[1], 10);
      for (var dx = -1; dx <= 1; dx++) {
        for (var dy = -1; dy <= 1; dy++) {
          var ok = bx + dx + ':' + (by + dy);
          var other = cells[ok];
          if (!other) continue;
          for (var ii = 0; ii < cells[ck].length; ii++) {
            for (var jj = 0; jj < other.length; jj++) {
              var ia = cells[ck][ii], ib = other[jj];
              if (ia >= ib) continue;                 // 每对只算一次
              var A = npcs[ia], B = npcs[ib];
              var ddx = A.gridX - B.gridX, ddy = A.gridY - B.gridY;
              if (ddx * ddx + ddy * ddy >= dist * dist) continue;   // 网格距离 < 2 才算靠近
              pairs++;
              if (out.length >= maxOut) continue;
              if ((A._cooldown && A._cooldown[B.id] > now) || (B._cooldown && B._cooldown[A.id] > now)) continue;
              var rel = getRelation(A, B);
              var prob = rel ? config.interactionProbKnown * Math.min(2, rel.strength + 0.4) : config.interactionProb;
              prob = 1 - Math.pow(1 - prob, dtSec);     // 按 dt 折算，帧率无关
              if (rand() > prob) continue;
              var ev = _doInteraction(A, B, rel, { now: now, size: size, opts: opts });
              if (ev) {
                out.push(ev);
                A._cooldown = A._cooldown || {}; A._cooldown[B.id] = now + config.interactionCooldownMs;
                B._cooldown = B._cooldown || {}; B._cooldown[A.id] = now + config.interactionCooldownMs;
              }
            }
          }
        }
      }
    }
    return { pairs: pairs, events: out };
  }
  function _doInteraction(A, B, rel, ctx) {
    var strength = rel ? (rel.strength || 0.5) : 0;
    var roll = rand();
    var candidates = INTERACTIONS.filter(function (x) {
      if (x.type === 'walk_together' && !rel) return false;      // 一起走只发生在有关系的人之间
      return strength + roll >= x.min;
    });
    var kind = candidates.length ? candidates[Math.floor(rand() * candidates.length)] : INTERACTIONS[0];
    var label = rel ? rel.type : '陌生人';
    // 关系强度随互动缓慢增长
    if (rel) {
      rel.encounters = (rel.encounters || 0) + 1;
      rel.strength = Math.min(2.2, (rel.strength || 0.5) + 0.02);
      rel.updated = ctx.now;
    } else if (rand() < 0.05) {
      rel = addRelation(A, B, '认识', { source: 'encounter' });
    }
    var desc = kind.desc;
    if (rel) desc += '（' + label + '·第 ' + (rel.encounters || 1) + ' 次）';
    var placeA = A.place ? A.place.name : '街上';
    return triggerEvent({
      type: kind.type,
      title: A.name + ' 与 ' + B.name + ' ' + kind.title,
      desc: desc,
      npcId: A.id, peerId: B.id,
      npcName: A.name,
      gridX: (A.gridX + B.gridX) / 2,
      gridY: (A.gridY + B.gridY) / 2,
      placeName: placeA,
      payload: { relation: rel ? rel.type : null, activityA: A.activity, activityB: B.activity },
      major: false,   // 日常互动不聚焦（只有重大事件才升级详情卡）
    });
  }

  // ─────────────────── 15. 事件流 UI（T4-4） ───────────────────
  var STYLE_ID = 'npc-sys-style';
  /** 注入样式（玻璃拟态 + #79d9ff 主色，类名统一 npc- 前缀，避免污染宿主页面） */
  function injectStyles(doc) {
    var d = doc || (typeof document !== 'undefined' ? document : null);
    if (!d || d.getElementById(STYLE_ID)) return;
    var css = [
      '.npc-feed{font-size:12px;color:#eaf3ff}',
      '.npc-feed-h{font-size:12.5px;color:rgba(180,205,235,.85);font-weight:600;margin:0 0 8px;display:flex;align-items:center;gap:6px}',
      '.npc-evitem{padding:8px 10px;border-left:3px solid #79d9ff;margin:7px 0;border-radius:0 10px 10px 0;background:rgba(255,255,255,.05);animation:npc-ev-in .28s ease}',
      '.npc-evitem.major{background:rgba(255,194,77,.10);box-shadow:0 0 0 1px rgba(255,194,77,.18) inset}',
      '.npc-evhead{display:flex;align-items:baseline;gap:5px}',
      '.npc-evtitle{font-weight:600;color:#dff3ff;line-height:1.45}',
      '.npc-evitem.major .npc-evtitle{color:#ffe6b0}',
      '.npc-evdesc{color:rgba(190,212,240,.75);font-size:11px;line-height:1.5;margin-top:3px}',
      '.npc-evtime{color:rgba(160,190,225,.6);font-size:10.5px;display:block;margin-top:3px}',
      '.npc-evempty{color:rgba(160,190,225,.6);font-size:11.5px;padding:6px 2px}',
      '.npc-evmeta{display:flex;align-items:baseline;gap:8px;margin-top:3px}',
      '.npc-evplace{color:rgba(143,214,255,.8);font-size:10.5px}',
      '.npc-evfocus{color:#ffc24d;font-size:10.5px;margin-left:auto;white-space:nowrap}',
      '@keyframes npc-ev-in{from{opacity:0;transform:translateX(-6px)}to{opacity:1;transform:none}}',
      // 覆盖层：头像 & 详情卡
      '.npc-overlay{position:absolute;inset:0;overflow:hidden;pointer-events:none;z-index:20}',
      '.npc-overlay .npc-el{position:absolute;left:0;top:0;will-change:transform}',
      // LingChat 角色：始终显示头像，比路人大
      '.npc-actor{transform-origin:50% 100%}',
      '.npc-actor .npc-ava{width:34px;height:34px;border-radius:50%;border:2px solid rgba(121,217,255,.9);box-shadow:0 0 12px rgba(121,217,255,.45),0 3px 10px rgba(0,0,0,.45);object-fit:cover;display:block}',
      '.npc-actor .npc-nm{margin-top:2px;font-size:11px;color:#dff3ff;background:rgba(8,16,26,.72);border:1px solid rgba(121,217,255,.35);border-radius:8px;padding:1px 7px;white-space:nowrap;backdrop-filter:blur(6px)}',
      '.npc-actor .npc-wrap{transform:translate(-50%,-100%);display:flex;flex-direction:column;align-items:center}',
      // 聚焦详情卡（点→卡升级）
      '.npc-card{transform-origin:50% 100%}',
      '.npc-card .npc-wrap{transform:translate(-50%,-100%);width:190px;background:rgba(255,255,255,.09);backdrop-filter:blur(20px) saturate(180%);border:1px solid rgba(255,194,77,.45);border-radius:14px;box-shadow:0 8px 32px rgba(0,0,0,.5),0 0 0 1px rgba(255,255,255,.06) inset;overflow:hidden;animation:npc-card-in .3s cubic-bezier(.2,.9,.3,1.2)}',
      '@keyframes npc-card-in{from{opacity:0;transform:translate(-50%,-90%) scale(.9)}to{opacity:1;transform:translate(-50%,-100%) scale(1)}}',
      '.npc-card .npc-chd{display:flex;gap:8px;padding:9px 10px;align-items:center}',
      '.npc-card .npc-ava{width:38px;height:38px;border-radius:50%;object-fit:cover;border:2px solid rgba(255,194,77,.9);box-shadow:0 0 12px rgba(255,194,77,.35);flex:0 0 auto}',
      '.npc-card .npc-mono{width:38px;height:38px;border-radius:50%;flex:0 0 auto;display:flex;align-items:center;justify-content:center;font-size:15px;font-weight:700;color:#0d1620;background:linear-gradient(135deg,#79d9ff,#4a9fd8)}',
      '.npc-card .npc-nm{font-size:13px;font-weight:700;color:#fff;line-height:1.3}',
      '.npc-card .npc-meta{font-size:10.5px;color:rgba(200,220,250,.8);margin-top:2px;line-height:1.45}',
      '.npc-card .npc-bubble{padding:7px 10px 9px;font-size:11px;line-height:1.5;color:#ffeecb;background:rgba(255,194,77,.10);border-top:1px solid rgba(255,194,77,.22)}',
      '.npc-card .npc-tag{position:absolute;top:-9px;right:6px;font-size:9.5px;padding:1px 7px;border-radius:7px;background:rgba(255,194,77,.9);color:#3a2600;font-weight:700}',
      '.npc-card .npc-bar{height:2px;background:rgba(255,194,77,.35)}',
      '.npc-card .npc-bar i{display:block;height:100%;background:#ffc24d;transition:width .3s linear}',
    ].join('\n');
    var st = d.createElement('style');
    st.id = STYLE_ID;
    st.textContent = css;
    (d.head || d.documentElement).appendChild(st);
  }

  /**
   * 生成事件流 HTML（参考 world_live.html 的「钦灵的动向」面板：玻璃拟态 + 左侧色条 + 时间戳）
   * @param {Array} list 事件数组（默认最新在上）
   * @param {Object} opts {limit=40, newestFirst=true, showDesc=true, emptyText}
   * @returns {String} HTML 片段
   */
  function renderEventFeed(list, opts) {
    opts = opts || {};
    injectStyles();
    var arr = (list || []).slice();
    if (opts.newestFirst !== false) arr.reverse();
    var limit = opts.limit == null ? 40 : opts.limit;
    if (limit > 0) arr = arr.slice(0, limit);
    if (!arr.length) {
      return '<div class="npc-evempty">' + esc(opts.emptyText || '暂无事件…') + '</div>';
    }
    var html = '';
    for (var i = 0; i < arr.length; i++) {
      var e = arr[i];
      var color = e.color || '#79d9ff';
      var d = new Date(e.ts || Date.now());
      var tstr = ('0' + d.getHours()).slice(-2) + ':' + ('0' + d.getMinutes()).slice(-2) + ':' + ('0' + d.getSeconds()).slice(-2);
      html += '<div class="npc-evitem' + (e.major ? ' major' : '') + '" style="border-left-color:' + color + '">' +
        '<div class="npc-evhead"><span>' + (e.icon || '•') + '</span>' +
        '<span class="npc-evtitle">' + esc(e.title || '') + '</span></div>';
      if (opts.showDesc !== false && e.desc) html += '<div class="npc-evdesc">' + esc(e.desc) + '</div>';
      var meta = [];
      if (e.placeName) meta.push('📍' + e.placeName);
      if (e.payload && e.payload.relation) meta.push('🔗' + e.payload.relation);
      if (e.focusedIds && e.focusedIds.length) meta.push('🔍聚焦×' + e.focusedIds.length);
      else if (e.major) meta.push('未聚焦');
      if (meta.length) html += '<div class="npc-evdesc" style="color:rgba(160,190,225,.7)">' + esc(meta.join(' · ')) + '</div>';
      html += '<span class="npc-evtime">' + tstr + ' · ' + esc(e.typeLabel || e.type || '') + '</span></div>';
    }
    return html;
  }
  /**
   * 把事件流挂到容器上（自动订阅 onEvent 增量刷新）
   * @returns {{el, update:Function, destroy:Function}}
   */
  function mountEventFeed(container, opts) {
    opts = opts || {};
    if (typeof container === 'string') container = document.getElementById(container);
    if (!container) return null;
    injectStyles();
    container.classList.add('npc-feed');
    if (opts.title && !container.querySelector('.npc-feed-h')) {
      var h = document.createElement('h4');
      h.className = 'npc-feed-h';
      h.textContent = opts.title;
      container.appendChild(h);
    }
    var body = document.createElement('div');
    body.className = 'npc-feed-body';
    container.appendChild(body);
    var self = { el: body, limit: opts.limit || 40, paused: false };
    self.update = function (list) {
      if (self.paused && !list) return;
      body.innerHTML = renderEventFeed(list || events, { limit: self.limit, emptyText: opts.emptyText });
    };
    self.unsubscribe = onEvent(function () {
      if (self.paused) return;
      self.update();
    });
    self.destroy = function () {
      self.unsubscribe();
      if (body.parentNode) body.parentNode.removeChild(body);
    };
    self.update();
    return self;
  }

  // ─────────────────── 16. DOM 覆盖层（头像 + 聚焦卡） ───────────────────
  /**
   * 创建覆盖层：负责「只有 LingChat 角色有头像」与「聚焦时升级为详情卡」的 DOM 呈现。
   * 每个渲染帧调用 overlay.render({toCanvas, camera}) 即可（位置随相机同步）。
   * @param {HTMLElement} container 定位父元素（position 需为 relative/absolute）
   * @param {Object} opts {showNamed=true, showFocus=true, scaleWithCamera=false}
   */
  function createOverlay(container, opts) {
    opts = opts || {};
    if (typeof container === 'string') container = document.getElementById(container);
    if (!container) return null;
    injectStyles();
    var root = document.createElement('div');
    root.className = 'npc-overlay';
    container.appendChild(root);
    var actors = {}, cards = {}, last = 0;
    function place(el, sx, sy) {
      el.style.transform = 'translate(' + Math.round(sx * 10) / 10 + 'px,' + Math.round(sy * 10) / 10 + 'px)';
    }
    function inView(sx, sy, cam) { return sx > -60 && sy > -100 && sx < cam.width + 60 && sy < cam.height + 120; }
    var overlay = {
      el: root,
      /** @param {Object} view {toCanvas, camera} */
      render: function (view) {
        var t = nowMs();
        if (config.overlayMinIntervalMs > 0 && t - last < config.overlayMinIntervalMs) return;
        last = t;
        var toCanvas = view.toCanvas || view.toCanvasFn;
        var cam = _normCamera(view.camera, { canvas: { width: root.clientWidth || 0, height: root.clientHeight || 0 } });
        var seenA = {}, seenC = {}, i;
        // ① LingChat 角色：始终显示头像（与聚焦无关）
        if (opts.showNamed !== false) {
          var named = getNamedCharacters();
          for (i = 0; i < named.length; i++) {
            var n = named[i];
            var p = _project(toCanvas, n.gridX, n.gridY);
            if (!p) continue;
            var sx = p.cx * cam.scale + cam.tx, sy = p.cy * cam.scale + cam.ty;
            if (!inView(sx, sy, cam)) continue;
            seenA[n.id] = 1;
            var el = actors[n.id];
            if (!el) {
              el = document.createElement('div');
              el.className = 'npc-el npc-actor';
              el.innerHTML = '<div class="npc-wrap"><img class="npc-ava" alt=""><div class="npc-nm"></div></div>';
              root.appendChild(el);
              actors[n.id] = el;
            }
            var img = el.querySelector('.npc-ava');
            if (n.avatarUrl && img.getAttribute('src') !== n.avatarUrl) img.setAttribute('src', n.avatarUrl);
            if (!n.avatarUrl && img.style.display !== 'none') img.style.display = 'none';
            var nm = el.querySelector('.npc-nm');
            if (nm.textContent !== n.name) nm.textContent = n.name;
            place(el, sx, sy);
          }
        }
        // ② 聚焦中的 NPC：点 → 详情卡（名字 / 头像 / 气泡）
        if (opts.showFocus !== false) {
          var foc = getFocused({ silent: true });
          for (i = 0; i < foc.length; i++) {
            var rec = foc[i], np = rec.npc;
            if (!np) continue;
            var p2 = _project(toCanvas, np.gridX, np.gridY);
            if (!p2) continue;
            var sx2 = p2.cx * cam.scale + cam.tx, sy2 = p2.cy * cam.scale + cam.ty;
            if (!inView(sx2, sy2, cam)) continue;
            seenC[np.id] = 1;
            var el2 = cards[np.id];
            if (!el2) {
              el2 = document.createElement('div');
              el2.className = 'npc-el npc-card';
              root.appendChild(el2);
              cards[np.id] = el2;
            }
            // 头像只给 LingChat 角色；普通 NPC 聚焦时用「首字圆形」占位（不假装有头像）
            var hasAva = np.kind === 'named' && np.avatarUrl;
            var avaHtml = hasAva
              ? '<img class="npc-ava" src="' + esc(np.avatarUrl) + '" alt="">'
              : '<div class="npc-mono">' + esc(String(np.name || '?').charAt(0)) + '</div>';
            var ev = rec.event;
            var bubble = ev ? '<div class="npc-bubble">' + (ev.icon || '') + ' ' + esc(ev.title || '') +
              (ev.desc ? '<div style="color:rgba(255,238,203,.8)">' + esc(ev.desc) + '</div>' : '') + '</div>' : '';
            var tag = ev && ev.major ? '<div class="npc-tag">重大事件</div>' : '<div class="npc-tag" style="background:rgba(121,217,255,.9);color:#04202e">手动聚焦</div>';
            var pct = Math.max(0, Math.min(100, Math.round((rec.remainMs || 0) / (rec.durationMs || 1) * 100)));
            var html = '<div class="npc-wrap">' + tag +
              '<div class="npc-chd">' + avaHtml +
              '<div><div class="npc-nm">' + esc(np.name) + '</div>' +
              '<div class="npc-meta">' + esc(np.activity || '') + ' · 心情' + esc(np.mood || '平静') +
              '<br>' + esc(np.place ? np.place.name : '街上') + '</div></div></div>' +
              bubble +
              '<div class="npc-bar"><i style="width:' + pct + '%"></i></div></div>';
            if (el2.__sig !== html) { el2.innerHTML = html; el2.__sig = html; }
            place(el2, sx2, sy2);
          }
        }
        for (var k in actors) {
          if (!seenA[k]) { if (actors[k].parentNode) actors[k].parentNode.removeChild(actors[k]); delete actors[k]; }
        }
        for (var k2 in cards) {
          if (!seenC[k2]) { if (cards[k2].parentNode) cards[k2].parentNode.removeChild(cards[k2]); delete cards[k2]; }
        }
        return { named: Object.keys(seenA).length, focused: Object.keys(seenC).length };
      },
      destroy: function () {
        if (root.parentNode) root.parentNode.removeChild(root);
        actors = {}; cards = {};
      },
    };
    return overlay;
  }

  // ─────────────────── 17. 导出 ───────────────────
  /** 时间戳 → HH:MM:SS */
  function fmtTs(ts) {
    var d = new Date(ts == null ? Date.now() : ts);
    function p2(n) { return (n < 10 ? '0' : '') + n; }
    return p2(d.getHours()) + ':' + p2(d.getMinutes()) + ':' + p2(d.getSeconds());
  }
  /**
   * 事件流 HTML 片段（玻璃拟态 + 左侧色条 + 时间戳）。
   * 重大事件条目带 major 高亮类；全部字段经 esc 转义防注入。
   * @param {Array|Object} list 事件数组，或 {events:[...]}，省略则用内部事件流
   * @param {Object} [opts] {limit} 最多渲染条数（默认 20）
   * @returns {string} HTML 片段
   */
  function renderEventFeed(list, opts) {
    opts = opts || {};
    var limit = opts.limit > 0 ? opts.limit : 20;
    var arr;
    if (Array.isArray(list)) arr = list;
    else if (list && Array.isArray(list.events)) arr = list.events;
    else arr = events;
    arr = arr.slice(Math.max(0, arr.length - limit));
    if (!arr.length) return '<div class="npc-evempty">还没有事件</div>';
    var out = '';
    for (var i = arr.length - 1; i >= 0; i--) {   // 最新在最上
      var e = arr[i] || {};
      var cls = 'npc-evitem' + (e.major ? ' major' : '');
      var color = e.color || '#79d9ff';
      // 位置标记（有 placeName 才显示）
      var place = e.placeName ? '<span class="npc-evplace">📍' + esc(e.placeName) + '</span>' : '';
      // 聚焦标记（重大事件聚焦了 N 人时显示 ×N）
      var fid = e.focusedIds || [];
      var focusTag = fid.length ? '<span class="npc-evfocus">🔍聚焦×' + fid.length + '</span>' : '';
      out += '<div class="' + cls + '" style="border-left-color:' + esc(color) + '">'
           + '<div class="npc-evhead">'
           + '<span class="npc-evicon">' + esc(e.icon || '•') + '</span>'
           + '<span class="npc-evtitle">' + esc(e.title || e.typeLabel || e.type || '') + '</span>'
           + focusTag
           + '</div>'
           + (e.desc ? '<div class="npc-evdesc">' + esc(e.desc) + '</div>' : '')
           + '<div class="npc-evmeta">' + place
           + '<span class="npc-evtime">' + esc(fmtTs(e.ts)) + '</span></div>'
           + '</div>';
    }
    return out;
  }

  return {
    version: VERSION,
    config: config, setConfig: setConfig, getConfig: getConfig,
    // 随机/工具
    makeRng: makeRng, setSeed: setSeed, rand: rand, randInt: randInt,
    clamp: clamp, esc: esc,
    // 时间
    timeStateOf: timeStateOf, normalizeTimeState: normalizeTimeState,
    parseHM: parseHM, hm: hm, periodOf: periodOf, PERIOD_ZH: PERIOD_ZH,
    fetchTimeState: fetchTimeState,
    // 地点/布局
    defaultLayout: defaultLayout, placesFromLayout: placesFromLayout,
    pickPlace: pickPlace, bindPools: bindPools, resolvePlace: resolvePlace,
    // 生成
    generateNpcs: generateNpcs, registerNpc: registerNpc, registerNpcs: registerNpcs,
    getNpc: getNpc, getAllNpcs: getAllNpcs, randomName: randomName,
    clearNpcIndex: clearNpcIndex,
    scheduleTemplate: scheduleTemplate, slotAt: slotAt, ageGroupOf: ageGroupOf,
    // T4-3 渲染 / 作息 / 移动 / 聚焦 / 头像
    drawCrowd: drawCrowd,
    updateNpcActivity: updateNpcActivity,
    tickNpcs: tickNpcs,
    focusNpc: focusNpc, unfocus: unfocus, getFocused: getFocused,
    clearFocus: clearFocus, isFocused: isFocused,
    setNamedCharacters: setNamedCharacters, getNamedCharacters: getNamedCharacters,
    getAvatarUrl: getAvatarUrl,
    // 事件
    triggerEvent: triggerEvent, getEvents: getEvents, clearEvents: clearEvents,
    onEvent: onEvent, randomIncident: randomIncident, eventStyle: eventStyle,
    EVENT_STYLE: EVENT_STYLE,
    // T4-4 关系 / 互动 / UI
    addRelation: addRelation, getRelations: getRelations, getRelation: getRelation,
    relationTypeBetween: relationTypeBetween, getRelationGraph: getRelationGraph,
    buildRelations: buildRelations, clearRelations: clearRelations, getAllRelations: getAllRelations,
    REL_TYPES: REL_TYPES,
    tickInteractions: tickInteractions,
    renderEventFeed: renderEventFeed, mountEventFeed: mountEventFeed,
    createOverlay: createOverlay, injectStyles: injectStyles,
    DOT: DOT, dotGroup: dotGroup,
    _debug: { events: events, relations: function () { return relations; } },
  };
})();
