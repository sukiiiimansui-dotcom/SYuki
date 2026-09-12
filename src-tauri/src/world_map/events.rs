//! 世界模拟 · 现实事件引擎（P5-1，**纯函数层**）
//!
//! 让世界自己「发生事情」：十类现实事件（天气 / 交通 / 社交 / 工作学习 / 健康 /
//! 消费 / 意外 / 节日 / 情绪 / 小确幸小倒霉），加权随机 + 条件修正 + 冷却 + 全局节流，
//! 产出四种文案形态（应用内弹窗 / 地图气泡 / 对话注入提示 / 角色记忆一行）。
//!
//! ## 为什么是一个「不依赖任何东西」的纯函数文件
//!
//! 1. **时间从参数进来**（`now_secs`），**随机数从参数进来**（`roll` + `rng` 闭包）。
//!    本文件不取系统时间、不引 `rand`、不碰文件、不碰网络，也就没有任何
//!    「跑一次才知道」的行为 —— 单测可以把 roll 钉在区间边界上、把小时钉在 23 点，
//!    逐条验证权重规则。抽签式代码最容易出的错就是"看起来对"，这里用可注入的
//!    roll/rng 把它变成可断言的东西。
//! 2. **不 import `tauri`、不 import `crate::world_map::*`**：
//!    本文件与 `state.rs`（另一个代理在改）零耦合，接线由 `mod.rs` / `state.rs` 做。
//!    副作用是本文件能**脱离整个工程单独 `rustc --test` 跑单测** —— 本机禁止
//!    `cargo build/test`（Tauri 全量编译 40 分钟起），这是唯一能真跑起来的路子。
//! 3. 行政区名、天气、室内外、心情体力这些**都由调用方塞进 [`EventContext`]**，
//!    本文件不认识地图，也不去查任何东西。
//!
//! ## 触发流程（`plan_event` 一步到位）
//!
//! ```text
//! ① 全局节流：距上一次「任何」事件 < MIN_GAP_SECS（300s）→ 直接 None（连随机数都不摇）
//! ② 逐条算权重 weight_for()：
//!      闸门不通过（时段/场所/室内外/天气/节日）→ 0
//!      冷却中（同 id，距上次 < cooldown_secs）→ 0
//!      否则 = base_weight × 一串修正因子（见文件下半部分的常量表）
//! ③ 候选里权重全部 ≤ 0（或没有候选）→ None
//! ④ 否则按 roll × 总权重 落在哪个区间就选哪条（区间左闭右开，见 plan_event 注释）
//! ```
//!
//! ### 三个刻意的设计取舍
//!
//! * **冷却/闸门是把该条权重置 0，不是整体跳过**。机主明确要求：某个事件在冷却里，
//!   剩下的候选仍按各自权重正常抽。若改成"跳过整轮"，就会出现"最近下过雨所以这半小时
//!   世界什么都不发生"这种怪事 —— 事件引擎的稀缺性应该由**全局节流**统一控制。
//! * **天气类事件严格按真实天气开闸**（`weather.rain` 只在 `ctx.weather` 分类为雨时可触发）。
//!   注入文本里有「时间/天气：19:24 · 小雨 22°C」这一行，如果事件引擎在晴天编一场雨，
//!   模型会收到自相矛盾的上下文。**宁可不发生，也不跟真实天气打架**。
//!   代价：天气接口挂掉（`ctx.weather` 为空）时天气类事件一次都不会触发。
//! * **`indoors` 未知时按"户外"处理**。`EventContext::default().indoors == false`，
//!   于是"仅室内"事件（停电/失眠/睡过头）在没有上下文时不触发，"仅户外"事件照常。
//!   理由是户外类文案更中性（"路上堵成一片"发生在哪儿都不算错）。
//!
//! ## 四种文案形态
//!
//! | 函数 | 去处 | 形态 |
//! |---|---|---|
//! | [`popup_text`] | 应用内 toast | `【标题】一句话`（打断性强，默认建议关） |
//! | [`bubble_text`] | 地图气泡 | 截到 ≤ 20 字 |
//! | [`speech_hint`] | 塞进对话上下文 | 「刚才发生了：…（你可以自然地提一句，不要生硬复述）」 |
//! | [`memory_line`] | 角色记忆 | `旁白: …`（与对话记忆的 `名称: 内容` 同形，≤ 40 字） |
//!
//! `memory_line` 用 `旁白` 是跟着工程的既有约定走的：`memory_builder.rs` 的
//! `format_context_line` 把 `旁白`/`系统` 的行**只取 content**、其余行渲染成
//! `名字: 内容`；`game_status.rs` / `api/chat.rs` 也都在用 `display_name = "旁白"`。
//! 所以这一行的形状对记忆管线是"认识"的。
//!
//! ## 接线（**本文件不做**，由 `mod.rs` / `state.rs` 负责）
//!
//! ```text
//! // ① mod.rs 加一行：pub mod events;
//! // ② 触发时机由 state.rs（或一个定时任务）决定，示意：
//! let ctx = events::EventContext {
//!     role: "小满".into(),
//!     area: scene_area.clone(),
//!     place: actor_place.clone(),
//!     place_kind: actor_kind.clone(),      // facilities 的 type，如 "commercial"
//!     indoors: front_ends_says_indoor,
//!     hour: local.hour() as u8,
//!     weather: weather_desc.clone(),       // world_map_weather 的 desc
//!     temp_c: weather_temp,
//!     mood: None, energy: None,
//!     player_distance_m: None,
//!     festival: false, festival_name: None,
//!     recent: runtime.events.iter().map(RecentEvent::from_json).collect(),
//! };
//! if events::global_throttled(&ctx, now) { return; }        // 省一次摇号
//! let roll: f64 = rand::random();                            // rand 已在 Cargo.toml
//! if let Some(ev) = events::plan_event(events::table(), &ctx, now, roll,
//!                                      &mut || rand::random::<f64>()) {
//!     runtime.push_event(serde_json::to_value(&ev)?);        // summary::last_event 认 text/title
//!     // ev → popup_text / bubble_text / speech_hint / memory_line 四条通道
//! }
//! ```
//!
//! ## 已知取舍（如实记录）
//!
//! * `luck.puddle`（踩到水坑）只在真实天气是雨时可触发，而"雨天降低户外运气项"
//!   这条规则又会把它 ×0.35 —— 两条规则方向相反。按机主要求保留规则本身，
//!   水坑事件因此只在雨天且其他候选都不可用时才会出现（权重仍 > 0，不是被禁掉）。
//! * 权重数值是**拍出来的手感值**（不是拟合出来的）：只有量级有意义
//!   （节日 ×5、深夜社交 ×0.25 这类），别把它们当标定参数。
//! * 「节日」判定完全由调用方给（`festival` / `festival_name`）。本文件不带农历表，
//!   也不认识任何具体节日 —— 那是数据问题，不该焊死在事件引擎里。

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

// ═══════════════════════════════════════════════════════════════════
//  全局参数
// ═══════════════════════════════════════════════════════════════════

/// 全局节流：距上一次**任何**事件不足这个秒数，整轮直接不出事。
///
/// 300 秒 = 5 分钟。事件引擎的价值在于"偶尔来一下"，密度上去了就变成噪音
/// （尤其 `speech_hint` 会进对话上下文，每轮都塞一句"刚才发生了…"会让角色
/// 满嘴都是意外）。这个闸门是**唯一的**全局密度控制点。
pub const MIN_GAP_SECS: i64 = 300;

/// 同类别「连着来」的惩罚窗口（秒）。只看这个窗口内的连续同类事件。
pub const SAME_CATEGORY_WINDOW_SECS: i64 = 1800;

/// 连着来一次的权重倍率（本类上一条事件还在窗口内 → ×0.3）。
/// 连着两次 → ×0.3² = 0.09。这样"一连三次都堵车"的概率被压到可以忽略。
pub const SAME_CATEGORY_PENALTY: f64 = 0.3;

/// 气泡文案上限（**字数**，不是字节数 —— 中文一字三字节，按字节截会拦腰砍）。
pub const BUBBLE_MAX_CHARS: usize = 20;

/// 记忆行上限（字数）。记忆是"一行"，长了会挤占角色上下文。
pub const MEMORY_MAX_CHARS: usize = 40;

/// 深夜判定（含两端）：23 点～次日 5 点。
pub const NIGHT_FROM_HOUR: u8 = 23;
pub const NIGHT_TO_HOUR: u8 = 5;

// ── 权重修正因子（全是"倍率"，1.0 = 不修正） ──────────────────────

/// 深夜：社交 ×0.25（这个点没人约饭）
pub const SOCIAL_NIGHT_FACTOR: f64 = 0.25;
/// 深夜：工作学习 ×0.2（都睡了）
pub const WORK_NIGHT_FACTOR: f64 = 0.2;
/// 深夜：健康 ×2.0（失眠 / 感冒最容易在这个点被想起）
pub const HEALTH_NIGHT_FACTOR: f64 = 2.0;

/// 雨雪天：交通 ×2.2（堵车 / 延误）
pub const TRAFFIC_RAIN_FACTOR: f64 = 2.2;
/// 雨雪天：**户外**运气项 ×0.35（机主要求：雨天降低 luck 里的户外项）
pub const OUTDOOR_LUCK_RAIN_FACTOR: f64 = 0.35;

/// 在餐厅/商场/超市这类消费场所：消费类 ×1.8
pub const MONEY_PLACE_FACTOR: f64 = 1.8;
/// 在公园/广场/步道这类户外场所：天气类 ×1.4
pub const OUTDOOR_PLACE_WEATHER_FACTOR: f64 = 1.4;
/// 在户外场所：运气类 ×1.5（户外的偶发事件本来就多）
pub const OUTDOOR_PLACE_LUCK_FACTOR: f64 = 1.5;

/// 心情低于阈值算"低落"
pub const LOW_MOOD_THRESHOLD: f64 = 0.35;
/// 情绪低落：情绪类 ×2.2
pub const LOW_MOOD_FACTOR: f64 = 2.2;

/// 体力低于阈值算"累"
pub const LOW_ENERGY_THRESHOLD: f64 = 0.3;
/// 体力低：健康类 ×1.6（累 → 容易生病）
pub const LOW_ENERGY_HEALTH_FACTOR: f64 = 1.6;
/// 体力低：工作学习 ×0.6（累 → 干不动）
pub const LOW_ENERGY_WORK_FACTOR: f64 = 0.6;

/// 节日当天：节日类 ×5.0（机主要求"大幅提高"）
pub const FESTIVAL_FACTOR: f64 = 5.0;
/// 节日当天：社交类 ×1.2（顺带的，节日里人也更容易凑到一起）
pub const FESTIVAL_SOCIAL_FACTOR: f64 = 1.2;

/// 玩家在同一地点附近（≤ 这个米数）：社交 ×1.25
pub const PLAYER_NEAR_M: f64 = 50.0;
pub const SOCIAL_NEAR_PLAYER_FACTOR: f64 = 1.25;
/// 玩家离得很远（≥ 这个米数）：社交 ×0.7
pub const PLAYER_FAR_M: f64 = 5000.0;
pub const SOCIAL_FAR_PLAYER_FACTOR: f64 = 0.7;

/// 「消费场所」关键词（在 `place` / `place_kind` / `area` 里命中即算）。
/// 只放两字以上的词，避免单字误伤。
const MONEY_PLACES: &[&str] = &[
    "餐厅", "饭馆", "饭店", "食堂", "商场", "超市", "便利店", "咖啡", "奶茶", "书店", "店",
];

/// 「户外场所」关键词。**故意不放单字**：单字「山」会命中「东山口」这种小区名，
/// 于是"在公园"的加成会莫名其妙地落到一堆无关地点上。
const OUTDOOR_PLACES: &[&str] = &[
    "公园", "广场", "海边", "海滩", "登山", "山上", "湖边", "江边", "河堤", "步道", "绿道",
    "街头", "路边", "球场", "操场", "户外", "景点", "露营", "天台",
];

/// `{someone}` 占位符的候选词（随机挑一个）
const SOMEONE_WORDS: &[&str] = &[
    "朋友", "同事", "邻居", "老同学", "店里的人", "路过的人", "很久没联系的人",
];

// ═══════════════════════════════════════════════════════════════════
//  类别
// ═══════════════════════════════════════════════════════════════════

