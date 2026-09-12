# 08 · 与 LingChat 功能适配 & 界面摆放方案

> 回答三个具体问题：**① 怎么和 0.1.3 已有功能融合 ② 人物头像/立绘怎么适配 ③ 地图摆在哪**
>
> 全部结论来自对 `~/lingchat-main`（v0.1.3）源码与 `~/lingchat-data` 实际数据的调研。

---

## 一、LingChat 0.1.3 现有结构与可挂载点

### 界面分层（`MainChat.vue`，即 `/chat`）

```
div.main-box                        ← 层叠上下文
├── GameBackground.vue              背景（z-index 最低，含粒子/特效）
├── GameRolesStage.vue              角色立绘舞台（多角色站位）
├── GameDialog.vue                  对话气泡
├── div#menu-panel                  菜单（z-index:100）
├── GameExtraUI.vue                 附加 UI
└── LoadingTransition               过场
```

**地图可插入的位置有 6 个**，见第三节。

### 角色与情绪系统（已有，直接复用）

| 能力 | 实现 | 地图如何用 |
|---|---|---|
| 角色列表 | `characterGetAll()` → `invoke('get_character_list')` | 地图上只给**这些角色**头像（路人只是点） |
| 表情立绘 | `characters/<角色>/avatar/<表情>.webp`，**21 种**：正常/高兴/生气/害羞/伤心/兴奋/厌恶/害怕/平静/心动/惊讶/慌张/担心/无奈/疑惑/紧张/羞耻/自信/认真/调皮/头像 | 地图上的角色头像随情绪切换 |
| 取图命令 | `invoke('get_avatar_file', { characterFolder, emotion, clothesName })` | 直接用，无需新接口 |
| 情绪映射 | `EMOTION_CONFIG_EMO[emotion] \|\| '正常'`（`@/controllers/emotion/config`） | 地图标记复用同一映射，保证表情一致 |
| 头像组件 | `GameRoleAvatar.vue`（含淡入动画、气泡、去重逻辑） | 可直接嵌进地图标记，或用简化版 |
| 角色站位 | `GameRolesStage.vue` + `settings.yml` 的 `offset_x/offset_y/scale` | **地图站位**需要另一套（按地理坐标），但缩放/偏移字段可复用 |

### 其它可融合的功能

| LingChat 功能 | 融合点 |
|---|---|
| 剧本模式 | 剧本步骤可带「地点」→ 地图自动跳到该地点 |
| 日程系统 | `game_data/schedules.json` → 角色此刻该在地图哪里（后端已实现 `schedule.py`） |
| 情绪系统 | 情绪变化 → 地图标记的表情同步变 |
| 记忆系统 | 记忆条目关联地点（「在越秀公园发生的事」）→ 地图上可回看 |
| 主动系统 | 世界事件写 `game_data/world_map/events.json` → 主动搭话素材 |
| 桌宠模式 | 地图作为桌宠的「活动范围」，可限制在某个区域 |
| 设置面板 | 地图设置：风格、细节密度、是否显示路名、图层默认开关 |

---

## 二、人物头像 / 立绘适配方案

### 三层角色表现

| 场景 | 用什么 | 尺寸 | 来源 |
|---|---|---|---|
| **地图标记**（远看） | 圆形头像 | 24–36px | `get_avatar_file(folder, '头像', clothes)` |
| **地图标记**（近看/聚焦） | 表情头像 + 名字 | 44–64px | `get_avatar_file(folder, EMOTION_CONFIG_EMO[emotion], clothes)` |
| **角色舞台**（聊天界面） | 全身立绘 | 沿用现有 `GameRolesStage` | 同上（不同 emotion 文件） |

### 规则（呼应你之前的要求）

1. **只有 LingChat 角色列表里的角色有头像**——其余 NPC 一律是圆点（原型已实现 `setNamedCharacters` 逻辑）
2. **头像随情绪变**——情绪系统推送新情绪时，地图标记同步换图（复用 `EMOTION_CONFIG_EMO`）
3. **路人不是隐形**——用低饱和小点表示，密度随区域真实人口估算变化
4. **缓存**——头像路径按 `folder|emotion|clothes` 缓存，避免每帧 invoke

### 实现要点

```ts
// 复用现有命令，不新增接口
const path = await invoke<string>('get_avatar_file', {
  characterFolder: c.folder, emotion: EMOTION_CONFIG_EMO[emotion] || '正常', clothesName,
})
const url = convertFileSrc(path)
```

