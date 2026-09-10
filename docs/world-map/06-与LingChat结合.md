# 06 · 与 LingChat 现有能力的结合

## 结合点一览

| LingChat 能力 | 位置 | 结合方式 |
|---|---|---|
| `useHeartbeat` 心跳 | `src/composables/useHeartbeat.ts` | 用户活跃度影响角色是否"在家等你"、离开多久触发"想念"移动 |
| `proactive_system` 的 `UserState` | `proactive_system/types.rs` | 直接映射角色在地图上的位置与活动 |
| `ProactiveEvent` | 同上（`ts_ms`/`kind`/`preview`）| 转化为地图事件与足迹 |
| `get_character_list` / `get_role_info` | `api/services/character.ts` | **只有角色列表中的角色显示头像**，其余为路人点 |
| 日程系统 | `schedule_manager.rs` / `schedule.ts` | 角色按作息走动（上班/吃饭/回家）|
| 记忆系统 | `persistent_memory_system` | 空间记忆："常去的地方"可写入角色记忆 |
| 网易云 / B 站 | `netmusic_service` / `bilibili_service` | 通过悬浮手机接入 |

### UserState → 地图活动的映射示例

| UserState | 角色行为 |
|---|---|
| IDLE | 在家 / 附近闲逛 |
| BROWSING | 在城市各处游走 |
| WORK | 在写字楼 / 书房 |
| GAME | 在娱乐场所 / 冒险地点 |
| CASUAL | 公园 / 海边 / 咖啡馆 |
| （Miss 意图）| 移动到"你们有回忆的地方"等你 |

**关键**：这不是新增一套状态，而是**给已有状态增加空间表达**。

## 落地路径（降低集成风险）

建议分三步，每步都能独立验证：

### 第一步：真实地理下钻（无 AI 依赖，风险最低）
- 世界 → 省 → 市 → 区县
- 纯 GeoJSON 数据 + 服务端渲染，不需要 LLM
- 能先看到"我们的世界长什么样"

### 第二步：区县 → AI 街区
- 引入 LLM 生成街区布局
- 逐步增加建筑 / 设施密度
- 加入"保留 / 重画"交互

### 第三步：世界模拟
- 时间（真实时间驱动昼夜）
- 天气（wttr.in 真实天气）
- NPC（路人点 + 重大事件聚焦 + 角色头像）
- 设施与交通（生活设施 + 9 种交通工具）
- 最后接入自主系统与心跳

## 一个建议：地图作为独立按需加载模块

LingChat 的客户端插件体系本身就是**按需加载**的思路。

地图同理：**用户大部分时间不看地图**，没必要让它常驻首屏。
把它做成独立模块（或客户端插件形态）加载，可以：

- 避免地图逻辑增加主对话页的解析与渲染成本；
- 与 LingChat 既有的插件体系一致；
- 手机上明显更流畅（避免主对话页首屏解析过重）。