/// 十大类别。`key()` 是稳定英文 key（事件 id 的前缀），`zh()` 是给人看的中文名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// 天气：下雨 / 下雪 / 大雾 / 高温 / 降温
    Weather,
    /// 交通：堵车 / 末班车 / 事故封路 / 地铁延误
    Traffic,
    /// 社交：朋友约饭 / 被搭话 / 偶遇熟人 / 被放鸽子
    Social,
    /// 工作学习：加班 / 临时会议 / 考试 / 作业截止
    Work,
    /// 健康：感冒 / 失眠 / 身体不适 / 睡过头
    Health,
    /// 消费：打折 / 丢东西 / 捡到东西 / 账单
    Money,
    /// 意外：停电 / 停水 / 手机没电 / 钥匙忘带
    Accident,
    /// 节日：节日氛围 / 活动 / 烟花
    Festival,
    /// 情绪：心情好 / 低落 / 莫名烦躁
    Mood,
    /// 小确幸小倒霉：抽到想要的 / 踩到水坑 / 排到最后一个
    Luck,
}

impl Category {
    /// 全部类别（测试与「十类是否都有事件」的检查都靠它，顺序固定）
    pub const ALL: [Category; 10] = [
        Category::Weather,
        Category::Traffic,
        Category::Social,
        Category::Work,
        Category::Health,
        Category::Money,
        Category::Accident,
        Category::Festival,
        Category::Mood,
        Category::Luck,
    ];

    /// 稳定英文 key（= 事件 id 的 `.` 前缀）
    pub fn key(self) -> &'static str {
        match self {
            Category::Weather => "weather",
            Category::Traffic => "traffic",
            Category::Social => "social",
            Category::Work => "work",
            Category::Health => "health",
            Category::Money => "money",
            Category::Accident => "accident",
            Category::Festival => "festival",
            Category::Mood => "mood",
            Category::Luck => "luck",
        }
    }

    /// 中文名（界面/日志用）
    pub fn zh(self) -> &'static str {
        match self {
            Category::Weather => "天气",
            Category::Traffic => "交通",
            Category::Social => "社交",
            Category::Work => "工作学习",
            Category::Health => "健康",
            Category::Money => "消费",
            Category::Accident => "意外",
            Category::Festival => "节日",
            Category::Mood => "情绪",
            Category::Luck => "小确幸",
        }
    }

    /// 从 key 反解（前端/存档回读用）
    pub fn from_key(k: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.key() == k.trim())
    }

    /// 从事件 id 反解：`"weather.rain"` → `Weather`。
    /// 事件 id 的前缀**就是**类别 key，这条约定让"只有 id 没有类别"的旧事件
    /// 也能参与"同类别连续触发"的惩罚统计（`RecentEvent::category` 用得上）。
    pub fn from_id(id: &str) -> Option<Category> {
        Category::from_key(id.split('.').next().unwrap_or(""))
    }
}

// ═══════════════════════════════════════════════════════════════════
//  天气分类
// ═══════════════════════════════════════════════════════════════════

/// 天气归类（给闸门与修正规则用）。
///
/// 分类口径：**先看描述里的关键词，没有关键词再看温度**。
/// 关键词里 `雪` 优先于 `雨`（"雨夹雪"算雪），因为文案上"飘雪"比"下雨"更贴。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeatherKind {
    /// 晴
    Clear,
    /// 多云 / 阴
    Cloudy,
    /// 雨（含阵雨 / 雷雨 / 大雨）
    Rain,
    /// 雪（含雨夹雪 / 小雪）
    Snow,
    /// 雾 / 霾
    Fog,
    /// 热（描述里有"热/高温"，或气温 ≥ HOT_TEMP_C）
    Hot,
    /// 冷（描述里有"冷/寒/降温"，或气温 ≤ COLD_TEMP_C）
    Cold,
    /// 认不出来（描述为空且温度也没有 / 温度在常温区间）
    Other,
}

/// 气温 ≥ 这个值算 [`WeatherKind::Hot`]
pub const HOT_TEMP_C: f64 = 33.0;
/// 气温 ≤ 这个值算 [`WeatherKind::Cold`]
pub const COLD_TEMP_C: f64 = 8.0;

impl WeatherKind {
    /// 由天气描述 + 气温分类。`desc` 可以是 `world_map_weather` 的 `desc`
    /// （"小雨" / "多云" / "晴"），空串表示未知。
    pub fn of(desc: &str, temp_c: Option<f64>) -> WeatherKind {
        let d = desc.trim();
        if d.contains('雪') {
            return WeatherKind::Snow;
        }
        if d.contains('雨') {
            return WeatherKind::Rain;
        }
        if d.contains('雾') || d.contains('霾') || d.contains("沙尘") {
            return WeatherKind::Fog;
        }
        if d.contains("雷") || d.contains("大风") || d.contains("台风") {
            // 雷暴/大风：归到雨那一档（交通影响与"下雨天"同一类修正）
            return WeatherKind::Rain;
        }
        if d.contains('晴') {
            return WeatherKind::Clear;
        }
        if d.contains('云') || d.contains('阴') {
            return WeatherKind::Cloudy;
        }
        if d.contains('热') || d.contains("高温") {
            return WeatherKind::Hot;
        }
        if d.contains('冷') || d.contains('寒') || d.contains("降温") || d.contains('凉') {
            return WeatherKind::Cold;
        }
        match temp_c {
            Some(t) if t >= HOT_TEMP_C => WeatherKind::Hot,
            Some(t) if t <= COLD_TEMP_C => WeatherKind::Cold,
            _ => WeatherKind::Other,
        }
    }

    /// 这个天气算不算"雨雪天"（交通加成 / 户外运气降权都用它）
    pub fn is_wet(self) -> bool {
        matches!(self, WeatherKind::Rain | WeatherKind::Snow)
    }
}

/// 天气描述的短形态：`"中雨转小雨"` → `"中雨"`（文案里 `{weather}` 用）。
///
/// 预报文案常带"转"（"小雨转多云"），塞进"下起了{weather}"会读成
/// "下起了小雨转多云"。只取转折前那一段。
pub fn weather_short(desc: &str) -> String {
    let head = desc.split('转').next().unwrap_or("").trim();
    if head.is_empty() {
        desc.trim().to_string()
    } else {
        head.to_string()
    }
}

// ═══════════════════════════════════════════════════════════════════
//  事件定义与事件表
// ═══════════════════════════════════════════════════════════════════

/// 一条事件的定义（编译期常量，全 `'static`，`Copy`）。
///
/// 字段分三组：
///   · 身份/文案：`id` `category` `title` `text`（`text` 里可带占位符）
///   · 抽签参数：`weight` `cooldown_secs`
///   · **可选闸门**（不通过 → 该条权重置 0）：`hours` `places` `outdoor`
///     `weather` `not_weather` `festival_only` `min_temp_c` / `max_temp_c`
///
/// 闸门之间是**与**关系：全部满足才可选。任何闸门留空表示"不限制"。
///
/// 只 `Serialize` 不 `Deserialize`：字段全是 `&'static`，本来就没法从 JSON 反序列化
/// （serde 需要 `'de: 'static`）。表是编译期常量，方向也只需要"读出来给前端/日志看"。
#[derive(Debug, Clone, Copy, Serialize)]
pub struct EventDef {
    /// 稳定英文 key，形如 `weather.rain`（前缀 = 类别 key，`.` 后是事件名）
    pub id: &'static str,
    /// 所属类别
    pub category: Category,
    /// 短标题（弹窗用，2～6 字）
    pub title: &'static str,
    /// 一句话描述（中文，可带 `{role}` / `{place}` / `{area}` / `{weather}` /
    /// `{someone}` / `{festival}` 占位符；**只允许这些**，见 `fill_placeholders`）
    pub text: &'static str,
    /// 基础权重（> 0；只表示相对稀有度，绝对值无意义）
    pub weight: f64,
    /// 同一条事件的最小间隔（秒，> 0）
    pub cooldown_secs: i64,
    /// 时段闸门：`(from, to)` 闭区间，支持跨零点（如 `(23, 5)`）；`(0, 23)` = 全天
    pub hours: (u8, u8),
    /// 场所闸门：`place` / `place_kind` / `area` 命中其中之一即可；空 = 不限
    pub places: &'static [&'static str],
    /// 室内外闸门：`Some(true)` 仅户外、`Some(false)` 仅室内、`None` 不限
    pub outdoor: Option<bool>,
    /// 天气闸门：当前天气分类命中其中之一才可选；空 = 不限
    pub weather: &'static [WeatherKind],
    /// 天气排除：当前天气分类命中其中之一就不可选
    pub not_weather: &'static [WeatherKind],
    /// 只在节日当天可选
    pub festival_only: bool,
    /// 气温下限（≥ 才可选）
    pub min_temp_c: Option<f64>,
    /// 气温上限（≤ 才可选）
    pub max_temp_c: Option<f64>,
}

impl EventDef {
    /// 全部闸门留空的模板（`ev!` 宏就是拿它做结构体更新）
    pub const DEFAULT: EventDef = EventDef {
        id: "",
        category: Category::Mood,
        title: "",
        text: "",
        weight: 1.0,
        cooldown_secs: 60,
        hours: (0, 23),
        places: &[],
        outdoor: None,
        weather: &[],
        not_weather: &[],
        festival_only: false,
        min_temp_c: None,
        max_temp_c: None,
    };

    /// 这条事件的 id 前缀推出来的类别（与 `category` 字段应当时刻一致，
    /// `EventTable::validate` 会检查）
    pub fn category_from_id(&self) -> Option<Category> {
        Category::from_id(self.id)
    }
}

/// 建表宏：`ev!("id", 类别, "标题", "文案", 权重, 冷却)`，
/// 后面可以按 `字段: 值` 追加任意闸门，其余字段取 [`EventDef::DEFAULT`]。
///
/// 用宏而不是手写 41 条完整结构体字面量：每条只有 2～3 个字段与模板不同，
/// 写全了满屏都是 `outdoor: None, places: &[]` —— 真正要看的差异反而被淹没。
macro_rules! ev {
    ($id:literal, $cat:expr, $title:literal, $text:literal, $w:expr, $cd:expr $(, $k:ident : $v:expr)*) => {
        EventDef { id: $id, category: $cat, title: $title, text: $text, weight: $w, cooldown_secs: $cd $(, $k: $v)*, ..EventDef::DEFAULT }
    };
}