地图标记组件 `MapActor.vue`：
```vue
<div class="actor" :style="{ left: x + '%', top: y + '%' }">
  <img :src="avatarUrl" :alt="name" />   <!-- 无头像时退化为 .dot -->
  <span class="name">{{ name }}</span>
</div>
```

---

## 三、地图摆放方案（重点）

### 六层挂载位（从低到高）

| # | 位置 | 组件 | 用途 | 干扰度 |
|---|---|---|---|---|
| 1 | **世界底图层** | `WorldMapLayer mode="overlay"` | 地图铺满作世界背景（半透明），角色立绘站在上面 → RPG 沉浸感 | 低（可调透明度/关闭） |
| 2 | 背景层（原有） | `GameBackground` | 与地图**二选一或叠加**（地图在下、背景特效在上） | — |
| 3 | 角色舞台 | `GameRolesStage` | 角色立绘（按地图坐标站位或沿用原站位） | — |
| 4 | 对话/UI | `GameDialog` / `#menu-panel` | 原有，不动 | — |
| 5 | **角落小窗** | `WorldMapLayer mode="corner"` | 常驻迷你地图（导航用，可拖动/收起） | 低 |
| 6 | **悬浮手机** | `PhoneOverlay`（phone.js） | 地图上的手机 UI | 低 |

外加一个**独立路由页** `/world`（`WorldMap.vue`）用来「专心看地图 / 下钻 / 生成小区」。

### 三种呈现形态的取舍

| 形态 | 何时用 | 优点 | 代价 |
|---|---|---|---|
| **全屏页** `/world` | 想认真看地图、下钻、生成小区 | 空间充足、细节全开 | 与聊天分离 |
| **背景层** overlay | 日常聊天时 | 沉浸感（角色站在世界里） | 细节看不清，需控制透明度（建议 0.15–0.35） |
| **角落小窗** corner | 随时瞄一眼 | 不挡聊天 | 只能看大概位置 |

**默认建议**：日常 = 背景层（低透明度）+ 角落小窗（可选）；看地图 = 全屏页。

### 「角色站在地图上」怎么定坐标

这是最关键的一步。链路：

```
角色日程（schedules.json）
   → 活动文本（"在公司上班"）
   → 设施类型（commercial）           ← schedule.py 已实现
   → 该类型的具体设施点（grid 坐标）    ← facilities 已实现
   → 网格坐标 → 世界坐标 → 屏幕坐标     ← coord.rs 已实现
   → 立绘/头像绝对定位
```

**同角色稳定命中同一设施**（用角色名 hash 做种子），避免每帧乱跳。

**兜底**：没有日程时按「在家」处理；在途（通勤）时显示为移动中的小点。

### 与剧本模式结合

剧本步骤可带 `place` 字段（如「越秀公园」）→ 触发时：
1. 地图跳到该地点（缩放定位）
2. 角色标记移动到那里（带过渡动画）
3. 对话气泡出现在角色旁

---

## 四、设置项（放进现有设置面板的「世界」分区）

| 设置 | 类型 | 默认 |
|---|---|---|
| 世界地图 | 开关（关闭/背景层/小窗） | 关闭 |
| 地图透明度 | 滑块 0.05–0.9 | 0.25 |
| 地图风格 | 高德 / 暗色 / 手绘 | 高德 |
| 细节层级上限 | 远景 L1 / 中景 L2 / 近景 L3 | L3 |
| 显示路名 | 开关 | 开 |
| 显示建筑名 | 开关 | 开 |
| 显示角色头像 | 开关（关则只显示点） | 开 |
| 路人密度 | 低 / 中 / 高 | 中 |
| 自动跟随日程 | 开关（角色按日程移动） | 开 |
| 缓存上限 | 数值（MB），超出按 LRU 清理 | 300 |

---

## 五、落地顺序（与 07 方案对接）

在 07 的**阶段 1（核心渲染链）**之上，本节内容按这样插进去：

1. **先把 `/world` 全屏页跑通**（阶段 1 的 WorldMapDrill / DistrictLive）
2. **再做 `MapActor.vue`**（角色标记 + 头像复用）——依赖 ①
3. **再接背景层/小窗**（`WorldMapLayer` 已存在，补角色层与设置项）
4. **最后做剧本/日程联动**（依赖 schedule.rs 移植，属阶段 2）

> 关键排序理由：**先把「地图本身」做对，再让它去适应界面**。
> 反过来的话（先塞进聊天界面）会被布局问题牵着走，模型都跑不通。

---

> 本文档随实现进展更新；全部文档索引见 `docs/world-map/README.md`。
