// L-SYuki · 交通工具系统 前端模块 (T2-3)
// 与 transport.py 配套：算法/状态机在服务端，本模块只负责「画」与「选」。
//   1) MODES        交通工具常量表（图标/颜色/速度/适用距离），与 transport.py 的 MODES 同步
//   2) drawTrip     在地图上画一次出行（已走实线 + 剩余虚线 + 当前位置交通工具图标）
//   3) drawRoute    画「规划中的路线」（未出发，纯虚线 + 起终点 + 换乘点）
//   4) routeOptionsHTML / modeChipsHTML / tripPanelHTML / stepsHTML  出行选择 UI 片段
//   5) CSS          自带样式，不依赖外部样式表
// 用法：
//   var toXY = function (lng, lat) { return WORLD_COORD.lngLatToCanvas(lng, lat, bbox, W, H, 30); };
//   TRANSPORT.drawTrip(ctx, tripPayload, toXY);
//   panel.innerHTML = TRANSPORT.routeOptionsHTML(route.options, { selected: 0 });
window.TRANSPORT = (function () {
  // ── 1. 交通工具常量表 ──
  // 字段与 transport.py 的 MODES 对齐：icon/color 供前端绘制，speed_kmh/范围供提示文案。
  // 注意：这里只用于展示与兜底，真正的规划结果一律以服务端 transport.py 返回为准。
  var MODES = {
    walk:   { key: 'walk',   name: '步行',         en: 'walk',   icon: '🚶', color: '#8a8f98', speed_kmh: 4.5, min_km: 0,   max_km: 2,     need_transfer: false, desc: '短距离最灵活' },
    bike:   { key: 'bike',   name: '自行车/电动车', en: 'bike',   icon: '🚲', color: '#4caf7d', speed_kmh: 15,  min_km: 0.3, max_km: 8,     need_transfer: false, desc: '中短距离性价比最高' },
    bus:    { key: 'bus',    name: '公交',         en: 'bus',    icon: '🚌', color: '#f0a020', speed_kmh: 20,  min_km: 0.8, max_km: 30,    need_transfer: true,  desc: '覆盖最广、最便宜' },
    subway: { key: 'subway', name: '地铁',         en: 'subway', icon: '🚇', color: '#3d7bd6', speed_kmh: 35,  min_km: 1.5, max_km: 60,    need_transfer: true,  desc: '准点快速不堵车' },
    taxi:   { key: 'taxi',   name: '出租车/网约车', en: 'taxi',   icon: '🚕', color: '#f5c518', speed_kmh: 28,  min_km: 0.8, max_km: 200,   need_transfer: false, desc: '门到门、随叫随走' },
    car:    { key: 'car',    name: '私家车',       en: 'car',    icon: '🚗', color: '#7e57c2', speed_kmh: 45,  min_km: 1,   max_km: 500,   need_transfer: false, desc: '自由度高、可带行李' },
    train:  { key: 'train',  name: '火车/高铁',    en: 'train',  icon: '🚄', color: '#e05252', speed_kmh: 200, min_km: 30,  max_km: 1500,  need_transfer: true,  desc: '中长途主力，准点舒适' },
    plane:  { key: 'plane',  name: '飞机',         en: 'plane',  icon: '✈️', color: '#2fa8d6', speed_kmh: 750, min_km: 250, max_km: 12000, need_transfer: true,  desc: '超长途最快' },
    ferry:  { key: 'ferry',  name: '轮船/渡轮',    en: 'ferry',  icon: '⛴️', color: '#1f9ea8', speed_kmh: 22,  min_km: 0.5, max_km: 500,   need_transfer: true,  desc: '跨水域最直接' }
  };
  var MODE_ORDER = ['walk', 'bike', 'bus', 'subway', 'taxi', 'car', 'train', 'plane', 'ferry'];

  function mode(key) {
    return MODES[key] || { key: key, name: key || '未知', icon: '❓', color: '#999999', desc: '', speed_kmh: 0 };
  }
  function modeList() { return MODE_ORDER.map(function (k) { return MODES[k]; }); }

  // ── 2. 格式化（与 transport.py 的 fmt_* 输出一致）──
  function fmtDuration(min) {
    var m = Math.round(Number(min) || 0);
    if (m < 60) return m + '分钟';
    var h = Math.floor(m / 60), r = m % 60;
    return r ? h + '小时' + r + '分' : h + '小时';
  }
  function fmtDistance(meters) {
    var m = Number(meters) || 0;
    if (m < 1000) return Math.round(m) + '米';
    return (m / 1000).toFixed(1) + '公里';
  }
  function fmtCost(cost) {
    var c = Number(cost) || 0;
    if (c <= 0) return '免费';
    return c >= 10 ? '¥' + Math.round(c) : '¥' + c.toFixed(1);
  }
  // 优先用服务端给的 *_text，缺失时本地兜底
  function durText(r) { return (r && r.duration_text) || fmtDuration(r && r.duration_min); }
  function distText(r) { return (r && r.distance_text) || fmtDistance(r && r.distance_m); }
  function costText(r) { return (r && r.cost_text) || fmtCost(r && r.cost); }

  function esc(s) {
    return String(s == null ? '' : s)
      .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;').replace(/'/g, '&#39;');
  }

  // ── 3. 坐标适配 ──
  // toCanvasFn(lng,lat) 允许返回 {cx,cy}（WORLD_COORD 风格）/ {x,y} / [x,y]
  function toXY(fn, lng, lat) {
    if (typeof fn !== 'function') return null;
    var p = fn(lng, lat);
    if (!p) return null;
    if (Array.isArray(p)) return { x: p[0], y: p[1] };
    if (typeof p.cx === 'number') return { x: p.cx, y: p.cy };
    if (typeof p.x === 'number') return { x: p.x, y: p.y };
    return null;
  }

  // ── 4. 折线 ──
  // 服务端的 trip_payload 已带 polyline；若拿到的是 start_trip 的原始 trip，前端就地算一份。
  function polylineOf(trip) {
    if (!trip) return { done: [], remain: [] };
    if (trip.polyline && trip.polyline.done) return trip.polyline;
    var phases = trip.phases || [];
    if (!phases.length) return { done: [trip.from], remain: [trip.to] };
    var cur = trip.current_phase || 0;
    var done = [trip.from], remain = [trip.position || trip.from];
    var i, p;
    for (i = 0; i < phases.length; i++) {
      p = phases[i];
      if (p.kind !== 'ride') continue;
      if (i < cur) done.push(p.to);
      else if (i === cur) done.push(trip.position || p.to);
    }
    for (i = 0; i < phases.length; i++) {
      p = phases[i];
      if (p.kind !== 'ride') continue;
      if (i > cur) { remain.push(p.from); remain.push(p.to); }
      else if (i === cur) remain.push(p.to);
    }
    remain.push(trip.to);
    return { done: done, remain: remain };
  }

  // 当前所处环节（坐地铁时显示 🚇，步行接驳时显示 🚶）
  function currentPhase(trip) {
    var ph = (trip && trip.phases) || [];
    if (!ph.length) return null;
    var i = Math.min(Math.max(trip.current_phase || 0, 0), ph.length - 1);
    return ph[i] || null;
  }
  function currentModeKey(trip) {
    var p = currentPhase(trip);
    if (p && p.mode) return p.mode;          // 候车/换乘阶段 mode 为 null → 退回主方式
    return (trip && trip.mode) || 'walk';
  }

  // ── 5. 绘制 ──
  var DEFAULTS = {
    lineWidth: 3, dash: [7, 6], dashWidth: 3,
    color: null, iconSize: 22, iconBubble: true,
    showEndpoints: true, showCurrent: true, showIcon: true,
    endpointRadius: 5, alpha: 1, shadow: true,
    startLabel: '起点', endLabel: '终点', labelFont: '11px sans-serif',
    pulse: 0            // 0~1，外部按帧传入，做当前位置呼吸圈动效
  };
  function opt(o, d) {
    o = o || {};
    var r = {}, k;
    for (k in d) if (Object.prototype.hasOwnProperty.call(d, k)) r[k] = o[k] === undefined ? d[k] : o[k];
    return r;
  }

  function path(ctx, pts, fn) {
    var started = false;
    for (var i = 0; i < pts.length; i++) {
      var pt = pts[i];
      if (!pt) continue;
      var p = toXY(fn, pt[0], pt[1]);
      if (!p) continue;
      if (!started) { ctx.moveTo(p.x, p.y); started = true; } else ctx.lineTo(p.x, p.y);
    }
    return started;
  }

  /**
   * 在地图上画一次出行。
   * @param ctx        Canvas 2D 上下文
   * @param trip       transport.trip_payload(...) 的返回值（或 start_trip 的原始 trip）
   * @param toCanvasFn function(lng,lat) -> {cx,cy} | {x,y} | [x,y]
   * @param options    可选，见 DEFAULTS
   * @returns {{drawn:number, current:object|null, progress:number|null, status:string|null}}
   */
  function drawTrip(ctx, trip, toCanvasFn, options) {
    if (!ctx || !trip) return null;
    var o = opt(options, DEFAULTS);
    var pl = polylineOf(trip);
    var color = o.color || mode(currentModeKey(trip)).color;
    var box = { drawn: 0, current: null };

    ctx.save();
    ctx.globalAlpha = o.alpha;
    ctx.lineJoin = 'round';
    ctx.lineCap = 'round';

    // 剩余路径：虚线（黑色浅描边打底，深浅色地图上都看得清）
    if (pl.remain && pl.remain.length > 1) {
      if (o.shadow) {
        ctx.save();
        ctx.setLineDash(o.dash); ctx.lineWidth = o.dashWidth + 3;
        ctx.strokeStyle = 'rgba(0,0,0,0.25)';
        ctx.beginPath();
        if (path(ctx, pl.remain, toCanvasFn)) ctx.stroke();
        ctx.restore();
      }
      ctx.save();
      ctx.setLineDash(o.dash); ctx.lineWidth = o.dashWidth;
      ctx.strokeStyle = color;
      ctx.beginPath();
      if (path(ctx, pl.remain, toCanvasFn)) { ctx.stroke(); box.drawn++; }
      ctx.restore();
    }

    // 已走路径：实线
    if (pl.done && pl.done.length > 1) {
      if (o.shadow) {
        ctx.save();
        ctx.setLineDash([]); ctx.lineWidth = o.lineWidth + 3;
        ctx.strokeStyle = 'rgba(0,0,0,0.25)';
        ctx.beginPath();
        if (path(ctx, pl.done, toCanvasFn)) ctx.stroke();
        ctx.restore();
      }
      ctx.save();
      ctx.setLineDash([]); ctx.lineWidth = o.lineWidth;
      ctx.strokeStyle = color;
      ctx.beginPath();
      if (path(ctx, pl.done, toCanvasFn)) { ctx.stroke(); box.drawn++; }
      ctx.restore();
    }

    // 起终点标记
    if (o.showEndpoints) {
      var s = toXY(toCanvasFn, trip.from[0], trip.from[1]);
      var e = toXY(toCanvasFn, trip.to[0], trip.to[1]);
      if (s) {
        circle(ctx, s.x, s.y, o.endpointRadius, '#ffffff', color, 2);
        dotLabel(ctx, o.startLabel, s.x, s.y, o.labelFont);
      }
      if (e) {
        circle(ctx, e.x, e.y, o.endpointRadius + 1, color, '#ffffff', 2);
        dotLabel(ctx, o.endLabel, e.x, e.y, o.labelFont);
      }
    }

    // 当前位置：交通工具图标（+ 底色气泡 + 呼吸圈）
    if (o.showCurrent && trip.position) {
      var c = toXY(toCanvasFn, trip.position[0], trip.position[1]);
      if (c) {
        var mk = mode(currentModeKey(trip));
        if (o.pulse > 0) {
          ctx.save();
          ctx.globalAlpha = o.alpha * (1 - o.pulse) * 0.5;
          ctx.beginPath();
          ctx.arc(c.x, c.y, (o.iconSize * 0.55) + o.pulse * o.iconSize, 0, Math.PI * 2);
          ctx.fillStyle = mk.color; ctx.fill();
          ctx.restore();
        }
        if (o.iconBubble) circle(ctx, c.x, c.y, o.iconSize * 0.78, '#ffffff', mk.color, 2.5);
        if (o.showIcon) {
          ctx.save();
          ctx.font = o.iconSize + 'px "Apple Color Emoji","Noto Color Emoji",sans-serif';
          ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
          ctx.fillText(mk.icon, c.x, c.y + 1);
          ctx.restore();
        }
        box.current = { x: c.x, y: c.y, mode: mk.key, icon: mk.icon, color: mk.color };
      }
    }
    ctx.restore();
    box.progress = trip.progress != null ? trip.progress : null;
    box.status = trip.status || null;
    return box;
  }

  /** 画「规划中的路线」（尚未出发）：纯虚线 + 起终点 + 换乘节点 */
  function drawRoute(ctx, route, toCanvasFn, options) {
    if (!ctx || !route) return null;
    var o = opt(options, DEFAULTS);
    var color = o.color || mode(route.mode).color;
    var pts = (route.steps && route.steps.length)
      ? [route.steps[0].from].concat(route.steps.map(function (s) { return s.to; }))
      : [route.from, route.to];

    ctx.save();
    ctx.lineJoin = 'round'; ctx.lineCap = 'round';
    if (o.shadow) {
      ctx.save();
      ctx.setLineDash(o.dash); ctx.lineWidth = o.dashWidth + 3;
      ctx.strokeStyle = 'rgba(0,0,0,0.25)';
      ctx.beginPath();
      if (path(ctx, pts, toCanvasFn)) ctx.stroke();
      ctx.restore();
    }
    ctx.setLineDash(o.dash); ctx.lineWidth = o.dashWidth; ctx.strokeStyle = color;
    ctx.beginPath();
    if (path(ctx, pts, toCanvasFn)) ctx.stroke();

    // 换乘节点小圆点
    ctx.setLineDash([]);
    if (route.steps && route.steps.length > 1) {
      for (var i = 1; i < route.steps.length; i++) {
        var p = toXY(toCanvasFn, route.steps[i].from[0], route.steps[i].from[1]);
        if (p) circle(ctx, p.x, p.y, 4, '#ffffff', mode(route.steps[i].mode).color, 2);
      }
    }
    var s = toXY(toCanvasFn, route.from[0], route.from[1]);
    var e = toXY(toCanvasFn, route.to[0], route.to[1]);
    if (o.showEndpoints) {
      if (s) circle(ctx, s.x, s.y, o.endpointRadius, '#ffffff', color, 2);
      if (e) circle(ctx, e.x, e.y, o.endpointRadius + 1, color, '#ffffff', 2);
    }
    // 路线中点放一个交通工具图标
    if (o.showIcon && s && e) {
      var mx = (s.x + e.x) / 2, my = (s.y + e.y) / 2, mk = mode(route.mode);
      circle(ctx, mx, my, o.iconSize * 0.75, '#ffffff', mk.color, 2.5);
      ctx.font = o.iconSize + 'px "Apple Color Emoji","Noto Color Emoji",sans-serif';
      ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
      ctx.fillText(mk.icon, mx, my + 1);
    }
    ctx.restore();
    return { steps: route.steps ? route.steps.length : 1 };
  }

  function circle(ctx, x, y, r, fill, stroke, lw) {
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    if (fill) { ctx.fillStyle = fill; ctx.fill(); }
    if (stroke) { ctx.lineWidth = lw || 1; ctx.strokeStyle = stroke; ctx.stroke(); }
  }
  function dotLabel(ctx, text, x, y, font) {
    if (!text || typeof ctx.strokeText !== 'function') return;
    ctx.save();
    ctx.font = font; ctx.textAlign = 'center'; ctx.textBaseline = 'bottom';
    ctx.lineWidth = 3; ctx.strokeStyle = 'rgba(255,255,255,0.9)';
    ctx.strokeText(text, x, y - 8);
    ctx.fillStyle = '#333333'; ctx.fillText(text, x, y - 8);
    ctx.restore();
  }

  // ── 6. 出行选择 UI ──
  /** 生成「推荐方案」列表 HTML。options 传服务端 plan_options 的结果（或 route.options） */
  function routeOptionsHTML(options, opts) {
    opts = opts || {};
    var list = options || [];
    if (!list.length) return '<div class="tp-empty">没有可用的出行方案</div>';
    var h = [], i;
    if (opts.title !== false) {
      h.push('<div class="tp-title">' + esc(opts.title || '推荐方案') +
             '<span class="tp-sub">' + list.length + ' 条 · 按' +
             esc(opts.sortName || '综合最优') + '排序</span></div>');
    }
    h.push('<div class="tp-options">');
    for (i = 0; i < list.length; i++) {
      h.push(optionCardHTML(list[i], {
        index: i,
        recommended: i === 0 && opts.markFirst !== false,
        selected: opts.selected === i,
        showReason: opts.showReason !== false,
        showSteps: opts.showSteps !== false,
        action: opts.action
      }));
    }
    h.push('</div>');
    return h.join('');
  }

  /** 单条方案卡片 */
  function optionCardHTML(route, o) {
    o = o || {};
    if (!route) return '';
    var mk = mode(route.mode);
    var steps = (route.steps || []).map(function (s) { return mode(s.mode).icon; })
      .join('<span class="tp-arrow">›</span>');
    var badges = [];
    if (o.recommended) badges.push('<span class="tp-badge tp-badge-rec">推荐</span>');
    if (route.cross_water) badges.push('<span class="tp-badge tp-badge-water">跨水</span>');
    if (route.transfer_count > 0) {
      badges.push('<span class="tp-badge">' + esc(route.transfer_text || ('换乘' + route.transfer_count + '次')) + '</span>');
    } else {
      badges.push('<span class="tp-badge tp-badge-ok">' + esc(route.transfer_text || '直达') + '</span>');
    }
    return '' +
      '<div class="tp-card' + (o.selected ? ' tp-card-sel' : '') + '"' +
        ' data-index="' + (o.index || 0) + '" data-mode="' + esc(route.mode) + '"' +
        (o.action ? ' data-action="' + esc(o.action) + '"' : '') + '>' +
        '<div class="tp-card-main">' +
          '<div class="tp-icon" style="background:' + esc(mk.color) + '22;border-color:' + esc(mk.color) + '">' +
            esc(mk.icon) + '</div>' +
          '<div class="tp-info">' +
            '<div class="tp-name">' + esc(route.mode_name || mk.name) + badges.join('') + '</div>' +
            (o.showSteps !== false && steps ? '<div class="tp-steps">' + steps + '</div>' : '') +
            (o.showReason !== false && route.reason ? '<div class="tp-reason">' + esc(route.reason) + '</div>' : '') +
          '</div>' +
          '<div class="tp-metrics">' +
            '<div class="tp-dur">' + esc(durText(route)) + '</div>' +
            '<div class="tp-cost">' + esc(costText(route)) + '</div>' +
            '<div class="tp-dist">' + esc(distText(route)) + '</div>' +
          '</div>' +
        '</div>' +
      '</div>';
  }

  /** 交通工具选择条（点击可强制指定 prefer） */
  function modeChipsHTML(modes, activeKey, o) {
    o = o || {};
    var list = modes || modeList();
    var h = ['<div class="tp-chips">'], i;
    if (o.autoLabel !== false) {
      h.push('<button class="tp-chip' + (!activeKey ? ' tp-chip-on' : '') + '" data-mode="">' +
             esc(o.autoLabel || '🤖 自动') + '</button>');
    }
    for (i = 0; i < list.length; i++) {
      var m = mode(list[i].key || list[i]);
      h.push('<button class="tp-chip' + (activeKey === m.key ? ' tp-chip-on' : '') +
             '" data-mode="' + esc(m.key) + '" title="' +
             esc(m.name + ' · 约' + m.speed_kmh + 'km/h · ' + m.desc) + '">' +
             esc(m.icon + ' ' + m.name) + '</button>');
    }
    h.push('</div>');
    return h.join('');
  }

  /** 出行进行中面板（进度 / 剩余 / 当前环节 / 速度 / 费用） */
  function tripPanelHTML(trip, o) {
    if (!trip) return '';
    o = o || {};
    var mk = mode(currentModeKey(trip));
    var p = currentPhase(trip);
    var pct = Math.round((trip.progress || 0) * 100);
    var doneM = Math.max(0, (trip.distance_m || 0) - (trip.remaining_m || 0));
    var stateZh = { moving: '前往中', waiting: '等待中', arrived: '已到达' }[trip.status] || (trip.status || '');
    return '' +
      '<div class="tp-trip">' +
        '<div class="tp-trip-head">' +
          '<span class="tp-trip-icon" style="background:' + esc(mk.color) + '22;border-color:' + esc(mk.color) + '">' +
            esc(mk.icon) + '</span>' +
          '<span class="tp-trip-title">' + esc(trip.mode_name || mk.name) +
            (trip.label ? ' · ' + esc(trip.label) : '') + '</span>' +
          '<span class="tp-trip-state tp-state-' + esc(trip.status) + '">' + esc(stateZh) + '</span>' +
        '</div>' +
        '<div class="tp-bar"><i style="width:' + pct + '%;background:' + esc(mk.color) + '"></i></div>' +
        '<div class="tp-trip-rows">' +
          '<span>剩余 <b>' + esc(trip.remaining_text || fmtDuration(trip.remaining_min)) + '</b></span>' +
          '<span>已走 <b>' + esc(fmtDistance(doneM)) + '</b></span>' +
          '<span>进度 <b>' + pct + '%</b></span>' +
          '<span>速度 <b>' + Math.round(trip.current_speed_kmh || 0) + ' km/h</b></span>' +
          '<span>费用 <b>' + esc(costText(trip)) + '</b></span>' +
        '</div>' +
        (p && p.note ? '<div class="tp-trip-note">当前：' + esc(p.mode_name || '') + ' · ' + esc(p.note) + '</div>' : '') +
        (o.showSummary !== false && trip.summary ? '<div class="tp-trip-sum">' + esc(trip.summary) + '</div>' : '') +
      '</div>';
  }

  /** 行程明细表（steps 分段） */
  function stepsHTML(route) {
    var steps = (route && route.steps) || [];
    if (!steps.length) return '';
    var h = ['<div class="tp-steps-list">'];
    for (var i = 0; i < steps.length; i++) {
      var s = steps[i], m = mode(s.mode);
      h.push('<div class="tp-step">' +
        '<span class="tp-step-icon">' + esc(m.icon) + '</span>' +
        '<span class="tp-step-name">' + esc(s.mode_name || m.name) + '</span>' +
        '<span class="tp-step-dist">' + esc(fmtDistance(s.distance_m)) + '</span>' +
        '<span class="tp-step-dur">' + esc(fmtDuration(s.duration_min)) + '</span>' +
        '<span class="tp-step-cost">' + esc(fmtCost(s.cost)) + '</span>' +
        '<span class="tp-step-note">' + esc(s.note || '') + '</span>' +
      '</div>');
    }
    h.push('</div>');
    return h.join('');
  }

  // ── 7. 自带样式 ──
  var CSS = [
    '.tp-options{display:flex;flex-direction:column;gap:8px}',
    '.tp-title{font:600 13px/1.6 system-ui,sans-serif;color:#333;margin-bottom:4px}',
    '.tp-title .tp-sub{font-weight:400;color:#888;margin-left:8px;font-size:12px}',
    '.tp-card{border:1px solid #e3e6ea;border-radius:10px;padding:10px 12px;background:#fff;cursor:pointer;transition:.15s}',
    '.tp-card:hover{border-color:#9bb7dd;box-shadow:0 2px 8px rgba(0,0,0,.06)}',
    '.tp-card-sel{border-color:#3d7bd6;background:#f4f8ff;box-shadow:0 0 0 2px #3d7bd622}',
    '.tp-card-main{display:flex;align-items:center;gap:10px}',
    '.tp-icon{width:38px;height:38px;flex:0 0 38px;border-radius:9px;border:1px solid;display:flex;align-items:center;justify-content:center;font-size:20px}',
    '.tp-info{flex:1;min-width:0}',
    '.tp-name{font:600 14px/1.4 system-ui,sans-serif;color:#222}',
    '.tp-steps{font-size:13px;color:#888;margin-top:2px}',
    '.tp-steps .tp-arrow{margin:0 3px;color:#bbb}',
    '.tp-reason{font-size:11.5px;color:#999;margin-top:2px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}',
    '.tp-metrics{text-align:right;flex:0 0 auto}',
    '.tp-dur{font:600 15px/1.3 system-ui,sans-serif;color:#222}',
    '.tp-cost{font-size:12.5px;color:#e0713a}',
    '.tp-dist{font-size:11.5px;color:#999}',
    '.tp-badge{display:inline-block;font-size:10.5px;padding:1px 6px;border-radius:8px;margin-left:6px;background:#f0f2f5;color:#666;vertical-align:middle}',
    '.tp-badge-rec{background:#3d7bd6;color:#fff}',
    '.tp-badge-water{background:#1f9ea8;color:#fff}',
    '.tp-badge-ok{background:#e8f5ec;color:#2e8b57}',
    '.tp-chips{display:flex;flex-wrap:wrap;gap:6px;margin-bottom:8px}',
    '.tp-chip{font-size:12px;padding:5px 10px;border-radius:16px;border:1px solid #dde1e6;background:#fff;color:#555;cursor:pointer}',
    '.tp-chip:hover{border-color:#9bb7dd}',
    '.tp-chip-on{background:#3d7bd6;border-color:#3d7bd6;color:#fff}',
    '.tp-empty{color:#999;font-size:13px;padding:10px}',
    '.tp-trip{border:1px solid #e3e6ea;border-radius:10px;padding:10px 12px;background:#fff}',
    '.tp-trip-head{display:flex;align-items:center;gap:8px}',
    '.tp-trip-icon{width:30px;height:30px;border-radius:8px;border:1px solid;display:flex;align-items:center;justify-content:center;font-size:16px}',
    '.tp-trip-title{font:600 14px system-ui,sans-serif;color:#222;flex:1}',
    '.tp-trip-state{font-size:11.5px;padding:2px 8px;border-radius:9px;background:#f0f2f5;color:#666}',
    '.tp-state-moving{background:#e8f2ff;color:#3d7bd6}',
    '.tp-state-waiting{background:#fff5e0;color:#b8860b}',
    '.tp-state-arrived{background:#e8f5ec;color:#2e8b57}',
    '.tp-bar{height:5px;border-radius:3px;background:#eef0f3;margin:9px 0;overflow:hidden}',
    '.tp-bar i{display:block;height:100%;border-radius:3px;transition:width .3s linear}',
    '.tp-trip-rows{display:flex;flex-wrap:wrap;gap:10px;font-size:12px;color:#777}',
    '.tp-trip-rows b{color:#222;font-weight:600}',
    '.tp-trip-note{font-size:11.5px;color:#999;margin-top:6px}',
    '.tp-trip-sum{font-size:11.5px;color:#aaa;margin-top:3px}',
    '.tp-steps-list{margin-top:8px}',
    '.tp-step{display:flex;align-items:center;gap:8px;font-size:12px;color:#666;padding:4px 0;border-top:1px dashed #eef0f3}',
    '.tp-step-icon{font-size:15px}',
    '.tp-step-name{flex:0 0 auto;color:#333}',
    '.tp-step-dist,.tp-step-dur,.tp-step-cost{flex:0 0 auto}',
    '.tp-step-dur{color:#222;font-weight:600}',
    '.tp-step-cost{color:#e0713a}',
    '.tp-step-note{flex:1;text-align:right;color:#aaa;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}'
  ].join('\n');

  /** 把自带样式注入文档（懒执行，缺失样式也不影响功能） */
  function injectCSS(doc) {
    doc = doc || (typeof document !== 'undefined' ? document : null);
    if (!doc || !doc.createElement) return false;
    if (doc.getElementById && doc.getElementById('tp-css')) return true;
    var st = doc.createElement('style');
    st.id = 'tp-css'; st.textContent = CSS;
    (doc.head || doc.documentElement).appendChild(st);
    return true;
  }

  return {
    MODES: MODES, MODE_ORDER: MODE_ORDER, mode: mode, modeList: modeList,
    fmtDuration: fmtDuration, fmtDistance: fmtDistance, fmtCost: fmtCost,
    durText: durText, distText: distText, costText: costText,
    toXY: toXY, polylineOf: polylineOf, currentPhase: currentPhase, currentModeKey: currentModeKey,
    drawTrip: drawTrip, drawRoute: drawRoute,
    routeOptionsHTML: routeOptionsHTML, optionCardHTML: optionCardHTML,
    modeChipsHTML: modeChipsHTML, tripPanelHTML: tripPanelHTML, stepsHTML: stepsHTML,
    CSS: CSS, injectCSS: injectCSS
  };
})();