/// 十类现实事件总表（41 条：机主逐条确认的 38 条 + 3 条同类补充 ——
/// `festival.crowd` / `mood.calm` / `luck.small_gift`）。
///
/// 权重手感：普通日常 5～7，罕见 3～5，节日当天专属 8～12（因为它平时被闸门关着）。
/// 冷却手感：一时半会儿不会重复的（堵车 45min）短，一天最多一次的（考试 24h）长。
pub static EVENTS: &[EventDef] = &[
    // ── 天气（5）：全部按**真实天气**开闸，晴天不会编出下雨 ──
    ev!("weather.rain", Category::Weather, "下雨", "{place}下起了{weather}，路面很快就积起了水", 8.0, 3600, weather: &[WeatherKind::Rain]),
    ev!("weather.snow", Category::Weather, "下雪", "{place}飘起了雪，路上安静得出奇", 7.0, 7200, weather: &[WeatherKind::Snow]),
    ev!("weather.fog", Category::Weather, "大雾", "{place}起了大雾，远处的楼都看不清了", 6.0, 7200, weather: &[WeatherKind::Fog]),
    ev!("weather.heat", Category::Weather, "高温", "天气热得反常，{place}吹过来的风都是烫的", 6.0, 10800, min_temp_c: Some(HOT_TEMP_C)),
    ev!("weather.cool", Category::Weather, "降温", "突然降温了，{role}在{place}缩了缩脖子", 6.0, 10800, max_temp_c: Some(COLD_TEMP_C)),

    // ── 交通（4）──
    ev!("traffic.jam", Category::Traffic, "堵车", "{place}前面的路堵成一片，车挪得比走路还慢", 7.0, 2700, hours: (6, 22), outdoor: Some(true)),
    ev!("traffic.last_bus", Category::Traffic, "末班车", "差点没赶上末班车，{place}的站台上只剩最后几个人", 5.0, 7200, hours: (21, 1), outdoor: Some(true)),
    ev!("traffic.accident", Category::Traffic, "事故封路", "前面出了事故，{place}附近的路被封了一半", 4.0, 5400, outdoor: Some(true)),
    ev!("traffic.subway_delay", Category::Traffic, "地铁延误", "地铁临时延误，{place}的站厅里挤满了人", 5.0, 3600, hours: (6, 23)),

    // ── 社交（4）──
    ev!("social.invite", Category::Social, "朋友约饭", "{someone}发消息约{role}吃饭，说就在{place}附近", 7.0, 3600, hours: (10, 22)),
    ev!("social.stranger", Category::Social, "被搭话", "在{place}被陌生人搭话，对方问路问得很认真", 6.0, 2700),
    ev!("social.acquaintance", Category::Social, "偶遇熟人", "在{place}撞见了好久不见的熟人，两个人都愣了一下", 5.0, 5400),
    ev!("social.stand_up", Category::Social, "被放鸽子", "{someone}临时说来不了，约在{place}的见面只能取消", 4.0, 7200),

    // ── 工作学习（4）──
    ev!("work.overtime", Category::Work, "加班", "临时被留下加班，{place}的灯一直亮到很晚", 6.0, 5400, hours: (17, 23)),
    ev!("work.meeting", Category::Work, "临时会议", "突然被拉进一个临时会议，{role}的下午被打断了", 6.0, 3600, hours: (9, 18), outdoor: Some(false)),
    // 截止时间是会拖到凌晨的，所以这条**不设时段闸门** —— 深夜的"工作 ×0.2"修正
    // 正好也是靠它才测得到（其余工作类事件都在白天窗口里）
    ev!("work.deadline", Category::Work, "作业截止", "作业快到截止时间了，{role}还在{place}赶进度", 5.0, 10800),
    ev!("work.exam", Category::Work, "考试", "明天有考试，{role}在{place}翻着书，有点心不在焉", 4.0, 86400, hours: (8, 23)),

    // ── 健康（4）──
    ev!("health.cold", Category::Health, "感冒", "{role}有点感冒，鼻子一直不通气", 5.0, 14400),
    ev!("health.insomnia", Category::Health, "失眠", "躺下很久也没睡着，{role}听着{place}外面的动静", 6.0, 14400, hours: (23, 5), outdoor: Some(false)),
    ev!("health.unwell", Category::Health, "身体不适", "{role}忽然觉得有点不舒服，只好先坐下来歇一会儿", 4.0, 7200),
    ev!("health.oversleep", Category::Health, "睡过头", "闹钟没响，{role}睡过头了，早上慌成一团", 5.0, 43200, hours: (6, 10), outdoor: Some(false)),

    // ── 消费（4）：打折只在消费场所出现 ──
    ev!("money.sale", Category::Money, "打折", "{place}正在打折，{role}没忍住多看了两眼", 6.0, 3600, places: MONEY_PLACES),
    ev!("money.lost", Category::Money, "丢了东西", "{role}发现丢了点小东西，怎么想都想不起落在哪儿了", 4.0, 10800),
    ev!("money.found", Category::Money, "捡到东西", "{role}在{place}捡到一枚硬币，顺手放进了口袋", 4.0, 10800),
    ev!("money.bill", Category::Money, "账单", "这个月的账单出来了，{role}盯着数字沉默了几秒", 5.0, 21600),

    // ── 意外（4）：停电/停水发生在"家里"（室内），这个前提不写清楚会很假 ──
    ev!("accident.power_out", Category::Accident, "停电", "{place}这一片突然停电了，屋里一下子安静下来", 3.0, 14400, outdoor: Some(false)),
    ev!("accident.water_out", Category::Accident, "停水", "停水了，{role}只好先把手里的活儿放下", 3.0, 14400, outdoor: Some(false)),
    ev!("accident.phone_dead", Category::Accident, "手机没电", "手机只剩最后一格电，{role}开始省着用了", 4.0, 7200),
    ev!("accident.locked_out", Category::Accident, "钥匙忘带", "{role}摸口袋才发现钥匙忘带了，只能站在{place}门口等", 3.0, 14400, outdoor: Some(true)),

    // ── 节日（4）：全部 festival_only，平时被闸门关着（这正是"大幅提高"的前提） ──
    ev!("festival.decor", Category::Festival, "节日氛围", "今天是{festival}，{area}到处都挂上了装饰", 12.0, 21600, festival_only: true),
    ev!("festival.activity", Category::Festival, "节日活动", "{area}有{festival}的活动，{place}附近比平时热闹很多", 10.0, 21600, festival_only: true),
    ev!("festival.fireworks", Category::Festival, "烟花", "晚上有烟花，{place}抬头就能看见", 8.0, 21600, hours: (18, 23), festival_only: true),
    ev!("festival.crowd", Category::Festival, "人挤人", "节日的{place}人挤人，{role}走两步就要停一下", 8.0, 21600, festival_only: true),

    // ── 情绪（4）──
    ev!("mood.good", Category::Mood, "心情不错", "{role}今天心情莫名地好，看什么都顺眼", 5.0, 7200),
    ev!("mood.low", Category::Mood, "有点低落", "{role}忽然有点低落，说不上来为什么", 5.0, 7200),
    ev!("mood.irritable", Category::Mood, "莫名烦躁", "{role}莫名有点烦躁，一点小动静都嫌吵", 5.0, 7200),
    ev!("mood.calm", Category::Mood, "安静发呆", "忙完一阵，{role}在{place}安静地发了会儿呆", 4.0, 7200),

    // ── 小确幸小倒霉（4）──
    ev!("luck.win", Category::Luck, "抽到想要的", "{role}抽到了想要的那个，忍不住笑了出来", 5.0, 10800),
    ev!("luck.puddle", Category::Luck, "踩到水坑", "{role}一脚踩进水坑，鞋袜全湿了", 5.0, 5400, outdoor: Some(true), weather: &[WeatherKind::Rain]),
    ev!("luck.last_one", Category::Luck, "排到最后一个", "{role}排到队伍最后，前面还有一长串人", 5.0, 5400, outdoor: Some(true), places: MONEY_PLACES),
    ev!("luck.small_gift", Category::Luck, "小惊喜", "店老板多送了一份小菜，{role}愣了一下才道谢", 4.0, 10800, places: MONEY_PLACES),
];

/// 事件表（`OnceLock` 里构建一次，之后全是 `&'static`）。
///
/// 为什么是 `Vec<EventDef>` 而不是直接 `&'static [EventDef]`：测试与将来的
/// "按存档覆盖事件表"都需要能自造一张表（[`EventTable::new`]）。
/// 生产路径用 [`EventTable::global`]（或简写 [`table`]），拿到的是一份只读引用。
#[derive(Debug, Clone)]
pub struct EventTable {
    events: Vec<EventDef>,
}

impl EventTable {
    /// 自造一张表（测试 / 将来的存档级覆盖用）
    pub fn new(events: Vec<EventDef>) -> EventTable {
        EventTable { events }
    }

    /// 内置总表（进程内只构建一次）
    pub fn global() -> &'static EventTable {
        static GLOBAL: OnceLock<EventTable> = OnceLock::new();
        GLOBAL.get_or_init(|| EventTable::new(EVENTS.to_vec()))
    }

    /// 全部事件（表序 = 抽签顺序，同一 roll 的结果只跟表序有关，稳定）
    pub fn events(&self) -> &[EventDef] {
        &self.events
    }

    /// 条数
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 空表
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 按 id 取（线性扫；41 条量级不值得再引一个 HashMap）
    pub fn get(&self, id: &str) -> Option<&EventDef> {
        self.events.iter().find(|e| e.id == id)
    }

    /// 按类别取
    pub fn by_category(&self, cat: Category) -> Vec<&EventDef> {
        self.events.iter().filter(|e| e.category == cat).collect()
    }

    /// 表里实际出现过的类别（顺序按 [`Category::ALL`]）
    pub fn categories(&self) -> Vec<Category> {
        Category::ALL
            .into_iter()
            .filter(|c| self.events.iter().any(|e| e.category == *c))
            .collect()
    }

    /// 自检：返回所有问题（空 = 表是健康的）。
    ///
    /// 检查项：id 非空且带类别前缀、id 唯一、category 与 id 前缀一致、
    /// weight/cooldown 为正、标题与文案非空、占位符都在白名单里、时段合法。
    /// 给调试命令与单测共用 —— 表是**手写**的，手写就会写错字。
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for (i, e) in self.events.iter().enumerate() {
            if e.id.trim().is_empty() {
                problems.push(format!("#{i} 的 id 为空"));
            }
            if self.events.iter().filter(|o| o.id == e.id).count() > 1
                && self.events.iter().position(|o| o.id == e.id) == Some(i)
            {
                problems.push(format!("id 重复：{}", e.id));
            }
            match e.category_from_id() {
                Some(c) if c != e.category => problems.push(format!(
                    "{} 的类别前后不一致：字段是 {}，id 前缀推出 {}",
                    e.id,
                    e.category.key(),
                    c.key()
                )),
                None => problems.push(format!("{} 的 id 前缀不是已知类别", e.id)),
                _ => {}
            }
            if !(e.weight > 0.0) || !e.weight.is_finite() {
                problems.push(format!("{} 的 weight 不是正数：{}", e.id, e.weight));
            }
            if e.cooldown_secs <= 0 {
                problems.push(format!("{} 的 cooldown_secs 不是正数：{}", e.id, e.cooldown_secs));
            }
            if e.title.trim().is_empty() || e.text.trim().is_empty() {
                problems.push(format!("{} 的标题或文案为空", e.id));
            }
            if e.hours.0 > 23 || e.hours.1 > 23 {
                problems.push(format!("{} 的时段越界：{:?}", e.id, e.hours));
            }
            for ph in placeholders(e.text) {
                if !KNOWN_PLACEHOLDERS.contains(&ph.as_str()) {
                    problems.push(format!("{} 用了不认识的占位符 {{{}}}", e.id, ph));
                }
            }
        }
        problems
    }
}

/// 内置总表（`EventTable::global()` 的简写，调用点更短）
pub fn table() -> &'static EventTable {
    EventTable::global()
}

/// 允许出现在 `text` 里的占位符白名单。
///
/// `validate` 会拦下白名单外的写法：写错一个 `{palce}` 在运行时只会静默变成空串，
/// 表是手写的，这种错必须在单测里就炸出来。
pub const KNOWN_PLACEHOLDERS: &[&str] = &["role", "place", "area", "weather", "someone", "festival"];

// ═══════════════════════════════════════════════════════════════════
//  上下文
// ═══════════════════════════════════════════════════════════════════

/// 最近发生过的一条事件（冷却与"同类别连续触发"惩罚都看它）。
///
/// 形状特意做得宽容：`category` 可以缺（老事件可能只有 id），
/// 缺了就从 id 前缀推（见 [`RecentEvent::category`]）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecentEvent {
    /// 事件 id（与 [`EventDef::id`] 同一套 key）
    pub id: String,
    /// 类别（可缺，缺了从 id 推）
    pub category: Option<Category>,
    /// 发生时刻（unix 秒）
    pub at_secs: i64,
}

impl RecentEvent {
    /// 显式构造
    pub fn new(id: impl Into<String>, category: Option<Category>, at_secs: i64) -> RecentEvent {
        RecentEvent {
            id: id.into(),
            category,
            at_secs,
        }
    }

    /// 只要 id 与时刻（类别交给 id 推断）
    pub fn at(id: impl Into<String>, at_secs: i64) -> RecentEvent {
        RecentEvent {
            id: id.into(),
            category: None,
            at_secs,
        }
    }

    /// 类别：字段优先，其次从 id 前缀推
    pub fn category(&self) -> Option<Category> {
        self.category.or_else(|| Category::from_id(&self.id))
    }
}

/// 触发一次事件时世界的样子。
///
/// 全部字段可缺省（`#[serde(default)]`）：前端/`state.rs` 一次只知道一部分信息时，
/// 推上来的残缺对象也能直接反序列化，缺的部分按 [`EventContext::default`] 处理。
/// **本文件不猜任何东西**：`weather` 为空就是"天气未知"，
/// `mood`/`energy`/`player_distance_m` 为 `None` 就是"不知道"（不参与修正）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EventContext {
    /// 角色名（`{role}` 占位符；空则渲染成"你"）
    pub role: String,
    /// 行政区文案，如 `"广州市·越秀区·东山口"`（`{area}` 占位符）
    pub area: String,
    /// 具体地点/设施名，如 `"便利店"`（`{place}` 占位符）
    pub place: String,
    /// 设施类型 key（`facilities.rs` 的 `type`，如 `commercial` / `park`）——
    /// 只参与"场所"判定，不参与文案
    pub place_kind: String,
    /// 是否在室内。**未知时是 `false`（按户外处理）**，见文件头取舍说明
    pub indoors: bool,
    /// 当地小时（0–23）。越界值会被 [`EventContext::hour_of`] 取模救回来
    pub hour: u8,
    /// 当前天气描述（`world_map_weather` 的 `desc`，如 `"小雨"`）；空 = 未知
    pub weather: String,
    /// 当前气温（摄氏度）；`None` = 未知
    pub temp_c: Option<f64>,
    /// 角色心情 0.0（很低落）～1.0（很好）；`None` = 未知
    pub mood: Option<f64>,
    /// 角色体力 0.0（很累）～1.0（充沛）；`None` = 未知
    pub energy: Option<f64>,
    /// 玩家与角色的距离（米）；`None` = 未知
    pub player_distance_m: Option<f64>,
    /// 今天是否节日
    pub festival: bool,
    /// 节日名（`{festival}` 占位符）；给了它也算"是节日"
    pub festival_name: Option<String>,
    /// 最近已发生的事件（含时间戳），冷却与同类惩罚都看它
    pub recent: Vec<RecentEvent>,
}

impl EventContext {
    /// 归一化后的小时（`hour = 25` → `1`）。设备时间/前端计算偶尔会给出怪值，
    /// 与其让闸门算错，不如取模。
    pub fn hour_of(&self) -> u8 {
        self.hour % 24
    }

    /// 是否深夜（23 点～次日 5 点）
    pub fn is_deep_night(&self) -> bool {
        let h = self.hour_of();
        h >= NIGHT_FROM_HOUR || h <= NIGHT_TO_HOUR
    }

    /// 天气分类（描述关键词优先，其次气温）
    pub fn weather_kind(&self) -> WeatherKind {
        WeatherKind::of(&self.weather, self.temp_c)
    }

    /// 今天算不算节日：`festival` 为真，或者给了非空的 `festival_name`
    pub fn is_festival(&self) -> bool {
        self.festival
            || self
                .festival_name
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
    }

    /// 场所判定的匹配文本：`place` + `place_kind` + `area` 拼在一起找关键词
    fn place_haystack(&self) -> String {
        format!("{}|{}|{}", self.place, self.place_kind, self.area)
    }

    /// 是否落在"消费场所"（餐厅/商场/超市…）
    pub fn at_money_place(&self) -> bool {
        let hay = self.place_haystack();
        MONEY_PLACES.iter().any(|k| hay.contains(k))
    }

    /// 是否落在"户外场所"（公园/广场/步道…）
    pub fn at_outdoor_place(&self) -> bool {
        let hay = self.place_haystack();
        OUTDOOR_PLACES.iter().any(|k| hay.contains(k))
    }
}

// ═══════════════════════════════════════════════════════════════════
//  产物
// ═══════════════════════════════════════════════════════════════════

/// 一次已经抽中的事件。
///
/// `text` 是**占位符已经填好**的成品文案（`plan_event` 里渲染的），
/// 所以四种通知形态都只需要这一个结构体，不必再把上下文带下去。
///
/// 注意没有闭包字段：本结构体要 `Serialize`（过 Tauri 边界、落 `events.json`），
/// 随机数只在 [`plan_event`] 内部用完即弃。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedEvent {
    /// 事件 id
    pub id: String,
    /// 类别
    pub category: Category,
    /// 短标题
    pub title: String,
    /// 成品文案（占位符已替换）
    pub text: String,
    /// **本次抽签实际用的权重**（含全部修正与惩罚）—— 调参/排障要看的数
    pub weight_used: f64,
    /// 发生时刻（= 调用方传进来的 `now_secs`）
    pub at_secs: i64,
}

// ═══════════════════════════════════════════════════════════════════
//  闸门 / 冷却 / 权重
// ═══════════════════════════════════════════════════════════════════

/// 小时是否落在闭区间 `(from, to)` 内；`from > to` 表示跨零点（如 `(23, 5)`）。
pub fn hour_in_window(hour: u8, window: (u8, u8)) -> bool {
    let h = hour % 24;
    let (from, to) = (window.0.min(23), window.1.min(23));
    if from <= to {
        h >= from && h <= to
    } else {
        h >= from || h <= to
    }
}

/// 静态闸门（与时间无关的那些）：时段 / 场所 / 室内外 / 天气 / 节日 / 气温。
/// 返回 `false` 表示这条事件此刻**根本不可选**（不同于"权重被压到 0"）。
pub fn gate_open(def: &EventDef, ctx: &EventContext) -> bool {
    // 时段
    if !hour_in_window(ctx.hour_of(), def.hours) {
        return false;
    }
    // 场所（任意命中即可）
    if !def.places.is_empty() {
        let hay = ctx.place_haystack();
        if !def.places.iter().any(|k| hay.contains(k)) {
            return false;
        }
    }
    // 室内外
    if let Some(outdoor) = def.outdoor {
        if outdoor == ctx.indoors {
            // 需要户外却在室内（或反之）
            return false;
        }
    }
    // 天气白名单 / 黑名单
    let kind = ctx.weather_kind();
    if !def.weather.is_empty() && !def.weather.contains(&kind) {
        return false;
    }
    if def.not_weather.contains(&kind) {
        return false;
    }
    // 节日专属
    if def.festival_only && !ctx.is_festival() {
        return false;
    }
    // 气温
    if let Some(min) = def.min_temp_c {
        match ctx.temp_c {
            Some(t) if t >= min => {}
            _ => return false,
        }
    }
    if let Some(max) = def.max_temp_c {
        match ctx.temp_c {
            Some(t) if t <= max => {}
            _ => return false,
        }
    }
    true
}

/// 这条事件是否还在冷却里（同 id，距上次发生不足 `cooldown_secs`）。
///
/// 时刻比 `now_secs` 还晚的（设备时间被改过 / 存档带未来时间戳）按"刚刚发生"算
/// —— `saturating_sub` 天然给出 0，冷却照样生效，不会因为时间倒流就无限刷。
pub fn cooldown_active(def: &EventDef, ctx: &EventContext, now_secs: i64) -> bool {
    ctx.recent
        .iter()
        .any(|r| r.id == def.id && now_secs.saturating_sub(r.at_secs) < def.cooldown_secs)
}

/// 最近一条事件的时刻（`ctx.recent` 里最大的 `at_secs`，只在 ≤ `now_secs` 的范围里找）。
///
/// 只看过去的：比现在还晚的时间戳（时钟回拨/脏数据）不能把引擎永久锁死。
pub fn last_event_at(ctx: &EventContext, now_secs: i64) -> Option<i64> {
    ctx.recent
        .iter()
        .filter(|r| r.at_secs <= now_secs)
        .map(|r| r.at_secs)
        .max()
}

/// 全局节流是否生效：距上一次**任何**事件不足 [`MIN_GAP_SECS`] → `true`。
///
/// `plan_event` 自己会查一遍；单独暴露是为了让调用方**先**查、省掉一次摇号
/// （也免得为了摇号去构造 RNG）。
pub fn global_throttled(ctx: &EventContext, now_secs: i64) -> bool {
    match last_event_at(ctx, now_secs) {
        Some(at) => now_secs.saturating_sub(at) < MIN_GAP_SECS,
        None => false,
    }
}

/// 「同类别连着来」的惩罚次数：从最新往回数，连续同类事件的条数
/// （只看 [`SAME_CATEGORY_WINDOW_SECS`] 窗口内、且发生在 `now_secs` 之前的）。
///
/// 中间插进一条别的类别就断链 —— 这正是"一连三次都堵车"要防的形状。
fn back_to_back_count(ctx: &EventContext, cat: Category, now_secs: i64) -> u32 {
    let mut recent: Vec<&RecentEvent> = ctx
        .recent
        .iter()
        .filter(|r| r.at_secs <= now_secs && now_secs - r.at_secs <= SAME_CATEGORY_WINDOW_SECS)
        .collect();
    recent.sort_by_key(|r| r.at_secs);
    let mut n = 0;
    for r in recent.iter().rev() {
        if r.category() == Some(cat) {
            n += 1;
        } else {
            break;
        }
    }
    n
}

/// 一条事件此刻的权重（0.0 = 不可选/在冷却里）。
///
/// 计算顺序：闸门 → 冷却 → 基础权重 → 一串**乘法**修正。
/// 因子相乘而不是相加：每条规则都是独立的"证据"（雨天 + 深夜 + 在商场），
/// 相乘才能让多条规则叠加出足够强的倾向，也不会出现"加了三次变成负数"。
///
/// 注意：本函数**不含全局节流** —— 节流是"整轮不出事"，是 [`plan_event`] 的事。
/// 调试时看单条权重不会被"5 分钟内什么都不许发生"干扰。
pub fn weight_for(def: &EventDef, ctx: &EventContext, now_secs: i64) -> f64 {
    if !gate_open(def, ctx) {
        return 0.0;
    }
    if cooldown_active(def, ctx, now_secs) {
        return 0.0;
    }
    if !(def.weight > 0.0) || !def.weight.is_finite() {
        return 0.0;
    }

    let mut w = def.weight;
    let cat = def.category;
    let kind = ctx.weather_kind();

    // ① 深夜（23–5）：社交/工作学习降，健康升
    if ctx.is_deep_night() {
        match cat {
            Category::Social => w *= SOCIAL_NIGHT_FACTOR,
            Category::Work => w *= WORK_NIGHT_FACTOR,
            Category::Health => w *= HEALTH_NIGHT_FACTOR,
            _ => {}
        }
    }

    // ② 雨雪天：交通升，**户外**运气项降
    if kind.is_wet() {
        if cat == Category::Traffic {
            w *= TRAFFIC_RAIN_FACTOR;
        }
        if cat == Category::Luck && def.outdoor == Some(true) {
            w *= OUTDOOR_LUCK_RAIN_FACTOR;
        }
    }

    // ③ 场所：消费场所拉消费，户外场所拉天气与运气
    if ctx.at_money_place() && cat == Category::Money {
        w *= MONEY_PLACE_FACTOR;
    }
    if ctx.at_outdoor_place() {
        match cat {
            Category::Weather => w *= OUTDOOR_PLACE_WEATHER_FACTOR,
            Category::Luck => w *= OUTDOOR_PLACE_LUCK_FACTOR,
            _ => {}
        }
    }

    // ④ 情绪低落：情绪类升
    if ctx.mood.is_some_and(|m| m < LOW_MOOD_THRESHOLD) && cat == Category::Mood {
        w *= LOW_MOOD_FACTOR;
    }

    // ⑤ 体力低：健康升，工作学习降
    if ctx.energy.is_some_and(|e| e < LOW_ENERGY_THRESHOLD) {
        match cat {
            Category::Health => w *= LOW_ENERGY_HEALTH_FACTOR,
            Category::Work => w *= LOW_ENERGY_WORK_FACTOR,
            _ => {}
        }
    }

    // ⑥ 节日当天：节日类大幅升，社交类小升
    if ctx.is_festival() {
        match cat {
            Category::Festival => w *= FESTIVAL_FACTOR,
            Category::Social => w *= FESTIVAL_SOCIAL_FACTOR,
            _ => {}
        }
    }

    // ⑦ 玩家距离：在身边更容易有社交，离得远就少了
    if cat == Category::Social {
        if let Some(d) = ctx.player_distance_m {
            if d <= PLAYER_NEAR_M {
                w *= SOCIAL_NEAR_PLAYER_FACTOR;
            } else if d >= PLAYER_FAR_M {
                w *= SOCIAL_FAR_PLAYER_FACTOR;
            }
        }
    }

    // ⑧ 同类别连着来：×0.3 的 n 次方（n = 连着几条）
    let n = back_to_back_count(ctx, cat, now_secs);
    if n > 0 {
        w *= SAME_CATEGORY_PENALTY.powi(n as i32);
    }

    if w.is_finite() && w > 0.0 {
        w
    } else {
        0.0
    }
}

/// 当前所有可选项及其权重（`weight > 0` 才进列表，顺序 = 表序）。
///
/// 抽签、调试面板、单测都用它；`plan_event` 也是先算它再按 roll 落点。
pub fn candidates<'a>(
    table: &'a EventTable,
    ctx: &EventContext,
    now_secs: i64,
) -> Vec<(&'a EventDef, f64)> {
    table
        .events()
        .iter()
        .filter_map(|d| {
            let w = weight_for(d, ctx, now_secs);
            if w > 0.0 {
                Some((d, w))
            } else {
                None
            }
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════
//  抽签
// ═══════════════════════════════════════════════════════════════════

/// 抽一次事件：这一步就是机主要求的那个纯函数入口。
///
/// * `roll` —— `0.0..=1.0` 的均匀随机数，**由调用方摇**（`rand::random::<f64>()`。
///   本文件不引 rand，也不取系统时间，所以单测能把 roll 钉在区间边界上）
/// * `rng` —— 需要更多随机数时用（目前只有 `{someone}` 选词）。同样由调用方注入。
///
/// 抽签口径：`target = roll × 总权重`，按表序累加，落在哪个区间就选哪条
/// （**左闭右开**：`acc_prev <= target < acc`）。`roll = 1.0` 或浮点累加误差
/// 导致没落进任何区间时，兜底取最后一条 —— 这条兜底是为了"永不 panic、永不返回
/// 越界下标"，不是为了让 1.0 有意义（真随机数取到 1.0 的概率是 0）。
///
/// `roll` 为 NaN / 负数 / > 1 都会被夹回 `[0, 1]`：调用方传什么都不会炸。
pub fn plan_event(
    table: &EventTable,
    ctx: &EventContext,
    now_secs: i64,
    roll: f64,
    rng: &mut impl FnMut() -> f64,
) -> Option<PlannedEvent> {
    // ① 全局节流：5 分钟内已经发生过任何事件 → 这一轮什么都不发生
    if global_throttled(ctx, now_secs) {
        return None;
    }
    // ② 候选（已排除闸门不通过、冷却中、权重非正的）
    let picks = candidates(table, ctx, now_secs);
    if picks.is_empty() {
        return None;
    }
    let total: f64 = picks.iter().map(|(_, w)| *w).sum();
    if !(total > 0.0) || !total.is_finite() {
        // 全部权重 ≤ 0（或出现 inf/NaN）→ 没有可发生的
        return None;
    }

    // ③ 落点
    let r = if roll.is_nan() {
        0.0
    } else {
        roll.clamp(0.0, 1.0)
    };
    let target = r * total;
    let mut acc = 0.0;
    let mut chosen = picks[picks.len() - 1]; // 浮点误差兜底：最后一条
    for &(def, w) in &picks {
        acc += w;
        if target < acc {
            chosen = (def, w);
            break;
        }
    }

    // ④ 渲染文案（占位符在这里一次性填好）
    let (def, weight_used) = chosen;
    let text = render_text(def, ctx, rng);
    Some(PlannedEvent {
        id: def.id.to_string(),
        category: def.category,
        title: def.title.to_string(),
        text,
        weight_used,
        at_secs: now_secs,
    })
}

// ═══════════════════════════════════════════════════════════════════
//  占位符
// ═══════════════════════════════════════════════════════════════════

/// 渲染一条文案：把 `text` 里的占位符全部替换掉。
///
/// 已经抽中的事件不需要它（`plan_event` 里填好了）；单独暴露是给
/// "只想要一句话"的场景（调试面板、AI 工具里试写一条事件）用。
pub fn render_text(def: &EventDef, ctx: &EventContext, rng: &mut impl FnMut() -> f64) -> String {
    fill_placeholders(def.text, ctx, rng)
}

/// 占位符替换。支持：`{role}` `{place}` `{area}` `{weather}` `{someone}` `{festival}`。
///
/// 三条保底：
///   · 认不出来的 `{xxx}` 替换成**空串**（不是原样留下）—— 宁可少几个字，
///     也不能让 `{palce}` 这种手滑写进角色嘴里；
///   · 没有闭合 `}` 的 `{` 直接丢掉，输出的字符串里**一定不含 `{`**；
///   · `{someone}` 用 `rng()` 选词（这是本文件唯一的随机来源）。
pub fn fill_placeholders(text: &str, ctx: &EventContext, rng: &mut impl FnMut() -> f64) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                out.push_str(&placeholder_value(&after[..close], ctx, rng));
                rest = &after[close + 1..];
            }
            None => {
                // 没有闭合括号：这一段全丢（保证结果里没有 '{'）
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// 单个占位符的值。
fn placeholder_value(key: &str, ctx: &EventContext, rng: &mut impl FnMut() -> f64) -> String {
    match key.trim() {
        "role" => {
            let r = ctx.role.trim();
            if r.is_empty() {
                "你".to_string()
            } else {
                r.to_string()
            }
        }
        // 地点：具体设施名优先；没有就退到行政区最后一段（"广州市·越秀区·东山口" → "东山口"）；
        // 再没有就"附近" —— 这三个兜底让所有含 {place} 的文案在任何上下文下都读得通。
        "place" => {
            let p = ctx.place.trim();
            if !p.is_empty() {
                return p.to_string();
            }
            let tail = ctx
                .area
                .split('·')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .next_back()
                .unwrap_or("");
            if tail.is_empty() {
                "附近".to_string()
            } else {
                tail.to_string()
            }
        }
        "area" => {
            let a = ctx.area.trim();
            if a.is_empty() {
                "这里".to_string()
            } else {
                a.to_string()
            }
        }
        "weather" => weather_short(&ctx.weather),
        "someone" => {
            let i = pick_index(rng(), SOMEONE_WORDS.len());
            SOMEONE_WORDS[i].to_string()
        }
        "festival" => ctx
            .festival_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("节日")
            .to_string(),
        // 不认识的占位符：空串（见 fill_placeholders 的说明）
        _ => String::new(),
    }
}

/// 0.0..1.0 的随机数 → `0..len` 的下标（含 NaN / 越界值的夹取，绝不 panic）
fn pick_index(r: f64, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let x = if r.is_nan() { 0.0 } else { r.clamp(0.0, 1.0) };
    // 用 (len - 1e-9) 乘法而不是 floor(x * len)：x = 1.0 时不会越界
    ((x * len as f64) as usize).min(len - 1)
}

/// 取文案里出现的全部占位符 key（`validate` 用）
fn placeholders(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                out.push(after[..close].trim().to_string());
                rest = &after[close + 1..];
            }
            None => break,
        }
    }
    out
}

// ═══════════════════════════════════════════════════════════════════
//  四种文案形态
// ═══════════════════════════════════════════════════════════════════

/// 应用内 toast：`【标题】一句话`。
///
/// 机主的默认建议是**这条关掉**（打断聊天），但通道本身要有 —— 设置项在前端。
pub fn popup_text(ev: &PlannedEvent) -> String {
    format!("【{}】{}", ev.title, ev.text)
}

/// 地图气泡：比 popup 更短，**≤ [`BUBBLE_MAX_CHARS`] 字**（超了截断加省略号）。
///
/// 气泡贴在地图角色头上，位置有限；按**字符**数截而不是字节数 ——
/// 中文一字三字节，按字节截会拦腰砍出乱码。
pub fn bubble_text(ev: &PlannedEvent) -> String {
    truncate_chars(trim_tail(&ev.text), BUBBLE_MAX_CHARS)
}

/// 塞进对话上下文的提示：让角色**自己**说出口，而不是生硬复述。
pub fn speech_hint(ev: &PlannedEvent) -> String {
    format!(
        "刚才发生了：{}（你可以自然地提一句，不要生硬复述）",
        ev.text
    )
}

/// 写进角色记忆的一行：`旁白: …`（与对话记忆 `名称: 内容` 同形，≤ 40 字）。
///
/// 用「旁白」是因为记忆管线**认识**它：`memory_builder.rs::format_context_line`
/// 对 `旁白`/`系统` 只取内容、不加名字前缀，`game_status.rs` 与 `api/chat.rs`
/// 也都在用 `display_name = "旁白"` 造叙述行。世界事件正是"角色没说话但发生了"，
/// 落在旁白里最自然。
pub fn memory_line(ev: &PlannedEvent) -> String {
    format!(
        "旁白: {}",
        truncate_chars(trim_tail(&ev.text), MEMORY_MAX_CHARS)
    )
}

/// 去尾部句读（气泡/记忆行里挂个句号显得很正式）
fn trim_tail(s: &str) -> &str {
    s.trim().trim_end_matches(['。', '！', '？', '!', '?', '，', ',', '、', '…', ' '])
}

/// 按**字符**截断，超长时用 `…` 占掉最后一个字符位（保证总长 ≤ `max`）
fn truncate_chars(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    let mut out: String = chars[..max - 1].iter().collect();
    out.push('…');
    out
}

// ═══════════════════════════════════════════════════════════════════
//  单测
//
//  跑法（手机上没有 cargo，这是唯一能真跑的路子）：
//    rustc --edition 2021 --test -L dependency=<deps> \
//          --extern serde=<libserde-*.rlib> --extern serde_json=<libserde_json-*.rlib> \
//          -o events_test shim.rs
//  其中 shim.rs 只有一行：`#[path = ".../world_map/events.rs"] pub mod events;`
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个"足够久以后"的时刻：不加冷却/惩罚的干净基准
    const NOW: i64 = 1_800_000_000;

    /// 固定 rng：永远返回同一个值。
    ///
    /// 特意写成"一个函数 + 参数"（而不是"取第一个词"/"取最后一个词"两个函数）：
    /// 两个函数的返回类型是**不同的**不透明类型（`impl FnMut` 每次实例化都不同），
    /// 放进同一个数组会编译不过 —— `[rng_a(), rng_b()]` 不可用。
    fn rng_fixed(v: f64) -> impl FnMut() -> f64 {
        move || v
    }

    /// 干净上下文：不在消费/户外场所、没有天气、没有节日、心情体力未知。
    /// **地带 `place` 留空**是有意的 —— 设成"便利店"会让消费类一直被加成。
    fn ctx_at(hour: u8) -> EventContext {
        EventContext {
            role: "小满".into(),
            area: "广州市·越秀区".into(),
            place: String::new(),
            hour,
            ..Default::default()
        }
    }

    /// 某条事件此刻的权重
    fn w(id: &str, ctx: &EventContext) -> f64 {
        weight_for(table().get(id).expect("事件 id 存在"), ctx, NOW)
    }

    /// 造一条测试用事件（文案里带占位符，方便验证渲染）
    fn tdef(id: &'static str, cat: Category, weight: f64) -> EventDef {
        EventDef {
            id,
            category: cat,
            title: "标题",
            text: "{role}在{place}",
            weight,
            cooldown_secs: 60,
            ..EventDef::DEFAULT
        }
    }

    /// 抽一次并只取 id（`plan_event` 的 rng 是 `impl FnMut`（`Sized`），
    /// 所以这里也得写成泛型函数、不能是闭包）
    fn pick_id(t: &EventTable, c: &EventContext, roll: f64, r: &mut impl FnMut() -> f64) -> String {
        plan_event(t, c, NOW, roll, r).unwrap().id
    }

    /// 把一条定义渲染成成品事件（验证文案用）
    fn render_def(def: &EventDef, ctx: &EventContext) -> PlannedEvent {
        let mut r = rng_fixed(0.0);
        PlannedEvent {
            id: def.id.to_string(),
            category: def.category,
            title: def.title.to_string(),
            text: render_text(def, ctx, &mut r),
            weight_used: def.weight,
            at_secs: NOW,
        }
    }

    /// 构造一个"能解锁这条事件全部闸门"的上下文（表完整性检查用）
    fn unlocking_ctx(def: &EventDef) -> EventContext {
        let mut c = EventContext {
            role: "小满".into(),
            area: "广州市·越秀区".into(),
            ..Default::default()
        };
        c.hour = def.hours.0; // 窗口起点一定在窗口内（含跨零点窗口）
        if !def.places.is_empty() {
            c.place = def.places[0].to_string();
        }
        if let Some(outdoor) = def.outdoor {
            c.indoors = !outdoor;
        }
        if let Some(kind) = def.weather.first() {
            c.weather = match kind {
                WeatherKind::Rain => "小雨",
                WeatherKind::Snow => "小雪",
                WeatherKind::Fog => "雾",
                WeatherKind::Clear => "晴",
                WeatherKind::Cloudy => "多云",
                WeatherKind::Hot => "高温",
                WeatherKind::Cold => "降温",
                WeatherKind::Other => "",
            }
            .to_string();
        }
        // 温度：先满足 min/max 闸门，再保证分类与 weather 列表一致
        if let Some(min) = def.min_temp_c {
            c.temp_c = Some(min);
        }
        if let Some(max) = def.max_temp_c {
            c.temp_c = Some(max);
        }
        match def.weather.first() {
            Some(WeatherKind::Hot) => c.temp_c = Some(HOT_TEMP_C + 2.0),
            Some(WeatherKind::Cold) => c.temp_c = Some(COLD_TEMP_C - 2.0),
            _ => {}
        }
        if def.festival_only {
            c.festival = true;
            c.festival_name = Some("中秋节".into());
        }
        c
    }

    // ── 事件表完整性 ────────────────────────────────────────────

    /// 十类一个不少，且每类至少 3 条（机主：每类至少 3 个具体事件）
    #[test]
    fn table_covers_all_ten_categories_with_at_least_three() {
        let t = table();
        assert_eq!(t.categories().len(), 10, "十类必须都有事件");
        for cat in Category::ALL {
            let n = t.by_category(cat).len();
            assert!(n >= 3, "{} 只有 {n} 条事件（要求 ≥ 3）", cat.key());
        }
        assert!(t.len() >= 32, "总事件数 {} 少于 32", t.len());
    }

    /// 机主逐条确认过的例子必须在表里（防止后来改表时把某条删没了）
    #[test]
    fn table_keeps_the_confirmed_examples() {
        let ids = [
            "weather.rain", "weather.snow", "weather.fog", "weather.heat", "weather.cool",
            "traffic.jam", "traffic.last_bus", "traffic.accident", "traffic.subway_delay",
            "social.invite", "social.stranger", "social.acquaintance", "social.stand_up",
            "work.overtime", "work.meeting", "work.exam", "work.deadline",
            "health.cold", "health.insomnia", "health.unwell", "health.oversleep",
            "money.sale", "money.lost", "money.found", "money.bill",
            "accident.power_out", "accident.water_out", "accident.phone_dead", "accident.locked_out",
            "festival.decor", "festival.activity", "festival.fireworks",
            "mood.good", "mood.low", "mood.irritable",
            "luck.win", "luck.puddle", "luck.last_one",
        ];
        for id in ids {
            assert!(table().get(id).is_some(), "表里缺了 {id}");
        }
    }

    /// `validate()` 必须干净：id 唯一、类别前缀一致、权重与冷却为正、文案非空、
    /// 占位符全在白名单里
    #[test]
    fn table_validates_clean() {
        let problems = table().validate();
        assert!(problems.is_empty(), "事件表自检不通过：{problems:#?}");
    }

    /// 每条事件都能在**某个**上下文里被解锁（权重 > 0）—— 防止闸门写反、
    /// 或者某条事件被自己的闸门永久锁死
    #[test]
    fn every_event_is_reachable_somewhere() {
        for def in table().events() {
            let ctx = unlocking_ctx(def);
            let weight = weight_for(def, &ctx, NOW);
            assert!(
                weight > 0.0,
                "{} 在 unlocking_ctx 下权重仍是 {weight}（闸门写反了？）",
                def.id
            );
        }
    }

    /// 所有占位符都能被替换，且结果里**不会留下 `{`**
    #[test]
    fn placeholders_are_all_replaced_and_never_leak() {
        for def in table().events() {
            let ctx = unlocking_ctx(def);
            for v in [0.0, 0.999_999] {
                let mut r = rng_fixed(v);
                let text = render_text(def, &ctx, &mut r);
                assert!(!text.contains('{'), "{} 渲染后残留花括号：{text}", def.id);
                assert!(!text.contains('}'), "{} 渲染后残留花括号：{text}", def.id);
                assert!(!text.is_empty(), "{} 渲染成空串", def.id);
            }
        }
    }

    /// `{role}` 用上下文里的角色名；没给名字时退成"你"
    #[test]
    fn role_placeholder_uses_context_and_falls_back() {
        let def = tdef("mood.x", Category::Mood, 1.0);
        let mut c = ctx_at(12);
        c.place = "公园".into();
        let mut r = rng_fixed(0.0);
        assert_eq!(render_text(&def, &c, &mut r), "小满在公园");
        c.role = "  ".into();
        assert_eq!(render_text(&def, &c, &mut r), "你在公园");
    }

    /// `{place}`：设施名优先 → 行政区最后一段 → "附近"
    #[test]
    fn place_placeholder_falls_back_through_three_levels() {
        let def = EventDef {
            text: "在{place}",
            ..tdef("mood.x", Category::Mood, 1.0)
        };
        let mut c = ctx_at(12);
        c.area = "广州市·越秀区·东山口".into();
        c.place = "便利店".into();
        let mut r = rng_fixed(0.0);
        assert_eq!(render_text(&def, &c, &mut r), "在便利店");
        c.place = String::new();
        assert_eq!(render_text(&def, &c, &mut r), "在东山口");
        c.area = String::new();
        assert_eq!(render_text(&def, &c, &mut r), "在附近");
    }

    /// `{area}` 空退成"这里"；不认识的占位符与残缺括号都丢掉
    #[test]
    fn unknown_and_broken_placeholders_are_dropped() {
        let def = EventDef {
            text: "A{area}B{nope}C{ oops",
            ..tdef("mood.x", Category::Mood, 1.0)
        };
        let mut c = ctx_at(12);
        c.area = String::new();
        let mut r = rng_fixed(0.0);
        assert_eq!(render_text(&def, &c, &mut r), "A这里BC");
    }

    /// `{someone}` 跟着 rng 走，且只能取到候选词
    #[test]
    fn someone_placeholder_follows_rng() {
        let def = EventDef {
            text: "{someone}",
            ..tdef("social.x", Category::Social, 1.0)
        };
        let c = ctx_at(12);
        let mut a = rng_fixed(0.0);
        let mut b = rng_fixed(0.999_999);
        let first = render_text(&def, &c, &mut a);
        let last = render_text(&def, &c, &mut b);
        assert_eq!(first, SOMEONE_WORDS[0]);
        assert_eq!(last, SOMEONE_WORDS[SOMEONE_WORDS.len() - 1]);
        for word in [&first, &last] {
            assert!(SOMEONE_WORDS.contains(&word.as_str()));
        }
    }

    /// `{weather}` 取"转"之前那段（"小雨转多云" → "小雨"）
    #[test]
    fn weather_placeholder_uses_short_form() {
        let def = EventDef {
            text: "下起了{weather}",
            ..tdef("weather.x", Category::Weather, 1.0)
        };
        let mut c = ctx_at(12);
        c.weather = "中雨转小雨".into();
        let mut r = rng_fixed(0.0);
        assert_eq!(render_text(&def, &c, &mut r), "下起了中雨");
    }

    /// `{festival}` 有名字就用名字，没有就"节日"
    #[test]
    fn festival_placeholder_defaults() {
        let def = EventDef {
            text: "今天是{festival}",
            ..tdef("festival.x", Category::Festival, 1.0)
        };
        let mut c = ctx_at(12);
        c.festival = true;
        let mut r = rng_fixed(0.0);
        assert_eq!(render_text(&def, &c, &mut r), "今天是节日");
        c.festival_name = Some("春节".into());
        assert_eq!(render_text(&def, &c, &mut r), "今天是春节");
    }

    // ── 权重修正规则 ────────────────────────────────────────────

    /// 深夜：社交降、工作学习降
    #[test]
    fn deep_night_lowers_social_and_work() {
        let day = ctx_at(12);
        let night = ctx_at(2);
        assert_eq!(w("social.stranger", &night), w("social.stranger", &day) * SOCIAL_NIGHT_FACTOR);
        assert_eq!(w("work.deadline", &night), w("work.deadline", &day) * WORK_NIGHT_FACTOR);
        assert!(night.is_deep_night() && !day.is_deep_night());
    }

    /// 深夜：健康升（失眠 / 感冒）
    #[test]
    fn deep_night_raises_health() {
        let day = ctx_at(12);
        let mut night = ctx_at(23);
        night.indoors = true; // 失眠是"仅室内"事件
        assert_eq!(w("health.cold", &night), w("health.cold", &day) * HEALTH_NIGHT_FACTOR);
        assert!(w("health.insomnia", &night) > 0.0);
        // 同样的深夜，如果人在户外，"失眠"根本不可选（闸门先于权重）
        assert_eq!(w("health.insomnia", &ctx_at(23)), 0.0);
    }

    /// 天气：下雨/下雪提高交通
    #[test]
    fn rain_raises_traffic() {
        let mut clear = ctx_at(12);
        clear.weather = "晴".into();
        let mut rain = ctx_at(12);
        rain.weather = "中雨".into();
        let mut snow = ctx_at(12);
        snow.weather = "小雪".into();
        assert_eq!(w("traffic.jam", &rain), w("traffic.jam", &clear) * TRAFFIC_RAIN_FACTOR);
        assert_eq!(w("traffic.jam", &snow), w("traffic.jam", &clear) * TRAFFIC_RAIN_FACTOR);
        assert!(rain.weather_kind().is_wet() && !clear.weather_kind().is_wet());
    }

    /// 天气：下雨/下雪降低 **luck 里的户外项**，室内项不受影响
    #[test]
    fn rain_lowers_outdoor_luck_only() {
        let mut clear = ctx_at(12);
        clear.weather = "晴".into();
        let mut rain = ctx_at(12);
        rain.weather = "小雨".into();
        // 真表里的 luck.puddle 只在雨天开闸，晴天权重本来就是 0（闸门，不是这条规则）
        assert_eq!(w("luck.puddle", &clear), 0.0);
        assert_eq!(w("luck.puddle", &rain), 5.0 * OUTDOOR_LUCK_RAIN_FACTOR);
        // luck.win 没有户外标记 → 雨雪天不降
        assert_eq!(w("luck.win", &rain), w("luck.win", &clear));
        // 规则本身用一对"只差 outdoor 标记"的合成事件验证：
        // 户外项 ×0.35，室内项原样
        let outdoor = EventDef {
            outdoor: Some(true),
            ..tdef("luck.demo_outdoor", Category::Luck, 5.0)
        };
        let indoor = tdef("luck.demo_indoor", Category::Luck, 5.0);
        assert_eq!(
            weight_for(&outdoor, &rain, NOW),
            weight_for(&outdoor, &clear, NOW) * OUTDOOR_LUCK_RAIN_FACTOR
        );
        assert_eq!(
            weight_for(&indoor, &rain, NOW),
            weight_for(&indoor, &clear, NOW)
        );
    }

    /// 位置：在餐厅/商场提高消费类
    #[test]
    fn money_place_raises_money_category() {
        let mut plain = ctx_at(12);
        plain.place = "街上".into();
        let mut mall = ctx_at(12);
        mall.place = "商场".into();
        let mut restaurant = ctx_at(12);
        restaurant.place = "餐厅".into();
        assert!(mall.at_money_place() && restaurant.at_money_place());
        assert!(!plain.at_money_place());
        assert_eq!(w("money.bill", &mall), w("money.bill", &plain) * MONEY_PLACE_FACTOR);
        assert_eq!(
            w("money.bill", &restaurant),
            w("money.bill", &plain) * MONEY_PLACE_FACTOR
        );
        // 别的类别不吃这个加成
        assert_eq!(w("mood.good", &mall), w("mood.good", &plain));
    }

    /// 位置：在公园/户外提高天气类与运气类
    #[test]
    fn outdoor_place_raises_weather_and_luck() {
        let mut plain = ctx_at(12);
        plain.place = "街上".into();
        let mut park = ctx_at(12);
        park.place = "公园".into();
        park.weather = "小雨".into(); // weather.rain 要真在下雨才开闸
        plain.weather = "小雨".into();
        assert!(park.at_outdoor_place() && !plain.at_outdoor_place());
        assert_eq!(
            w("weather.rain", &park),
            w("weather.rain", &plain) * OUTDOOR_PLACE_WEATHER_FACTOR
        );
        assert_eq!(
            w("luck.win", &park),
            w("luck.win", &plain) * OUTDOOR_PLACE_LUCK_FACTOR
        );
    }

    /// 情绪低落：情绪类升；心情正常或未知时不变
    #[test]
    fn low_mood_raises_mood_category() {
        let plain = ctx_at(12);
        let mut low = ctx_at(12);
        low.mood = Some(0.1);
        let mut happy = ctx_at(12);
        happy.mood = Some(0.9);
        assert_eq!(w("mood.low", &low), w("mood.low", &plain) * LOW_MOOD_FACTOR);
        assert_eq!(w("mood.low", &happy), w("mood.low", &plain));
        assert_eq!(w("mood.low", &plain), 5.0);
    }

    /// 节日当天：节日类大幅提高（×5）
    #[test]
    fn festival_day_boosts_festival() {
        // 时刻取 19 点：节日事件里"烟花"有 (18,23) 的时段闸门，
        // 用正午做基准会把它误判成"节日当天也不可选"
        let plain = ctx_at(19);
        let mut fest = ctx_at(19);
        fest.festival = true;
        assert!(!plain.is_festival() && fest.is_festival());
        // 真表里的节日事件是 festival_only：平日闸门直接关掉（0），节日当天才可选
        for def in table().by_category(Category::Festival) {
            assert_eq!(w(def.id, &plain), 0.0, "{} 平日不该可选", def.id);
            assert!(w(def.id, &fest) > def.weight, "{} 节日当天应当被大幅加成", def.id);
        }
        // ×5 这条规则本身，用一条不带 festival_only 的合成事件量出来
        let demo = tdef("festival.demo", Category::Festival, 10.0);
        assert_eq!(
            weight_for(&demo, &fest, NOW),
            weight_for(&demo, &plain, NOW) * FESTIVAL_FACTOR
        );
        // 顺带：节日当天社交也小升
        assert_eq!(
            w("social.stranger", &fest),
            w("social.stranger", &plain) * FESTIVAL_SOCIAL_FACTOR
        );
        // 只给名字也算节日
        let mut named = ctx_at(19);
        named.festival_name = Some("中秋".into());
        assert!(named.is_festival());
        assert!(w("festival.decor", &named) > 0.0);
    }

    /// 体力低：健康升、工作学习降
    #[test]
    fn low_energy_raises_health_and_lowers_work() {
        let plain = ctx_at(12);
        let mut tired = ctx_at(12);
        tired.energy = Some(0.05);
        assert_eq!(
            w("health.cold", &tired),
            w("health.cold", &plain) * LOW_ENERGY_HEALTH_FACTOR
        );
        assert_eq!(
            w("work.deadline", &tired),
            w("work.deadline", &plain) * LOW_ENERGY_WORK_FACTOR
        );
    }

    /// 玩家在身边：社交升；离得远：社交降
    #[test]
    fn player_distance_modulates_social() {
        let plain = ctx_at(12);
        let mut near = ctx_at(12);
        near.player_distance_m = Some(10.0);
        let mut far = ctx_at(12);
        far.player_distance_m = Some(20_000.0);
        assert_eq!(
            w("social.stranger", &near),
            w("social.stranger", &plain) * SOCIAL_NEAR_PLAYER_FACTOR
        );
        assert_eq!(
            w("social.stranger", &far),
            w("social.stranger", &plain) * SOCIAL_FAR_PLAYER_FACTOR
        );
    }

    /// 多条规则叠加是乘法（雨天 + 深夜 + 在商场）
    #[test]
    fn modifiers_stack_multiplicatively() {
        let mut c = ctx_at(2); // 深夜
        c.weather = "小雨".into(); // 雨天
        c.place = "商场".into(); // 消费场所
        let base = 5.0; // money.bill 的基础权重
        assert_eq!(
            w("money.bill", &c),
            base * MONEY_PLACE_FACTOR,
            "消费只在深夜/雨天没有额外修正"
        );
        let mut traffic = ctx_at(2);
        traffic.weather = "小雨".into();
        // traffic.accident 没有时段闸门（jam/subway_delay 在这个点被时段闸门关掉了，
        // 正好说明"闸门"与"修正"是两件事）
        assert_eq!(w("traffic.jam", &traffic), 0.0, "凌晨 2 点不该堵车");
        assert_eq!(
            w("traffic.accident", &traffic),
            4.0 * TRAFFIC_RAIN_FACTOR,
            "交通在深夜没有额外修正"
        );
    }

    /// hour 越界值取模救回来
    #[test]
    fn hour_out_of_range_is_normalized() {
        let mut c = ctx_at(12);
        c.hour = 25;
        assert_eq!(c.hour_of(), 1);
        assert!(c.is_deep_night());
        c.hour = 200; // 200 % 24 = 8
        assert_eq!(c.hour_of(), 8);
    }

    // ── 闸门 ────────────────────────────────────────────────────

    /// 天气闸门：晴天不会编出下雨/下雪
    #[test]
    fn weather_gate_blocks_mismatched_weather() {
        let mut clear = ctx_at(12);
        clear.weather = "晴".into();
        assert!(!gate_open(table().get("weather.rain").unwrap(), &clear));
        assert_eq!(w("weather.rain", &clear), 0.0);
        let mut rain = ctx_at(12);
        rain.weather = "小雨".into();
        assert!(gate_open(table().get("weather.rain").unwrap(), &rain));
        assert!(w("weather.rain", &rain) > 0.0);
        // 天气未知（空串）→ 天气类一律不出
        let unknown = ctx_at(12);
        assert_eq!(w("weather.rain", &unknown), 0.0);
        assert_eq!(w("weather.snow", &unknown), 0.0);
        assert_eq!(w("weather.fog", &unknown), 0.0);
    }

    /// 天气分类：关键词优先，其次气温
    #[test]
    fn weather_kind_prefers_keywords_then_temp() {
        assert_eq!(WeatherKind::of("雨夹雪", Some(1.0)), WeatherKind::Snow);
        assert_eq!(WeatherKind::of("雷阵雨", None), WeatherKind::Rain);
        assert_eq!(WeatherKind::of("霾", None), WeatherKind::Fog);
        assert_eq!(WeatherKind::of("", Some(35.0)), WeatherKind::Hot);
        assert_eq!(WeatherKind::of("", Some(2.0)), WeatherKind::Cold);
        assert_eq!(WeatherKind::of("", Some(22.0)), WeatherKind::Other);
        assert_eq!(WeatherKind::of("", None), WeatherKind::Other);
    }

    /// 时段闸门：闭区间 + 跨零点
    #[test]
    fn hour_gate_respects_wrapping_window() {
        assert!(hour_in_window(6, (6, 22)));
        assert!(hour_in_window(22, (6, 22)));
        assert!(!hour_in_window(23, (6, 22)));
        assert!(hour_in_window(23, (21, 1)));
        assert!(hour_in_window(0, (21, 1)));
        assert!(hour_in_window(1, (21, 1)));
        assert!(!hour_in_window(2, (21, 1)));
        // 末班车：21–1 点可触发，午后不可
        let mut noon = ctx_at(14);
        noon.place = "站台".into();
        noon.indoors = false;
        assert_eq!(w("traffic.last_bus", &noon), 0.0);
        let mut night = ctx_at(23);
        night.place = "站台".into();
        assert!(w("traffic.last_bus", &night) > 0.0);
    }

    /// 场所闸门：不在店里就没有"打折"
    #[test]
    fn place_gate_requires_a_hit() {
        let mut park = ctx_at(12);
        park.place = "公园".into();
        assert_eq!(w("money.sale", &park), 0.0);
        let mut shop = ctx_at(12);
        shop.place = "便利店".into();
        assert!(w("money.sale", &shop) > 0.0);
        // place_kind 也算数（前端给的是 type key）
        let mut kind_only = ctx_at(12);
        kind_only.place_kind = "commercial_店".into();
        assert!(w("money.sale", &kind_only) > 0.0);
    }

    /// 室内外闸门：室内不会有"堵车/钥匙忘带"，户外不会有"停电/失眠"
    #[test]
    fn indoor_outdoor_gate() {
        let mut indoors = ctx_at(12);
        indoors.indoors = true;
        assert_eq!(w("traffic.jam", &indoors), 0.0);
        assert_eq!(w("accident.locked_out", &indoors), 0.0);
        assert!(w("accident.power_out", &indoors) > 0.0);
        let mut outdoors = ctx_at(12);
        outdoors.indoors = false;
        assert!(w("traffic.jam", &outdoors) > 0.0);
        assert_eq!(w("accident.power_out", &outdoors), 0.0);
    }

    /// 节日专属：平时一条节日事件都不出，节日当天才全部开闸
    #[test]
    fn festival_only_gate() {
        let plain = ctx_at(19);
        for def in table().by_category(Category::Festival) {
            assert_eq!(w(def.id, &plain), 0.0, "{} 不该在平日出现", def.id);
        }
        let mut fest = ctx_at(19);
        fest.festival = true;
        for def in table().by_category(Category::Festival) {
            assert!(w(def.id, &fest) > 0.0, "{} 节日当天应当可选", def.id);
        }
    }

    // ── 冷却 / 节流 / 同类惩罚 ──────────────────────────────────

    /// 冷却中只把**那一条**权重置 0，同类其他事件照常可选
    #[test]
    fn cooldown_zeroes_only_that_event() {
        let mut c = ctx_at(12);
        assert!(w("mood.good", &c) > 0.0);
        c.recent = vec![RecentEvent::at("mood.good", NOW - 100)];
        assert_eq!(w("mood.good", &c), 0.0, "冷却中的事件权重必须归零");
        assert!(w("mood.low", &c) > 0.0, "同类其他事件不该被连坐");
        assert!(w("mood.calm", &c) > 0.0);
        // 候选里也只是少了那一条，不是整表清空
        let picks = candidates(table(), &c, NOW);
        assert!(picks.iter().all(|(d, _)| d.id != "mood.good"));
        assert!(picks.len() > 3, "冷却一条不该让候选塌掉：{}", picks.len());
        // 冷却过期后恢复
        c.recent = vec![RecentEvent::at("mood.good", NOW - 7200)];
        assert!(w("mood.good", &c) > 0.0);
    }

    /// 未来时间戳（时钟回拨/脏数据）按"刚刚发生"处理，冷却照样生效
    #[test]
    fn future_timestamp_still_cools_down() {
        let mut c = ctx_at(12);
        c.recent = vec![RecentEvent::at("mood.good", NOW + 10_000)];
        assert!(cooldown_active(table().get("mood.good").unwrap(), &c, NOW));
        assert_eq!(w("mood.good", &c), 0.0);
        // 而且它不能把"全局节流"永久锁死：last_event_at 只看 ≤ now 的
        assert_eq!(last_event_at(&c, NOW), None);
        assert!(!global_throttled(&c, NOW));
    }

    /// 全局节流：距上一次任何事件 < 300s → 整轮 None
    #[test]
    fn global_throttle_blocks_within_min_gap() {
        let mut c = ctx_at(12);
        c.recent = vec![RecentEvent::at("mood.calm", NOW - 100)];
        assert!(global_throttled(&c, NOW));
        let mut r = rng_fixed(0.0);
        assert!(
            plan_event(table(), &c, NOW, 0.0, &mut r).is_none(),
            "节流期内不该发生任何事件"
        );
        // 边界：恰好 300s 整可以发生（< 才算太近）
        c.recent = vec![RecentEvent::at("mood.calm", NOW - MIN_GAP_SECS)];
        assert!(!global_throttled(&c, NOW));
    }

    /// 节流期过了就恢复正常
    #[test]
    fn global_throttle_releases_after_min_gap() {
        let mut c = ctx_at(12);
        c.recent = vec![RecentEvent::at("mood.calm", NOW - MIN_GAP_SECS - 1)];
        assert!(!global_throttled(&c, NOW));
        let mut r = rng_fixed(0.0);
        assert!(plan_event(table(), &c, NOW, 0.5, &mut r).is_some());
    }

    /// 同类别连着来：×0.3，连着两条 → ×0.09；中间插别的类别就断链
    ///
    /// 主体用**自造表**而不是真表：真表里同类别事件的冷却都很长
    /// （`traffic.subway_delay` 冷却 3600s），想让"上一条同类"足够近、
    /// 又不让目标自己落进冷却，只有把冷却调小才好构造。
    /// 真表上的行为另有 `cooldown_zeroes_only_that_event` 把关，
    /// 末尾也补了一条真表断言。
    #[test]
    fn same_category_back_to_back_penalty() {
        let t = EventTable::new(vec![
            tdef("traffic.a", Category::Traffic, 10.0), // cooldown 60s
            tdef("traffic.b", Category::Traffic, 10.0),
            tdef("mood.c", Category::Mood, 10.0),
        ]);
        let c = ctx_at(12);
        let base = weight_for(t.get("traffic.b").unwrap(), &c, NOW);
        assert_eq!(base, 10.0);

        let mut one = ctx_at(12);
        one.recent = vec![RecentEvent::at("traffic.a", NOW - 100)];
        assert_eq!(
            weight_for(t.get("traffic.b").unwrap(), &one, NOW),
            base * SAME_CATEGORY_PENALTY
        );

        let mut two = ctx_at(12);
        two.recent = vec![
            RecentEvent::at("traffic.a", NOW - 100),
            RecentEvent::at("traffic.b", NOW - 200),
        ];
        assert_eq!(
            weight_for(t.get("traffic.b").unwrap(), &two, NOW),
            base * SAME_CATEGORY_PENALTY.powi(2),
            "连着两条同类 → 惩罚平方"
        );

        // 中间隔了一条别的类别 → 不算"连着"
        let mut broken = ctx_at(12);
        broken.recent = vec![
            RecentEvent::at("traffic.a", NOW - 100),
            RecentEvent::at("mood.c", NOW - 200),
        ];
        assert_eq!(
            weight_for(t.get("traffic.b").unwrap(), &broken, NOW),
            base * SAME_CATEGORY_PENALTY
        );

        // 窗口外的旧事件不参与
        let mut old = ctx_at(12);
        old.recent = vec![RecentEvent::at("traffic.a", NOW - SAME_CATEGORY_WINDOW_SECS - 1)];
        assert_eq!(weight_for(t.get("traffic.b").unwrap(), &old, NOW), base);

        // 只有 id、没有 category 字段的老事件也能参与统计（从 id 前缀推）
        let mut id_only = ctx_at(12);
        id_only.recent = vec![RecentEvent::at("traffic.a", NOW - 100)];
        assert_eq!(
            id_only.recent[0].category(),
            Some(Category::Traffic),
            "id 前缀必须能推出类别"
        );
        assert_eq!(
            weight_for(t.get("traffic.b").unwrap(), &id_only, NOW),
            base * SAME_CATEGORY_PENALTY,
            "缺 category 字段时要退到 id 推断"
        );

        // 真表上也过一遍同一条规则（目标自己不在冷却里，所以量得出来）
        let plain = ctx_at(12);
        let mut real = ctx_at(12);
        real.recent = vec![RecentEvent::at("traffic.accident", NOW - 100)];
        assert_eq!(
            w("traffic.subway_delay", &real),
            w("traffic.subway_delay", &plain) * SAME_CATEGORY_PENALTY
        );
    }

    /// 类别反解与 id 前缀约定
    #[test]
    fn category_keys_and_ids_round_trip() {
        for cat in Category::ALL {
            assert_eq!(Category::from_key(cat.key()), Some(cat));
            assert_eq!(Category::from_id(&format!("{}.x", cat.key())), Some(cat));
            assert!(!cat.zh().is_empty());
        }
        assert_eq!(Category::from_key("nope"), None);
        assert_eq!(Category::from_id(""), None);
    }

    // ── 抽签 ────────────────────────────────────────────────────

    /// 全部候选权重 ≤ 0 → None（不是 panic，也不是硬塞一条）
    #[test]
    fn all_zero_weights_returns_none() {
        let t = EventTable::new(vec![
            tdef("mood.a", Category::Mood, 1.0),
            tdef("mood.b", Category::Mood, 2.0),
        ]);
        let mut c = ctx_at(12);
        c.recent = vec![
            RecentEvent::at("mood.a", NOW - 1),
            RecentEvent::at("mood.b", NOW - 1),
        ];
        assert!(candidates(&t, &c, NOW).is_empty());
        let mut r = rng_fixed(0.0);
        assert!(plan_event(&t, &c, NOW, 0.5, &mut r).is_none());

        // 空表同样 None
        let empty = EventTable::new(Vec::new());
        assert!(plan_event(&empty, &ctx_at(12), NOW, 0.5, &mut r).is_none());

        // 权重非正/NaN 的定义直接被当成 0
        let bad = EventTable::new(vec![
            tdef("mood.c", Category::Mood, 0.0),
            tdef("mood.d", Category::Mood, f64::NAN),
        ]);
        assert!(plan_event(&bad, &ctx_at(12), NOW, 0.5, &mut r).is_none());
    }

    /// roll 边界：0.0 / 1.0 / 恰好落在区间边界 / 越界 / NaN —— 都不 panic、不越界
    #[test]
    fn roll_boundaries_never_panic() {
        let t = EventTable::new(vec![
            tdef("mood.a", Category::Mood, 1.0),
            tdef("mood.b", Category::Mood, 2.0),
            tdef("mood.c", Category::Mood, 3.0),
        ]);
        let c = ctx_at(12);
        let mut r = rng_fixed(0.0);
        // 区间边界：1.0/6.0 恰好是 a|b 的分界，3.0/6.0 是 b|c 的分界
        for roll in [
            0.0,
            1.0,
            -1.0,
            2.0,
            f64::NAN,
            f64::INFINITY,
            1.0 / 6.0,
            3.0 / 6.0,
            1.0 / 6.0 - f64::EPSILON,
            1.0 - f64::EPSILON,
            0.999_999_999,
        ] {
            let ev = plan_event(&t, &c, NOW, roll, &mut r).expect("总有候选");
            assert!(
                ["mood.a", "mood.b", "mood.c"].contains(&ev.id.as_str()),
                "roll={roll} 抽出了表外的事件 {}",
                ev.id
            );
            assert!(ev.weight_used > 0.0);
        }
    }

    /// 区间是**左闭右开**：分界点归下一条；roll=1.0 兜底取最后一条
    #[test]
    fn roll_lands_in_the_expected_bucket() {
        let t = EventTable::new(vec![
            tdef("mood.a", Category::Mood, 1.0),
            tdef("mood.b", Category::Mood, 2.0),
        ]);
        let c = ctx_at(12);
        let total = 3.0;
        let mut r = rng_fixed(0.0);
        assert_eq!(pick_id(&t, &c, 0.0, &mut r), "mood.a");
        assert_eq!(pick_id(&t, &c, 1.0 / total - 1e-9, &mut r), "mood.a");
        assert_eq!(pick_id(&t, &c, 1.0 / total, &mut r), "mood.b", "分界点归后一条");
        assert_eq!(pick_id(&t, &c, 0.99, &mut r), "mood.b");
        assert_eq!(pick_id(&t, &c, 1.0, &mut r), "mood.b", "roll=1.0 兜底最后一条");
    }

    /// 同样的输入必须给同样的结果（roll 相同、rng 相同）
    #[test]
    fn plan_event_is_deterministic() {
        let c = ctx_at(12);
        let mut a = rng_fixed(0.0);
        let mut b = rng_fixed(0.0);
        let e1 = plan_event(table(), &c, NOW, 0.42, &mut a).unwrap();
        let e2 = plan_event(table(), &c, NOW, 0.42, &mut b).unwrap();
        assert_eq!(e1, e2);
        assert_eq!(e1.at_secs, NOW);
        assert_eq!(e1.category, Category::from_id(&e1.id).unwrap());
    }

    /// 抽中的事件文案是**成品**：占位符已填、标题/权重都在
    #[test]
    fn planned_event_carries_rendered_text() {
        let mut c = ctx_at(12);
        c.weather = "小雨".into();
        let mut r = rng_fixed(0.0);
        let ev = plan_event(table(), &c, NOW, 0.3, &mut r).unwrap();
        let def = table().get(&ev.id).expect("抽中的必须是表里的事件");
        assert!(!ev.text.contains('{'), "成品文案不该有占位符：{}", ev.text);
        // 定义里用了哪个占位符，成品里就要有对应的值
        if def.text.contains("{role}") {
            assert!(ev.text.contains("小满"), "{{role}} 没被替换：{}", ev.text);
        }
        if def.text.contains("{place}") {
            assert!(ev.text.contains("越秀区"), "{{place}} 没退到行政区末段：{}", ev.text);
        }
        assert!(!ev.title.is_empty());
        assert!(ev.weight_used > 0.0);
        assert_eq!(ev.category, def.category);
    }

    /// 加权随机确实按权重分区间：把 roll 扫一遍，落点比例接近权重比例
    #[test]
    fn roll_distribution_follows_weights() {
        let t = EventTable::new(vec![
            tdef("mood.a", Category::Mood, 1.0),
            tdef("mood.b", Category::Mood, 3.0),
        ]);
        let c = ctx_at(12);
        let mut r = rng_fixed(0.0);
        let mut a: i32 = 0;
        const N: i32 = 1000;
        for i in 0..N {
            let roll = (i as f64 + 0.5) / N as f64;
            if plan_event(&t, &c, NOW, roll, &mut r).unwrap().id == "mood.a" {
                a += 1;
            }
        }
        // 1:3 → a 应当占 25% 上下（±3%）
        assert!((a - 250).abs() < 30, "a 落点 {a}/1000，偏离 1:3 的预期");
    }

    // ── 四种文案形态 ────────────────────────────────────────────

    /// popup：标题 + 一句话
    #[test]
    fn popup_text_has_title_and_body() {
        let def = table().get("traffic.jam").unwrap();
        let mut c = ctx_at(12);
        c.place = "路口".into();
        let ev = render_def(def, &c);
        let s = popup_text(&ev);
        assert!(s.starts_with("【堵车】"), "{s}");
        assert!(s.contains(&ev.text));
    }

    /// 气泡：**每条事件**都 ≤ 20 字，且非空
    #[test]
    fn bubble_text_never_exceeds_the_limit() {
        for def in table().events() {
            let ctx = unlocking_ctx(def);
            let ev = render_def(def, &ctx);
            let b = bubble_text(&ev);
            assert!(!b.is_empty(), "{} 的气泡是空的", def.id);
            assert!(
                b.chars().count() <= BUBBLE_MAX_CHARS,
                "{} 的气泡 {} 字：{b}",
                def.id,
                b.chars().count()
            );
        }
        // 边界：正好 20 字不截，21 字截成 19+…
        let mk = |t: &str| PlannedEvent {
            id: "x".into(),
            category: Category::Mood,
            title: "T".into(),
            text: t.into(),
            weight_used: 1.0,
            at_secs: 0,
        };
        let twenty: String = "字".repeat(20);
        assert_eq!(bubble_text(&mk(&twenty)), twenty);
        let twenty_one = "字".repeat(21);
        let cut = bubble_text(&mk(&twenty_one));
        assert_eq!(cut.chars().count(), 20);
        assert!(cut.ends_with('…'));
    }

    /// speech_hint：给模型的形态（含"不要生硬复述"的约束）
    #[test]
    fn speech_hint_has_the_agreed_shape() {
        let def = table().get("mood.good").unwrap();
        let ev = render_def(def, &ctx_at(12));
        let s = speech_hint(&ev);
        assert!(s.starts_with("刚才发生了："), "{s}");
        assert!(s.contains(&ev.text));
        assert!(s.contains("不要生硬复述"));
    }

    /// memory_line：`旁白: …` 且 ≤ 40 字（**每条事件**都查一遍）
    #[test]
    fn memory_line_matches_dialogue_memory_shape() {
        for def in table().events() {
            let ctx = unlocking_ctx(def);
            let ev = render_def(def, &ctx);
            let m = memory_line(&ev);
            assert!(m.starts_with("旁白: "), "{} 的记忆行形状不对：{m}", def.id);
            assert!(
                m.chars().count() <= MEMORY_MAX_CHARS + 4,
                "{} 的记忆行太长（{} 字）：{m}",
                def.id,
                m.chars().count()
            );
            assert!(!m.contains('{'));
        }
    }

    /// 过长文案的截断：中文按**字符**数截，不会砍出半个字
    #[test]
    fn truncation_is_char_wise() {
        let long: String = "一二三四五六七八九十".repeat(6); // 60 字
        let out = truncate_chars(&long, 10);
        assert_eq!(out.chars().count(), 10);
        assert!(out.ends_with('…'));
        assert!(long.starts_with(&out[..out.len() - '…'.len_utf8()]));
        assert_eq!(truncate_chars("短", 10), "短");
        assert_eq!(truncate_chars("短", 0), "");
        assert_eq!(truncate_chars("短", 1), "短");
        assert_eq!(truncate_chars("长短", 1), "…");
    }

    /// PlannedEvent 过 Tauri/JSON 边界：字段名与类别 key 都要稳定
    #[test]
    fn planned_event_serializes_with_stable_keys() {
        let def = table().get("weather.rain").unwrap();
        let mut c = ctx_at(12);
        c.weather = "小雨".into();
        let ev = render_def(def, &c);
        let v = serde_json::to_value(&ev).unwrap();
        for key in ["id", "category", "title", "text", "weight_used", "at_secs"] {
            assert!(v.get(key).is_some(), "序列化结果缺字段 {key}：{v}");
        }
        assert_eq!(v["category"], serde_json::json!("weather"));
        assert_eq!(v["text"], serde_json::json!(ev.text));
        // summary::last_event 认 text/title 两个键 → 落进 MapRuntime.events 就能被注入用
        assert!(v.get("text").is_some() && v.get("title").is_some());
    }

    /// EventContext 能从前端那种"只有一半字段"的 JSON 直接反序列化
    #[test]
    fn context_deserializes_from_partial_json() {
        let c: EventContext = serde_json::from_str(r#"{"hour": 23, "weather": "小雨"}"#).unwrap();
        assert_eq!(c.hour, 23);
        assert_eq!(c.weather_kind(), WeatherKind::Rain);
        assert!(c.role.is_empty() && c.recent.is_empty());
        assert!(c.is_deep_night());

        let full: EventContext = serde_json::from_str(
            r#"{"role":"小满","area":"广州市","place":"便利店","indoors":true,"hour":9,
                "weather":"晴","temp_c":31.5,"mood":0.2,"energy":0.9,"player_distance_m":12.0,
                "festival":true,"festival_name":"中秋",
                "recent":[{"id":"weather.rain","at_secs":100}]}"#,
        )
        .unwrap();
        assert!(full.is_festival() && full.at_money_place() && full.indoors);
        assert_eq!(full.recent[0].category(), Some(Category::Weather));
    }

    /// 端到端：一个真实场景从候选到成品文案
    #[test]
    fn end_to_end_rainy_night_in_the_city() {
        let mut c = ctx_at(23); // 深夜
        c.area = "广州市·越秀区·东山口".into();
        c.place = "便利店".into();
        c.place_kind = "commercial".into();
        c.weather = "中雨".into();
        c.temp_c = Some(14.0);
        c.mood = Some(0.2);
        c.energy = Some(0.2);
        c.recent = vec![RecentEvent::at("weather.rain", NOW - MIN_GAP_SECS - 1)];

        let picks = candidates(table(), &c, NOW);
        assert!(picks.len() >= 5, "深夜雨天的候选不该太少：{}", picks.len());
        // 雨天的交通被抬高、深夜的社交被压低
        let pick_w = |id: &str| picks.iter().find(|(d, _)| d.id == id).map(|(_, w)| *w);
        // 深夜的社交被压到白天的 1/4
        assert_eq!(
            pick_w("social.stranger").unwrap(),
            w("social.stranger", &ctx_at(12)) * SOCIAL_NIGHT_FACTOR
        );
        assert!(pick_w("social.invite").is_none(), "22 点后不该有人约饭");
        assert!(pick_w("traffic.subway_delay").is_some());

        let mut r = rng_fixed(0.0);
        let ev = plan_event(table(), &c, NOW, 0.37, &mut r).unwrap();
        assert!(ev.at_secs == NOW);
        assert!(popup_text(&ev).starts_with('【'));
        assert!(bubble_text(&ev).chars().count() <= BUBBLE_MAX_CHARS);
        assert!(speech_hint(&ev).contains("刚才发生了"));
        assert!(memory_line(&ev).starts_with("旁白: "));
    }
}
