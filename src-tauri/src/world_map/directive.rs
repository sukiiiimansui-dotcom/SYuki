//! 世界模拟「位置指令」的剥离器（P4-1）。
//!
//! AI 在**回复的最后**输出一条位置指令：
//! ```text
//! ⟦wm:{"to":"楼下便利店","kind":"walk"}⟧
//! ```
//! 本模块负责在**流式切句之前**把这段指令从正文里摘出来，交给
//! [`super::move`] 的移动状态机去派发，同时保证用户可见的对话文本里
//! 一点痕迹都没有。
//!
//! ## 为什么在 `producer.rs` 这一层剥（选型理由）
//!
//! 管线是：`stream_with_tool_loop` → **producer（流式切句）** → consumer →
//! `processor::parse_and_classify_emotional_segments` → `consume_sentence`
//! （ONNX 情绪 → 翻译/TTS → `add_assistant_line`）→ `ai:reply` 事件 → 前端。
//!
//! 逐层比过之后选 producer，理由是**只有它同时满足这四条**：
//! 1. **唯一的上游**：句子一旦投进 sentence channel，后面每一环都会看到原文
//!    （情绪分类、翻译、TTS 语音合成会去念那串 JSON、`line_list` 历史、
//!    `ai:reply` 前端事件）。在 producer 摘掉，这些地方**根本见不到**指令。
//! 2. **能兜住跨 chunk 的残缺**：producer 本来就是攒 buffer 的状态机
//!    （`【`/`】` 那段），多一个 `⟦`/`⟧` 候选态不引入新的架构，而且天然
//!    能处理"指令被切成两个 LLM chunk"（`⟦wm:` 在前一个 chunk 尾部）。
//! 3. **不能复用 `【…】`**：那条路会被当成情绪标签 —— `processor.rs:159` 的
//!    `【[^】]*】` 会把它当 tag 送进 ONNX 情绪分类，`preprocess_text` 里的
//!    `content_braces_re` 还会先把 `{...}` 抠掉，等于指令被拆成两半乱码。
//! 4. **拿到 processor 就已经晚了**：`ai:reply` 是按"句"推给前端的，指令跟着
//!    最后一个句子一起流出去，用户在句子里就会看到 `⟦wm:{...}⟧` 一闪。
//!
//! ## 与官方行为的关系（提 PR 的红线）
//!
//! [`Scanner::new(false)`]（世界模拟没开）时，[`Scanner::push`] **原样返回**
//! 输入（`Cow::Borrowed`，零拷贝零分配），`finish()` 恒返回空串，
//! `directives` 恒为空 —— 也就是 producer 里那三行 `push_str` 拿到的字节
//! 与改造前**完全一致**。这条有差分测试兜着（见 `~/chk/p4/producer_diff.rs`）。
//!
//! ## 容错口径（都是刻意定的，逐条有单测）
//!
//! | 输入 | 行为 | 为什么 |
//! |---|---|---|
//! | `⟦` 后面不是 `wm:` | 原样当正文吐出（最多憋 3 个字符） | 不能因为一个装饰性符号吃掉正文 |
//! | 指令闭合但 JSON 残缺 | 吞掉、不派发、`invalid += 1` | 露出 `⟦wm:…` 比丢掉一条坏指令更糟 |
//! | 指令里没有目的地 | 同上 | |
//! | 指令体超过 [`MAX_BODY_CHARS`] 还没闭合 | **原文吐回** | 这里已经是正文了，宁可见到残缺标记也不吃掉一大段话 |
//! | 流结束时还挂着 `⟦`/`⟦w`/`⟦wm` | 吐回（未确认是指令） | 与"没见过指令"等价 |
//! | 流结束时挂着 `⟦wm:……` 没闭合 | 丢弃 + `invalid += 1` | 截断的指令，露出来就是残留 |
//! | 一轮出现多条 | 全部收集，**派发时取最后一条** | 模型最后的表态才是它真正想去的地方 |
//!
//! 注意：本模块不碰 `accumulated` 之外的任何东西，也不做任何 IO —— 纯函数 + 状态机，
//! 所以能脱离 tauri 单独 `rustc --test`（见文件末尾单测）。

use std::borrow::Cow;

use serde_json::Value;

/// 指令开头标记（U+27E6 MATHEMATICAL LEFT WHITE SQUARE BRACKET）
pub const OPEN: char = '⟦';
/// 指令结束标记（U+27E7）
pub const CLOSE: char = '⟧';
/// 开头的命名空间，防止和正文里的 `⟦…⟧` 撞车（必须逐字匹配，含小写冒号）
pub const PREFIX: &str = "wm:";
/// 指令体的字符数上限。真指令 < 120 字符；超过这个数说明它其实是一段正文。
pub const MAX_BODY_CHARS: usize = 256;
/// 目的地字段的候选键（`to` 是约定的，其余是给模型留的活口）
pub const TO_KEYS: [&str; 5] = ["to", "target", "destination", "dest", "place"];
/// 出行方式字段的候选键
pub const KIND_KEYS: [&str; 4] = ["kind", "by", "mode", "way"];
/// 裸文本兜底（`⟦wm:楼下便利店⟧`）允许的最大字符数
const BARE_MAX_CHARS: usize = 60;

/// 解析出来的一条位置指令。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Directive {
    /// 目的地名字（**名字**，不是坐标 —— 坐标由 [`super::move`] 去地图数据里找）
    pub to: String,
    /// 出行方式原文（未归一化；`move::MoveKind::parse` 负责认中文/英文别名）
    pub kind: Option<String>,
    /// 指令体原文（日志/调试用，不含标记本身）
    pub raw: String,
}

/// 解析一个指令体（`⟦` 与 `⟧` 之间的内容）。
///
/// 三级容错：严格 JSON → 手扫字符串字段（截断的 JSON 也能救）→ 裸地名。
/// 返回 `None` 表示"这不是一条可用指令"，调用方应当**丢弃**（不是吐回正文）。
pub fn parse(body: &str) -> Option<Directive> {
    let raw = body.trim();
    if raw.is_empty() {
        return None;
    }

    // ① 严格 JSON：`{"to":"便利店","kind":"walk"}`
    //
    // 合法的 JSON 只走这一条路（解析不出 `to` 就直接判死）：否则 `null`、`123`、
    // `[1,2]` 这些会被 ③ 的裸地名兜底当成地名，凭空开出一趟行程。
    if let Ok(v) = serde_json::from_str::<Value>(raw) {
        let (to, kind) = from_value(&v)?;
        return Some(Directive {
            to,
            kind,
            raw: raw.to_string(),
        });
    }

    // ② 残缺 JSON：手扫 `"to":"…"`（值没闭合就退回 ③）
    let to = scan_string_field(raw, &TO_KEYS);
    if let Some(to) = to {
        return Some(Directive {
            to,
            kind: scan_string_field(raw, &KIND_KEYS),
            raw: raw.to_string(),
        });
    }

    // ③ 裸地名：`⟦wm:楼下便利店⟧`。
    //    护栏：不能含 JSON 标点、不能太长 —— 否则一段正经正文会被误认成地名。
    if raw.chars().count() <= BARE_MAX_CHARS
        && !raw
            .chars()
            .any(|c| matches!(c, '{' | '}' | '"' | '\\' | '[' | ']'))
    {
        return Some(Directive {
            to: raw.to_string(),
            kind: None,
            raw: raw.to_string(),
        });
    }

    None
}

/// 从合法 JSON 里取 `(目的地, 出行方式)`。
fn from_value(v: &Value) -> Option<(String, Option<String>)> {
    let obj = v.as_object()?;
    let to = TO_KEYS
        .iter()
        .find_map(|k| obj.get(*k).and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let kind = KIND_KEYS
        .iter()
        .find_map(|k| obj.get(*k).and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Some((to, kind))
}

/// 手扫 `"键" : "值"`（不要求整体是合法 JSON）。
///
/// 用在"JSON 被截断/混了别的东西"的场景，例如：
/// `{"to":"楼下便利店","kind":"wal` —— 严格解析失败，但 `to` 是完整的。
/// 代价是不处理 `\uXXXX` 转义（合法 JSON 走不到这条路，见 [`parse`] ①）。
fn scan_string_field(src: &str, keys: &[&str]) -> Option<String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != '"' {
            i += 1;
            continue;
        }
        let Some((token, mut j)) = read_string(&chars, i) else {
            return None; // 引号没闭合，后面都不用看了
        };
        if keys.contains(&token.as_str()) {
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && chars[j] == ':' {
                j += 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if j < chars.len() && chars[j] == '"' {
                    if let Some((val, _)) = read_string(&chars, j) {
                        let val = val.trim().to_string();
                        if !val.is_empty() {
                            return Some(val);
                        }
                    }
                }
            }
        }
        i = j;
    }
    None
}

/// 从 `start`（指向开引号）读一个字符串，返回 `(内容, 结束后的下标)`。
/// 反斜杠按"吃掉一个字符"处理（够用：真指令里不会出现转义引号）。
fn read_string(chars: &[char], start: usize) -> Option<(String, usize)> {
    let mut s = String::new();
    let mut i = start + 1;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                i += 1;
                if i < chars.len() {
                    s.push(chars[i]);
                    i += 1;
                }
            }
            '"' => return Some((s, i + 1)),
            c => {
                s.push(c);
                i += 1;
            }
        }
    }
    None
}

/// 扫描状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// 普通正文
    Idle,
    /// 见过 `⟦`，正在比对 `wm:` 前缀（还没确认）
    Cand,
    /// 已确认 `⟦wm:`，正在攒 body 直到 `⟧`
    Body,
}

/// 流式剥离器：喂 chunk，拿可见正文；指令另存，流结束后统一取走。
///
/// `enabled = false` 时它就是**恒等函数**（世界模拟没开的红线，见模块注释）。
#[derive(Debug, Clone)]
pub struct Scanner {
    enabled: bool,
    state: State,
    /// 候选态攒下的字符（`⟦` + 已匹配的 `wm:` 前缀）
    cand: String,
    /// 已匹配的 PREFIX 字符数（0..=PREFIX.len()）
    matched: usize,
    /// body 状态攒下的内容
    body: String,
    directives: Vec<Directive>,
    /// 吞掉但没解析成功的指令条数（坏格式的可观测性，只记日志不影响流程）
    invalid: usize,
}

impl Scanner {
    /// `enabled` 传 `false` 就是"完全不参与"：与没有本模块时逐字节一致。
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            state: State::Idle,
            cand: String::new(),
            matched: 0,
            body: String::new(),
            directives: Vec::new(),
            invalid: 0,
        }
    }

    /// 关掉的扫描器（世界模拟没开 / 单测对照用）。
    pub fn disabled() -> Self {
        Self::new(false)
    }

    /// 已收齐的指令（按出现顺序）。
    pub fn directives(&self) -> &[Directive] {
        &self.directives
    }

    /// 取走指令（派发一次就够，取完即空）。
    pub fn take_directives(&mut self) -> Vec<Directive> {
        std::mem::take(&mut self.directives)
    }

    /// 最后一条指令 —— **派发用的是它**（模型最后的表态）。
    pub fn last(&self) -> Option<&Directive> {
        self.directives.last()
    }

    /// 被吞掉但没解析成功的条数。
    pub fn invalid_count(&self) -> usize {
        self.invalid
    }

    /// 喂一个流式 chunk，返回**可以进入正文**的那部分。
    ///
    /// 返回值借用入参：绝大多数 chunk 走快路径返回 `Cow::Borrowed`（零拷贝），
    /// 只有 chunk 里真的出现 `⟦` 才分配一个新字符串。
    pub fn push<'a>(&mut self, chunk: &'a str) -> Cow<'a, str> {
        // 快路径：关掉的扫描器，或本 chunk 没有标记且当前不在候选/body 态。
        // 这一行保证了"世界模拟没开时逐字节不变"。
        if !self.enabled || (self.state == State::Idle && !chunk.contains(OPEN)) {
            return Cow::Borrowed(chunk);
        }

        let mut out = String::with_capacity(chunk.len());
        let mut rest = chunk;
        while !rest.is_empty() {
            match self.state {
                State::Idle => match rest.find(OPEN) {
                    None => {
                        out.push_str(rest);
                        rest = "";
                    }
                    Some(i) => {
                        out.push_str(&rest[..i]);
                        self.cand.clear();
                        self.cand.push(OPEN);
                        self.matched = 0;
                        self.state = State::Cand;
                        rest = &rest[i + OPEN.len_utf8()..];
                    }
                },
                State::Cand => {
                    // 逐字符比对 PREFIX（全 ASCII）；不匹配就整段吐回正文
                    let mut take = 0usize;
                    let mut decided = false;
                    for (i, c) in rest.char_indices() {
                        if PREFIX.chars().nth(self.matched) == Some(c) {
                            self.cand.push(c);
                            self.matched += 1;
                            take = i + c.len_utf8();
                            if self.matched == PREFIX.chars().count() {
                                self.state = State::Body;
                                self.body.clear();
                                decided = true;
                                break;
                            }
                        } else {
                            out.push_str(&self.cand);
                            self.cand.clear();
                            self.matched = 0;
                            self.state = State::Idle;
                            take = 0; // 这个字符没消费，回到 Idle 重新按正文处理
                            decided = true;
                            break;
                        }
                    }
                    if !decided {
                        take = rest.len(); // chunk 用尽，仍在候选态（等下一个 chunk）
                    }
                    rest = &rest[take..];
                }
                State::Body => match rest.find(CLOSE) {
                    None => {
                        self.body.push_str(rest);
                        rest = "";
                        if self.body.chars().count() > MAX_BODY_CHARS {
                            // 太长还没闭合：判定"这其实是正文"，原文吐回（宁可露出残缺标记）
                            out.push_str(&self.cand);
                            out.push_str(&self.body);
                            self.cand.clear();
                            self.body.clear();
                            self.matched = 0;
                            self.state = State::Idle;
                            self.invalid += 1;
                            tracing::warn!(
                                "[world_map] 位置指令超过 {MAX_BODY_CHARS} 字符仍未闭合，按正文处理"
                            );
                        }
                    }
                    Some(i) => {
                        self.body.push_str(&rest[..i]);
                        rest = &rest[i + CLOSE.len_utf8()..];
                        let body = std::mem::take(&mut self.body);
                        self.cand.clear();
                        self.matched = 0;
                        self.state = State::Idle;
                        match parse(&body) {
                            Some(d) => {
                                tracing::info!("[world_map] 收到位置指令: to={} kind={:?}", d.to, d.kind);
                                self.directives.push(d);
                            }
                            None => {
                                self.invalid += 1;
                                // 只记前 80 字符，别把整段（可能是被误判的正文）灌进日志
                                let head: String = body.chars().take(80).collect();
                                tracing::warn!("[world_map] 位置指令无法解析，已丢弃: {head}");
                            }
                        }
                    }
                },
            }
        }
        Cow::Owned(out)
    }

    /// 流结束收口：返回**还应该补进正文**的尾巴。
    ///
    /// - 候选态（`⟦` / `⟦w` / `⟦wm`）：还没确认是指令 → 吐回（等价于"没有指令"）
    /// - body 态（`⟦wm:……` 没闭合，通常是流被截断）：**丢弃**，露出来就是残留
    pub fn finish(&mut self) -> String {
        if !self.enabled {
            return String::new();
        }
        let mut out = String::new();
        match self.state {
            State::Idle => {}
            State::Cand => out.push_str(&self.cand),
            State::Body => {
                self.invalid += 1;
                let head: String = self.body.chars().take(80).collect();
                tracing::warn!("[world_map] 位置指令在流结束时仍未闭合，已丢弃: {head}");
            }
        }
        self.cand.clear();
        self.body.clear();
        self.matched = 0;
        self.state = State::Idle;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 拼一条指令文本。
    fn d(body: &str) -> String {
        format!("{OPEN}{PREFIX}{body}{CLOSE}")
    }

    /// 一次喂完整串（模拟"一个 chunk 装下整条回复"）。
    fn feed(enabled: bool, text: &str) -> (String, Scanner) {
        let mut s = Scanner::new(enabled);
        let visible = s.push(text).to_string();
        let tail = s.finish();
        (format!("{visible}{tail}"), s)
    }

    /// 关掉时是恒等函数：各种奇形怪状的输入都逐字节不变。
    #[test]
    fn disabled_scanner_is_identity() {
        let cases = [
            "普通回复",
            "【开心】好呀",
            "⟦wm:{\"to\":\"便利店\"}⟧",
            "⟦没写完",
            "⟦wm:{\"to\":\"便利店\"",
            "⟦ ⟧ ⟦wm: ⟧",
            "",
        ];
        for c in cases {
            let (visible, s) = feed(false, c);
            assert_eq!(visible, c, "关了世界模拟必须逐字节一致: {c:?}");
            assert!(s.directives().is_empty());
            assert_eq!(s.invalid_count(), 0);
        }
    }

    /// 关掉时 `push` 必须走零拷贝分支。
    #[test]
    fn disabled_scanner_borrows_without_allocating() {
        let mut s = Scanner::disabled();
        let chunk = "⟦wm:{\"to\":\"便利店\"}⟧";
        assert!(matches!(s.push(chunk), Cow::Borrowed(_)));
    }

    /// 开着时：指令被摘掉，正文一字不动。
    #[test]
    fn enabled_strips_directive_and_keeps_text() {
        let text = format!("【开心】好呀{}", d("{\"to\":\"楼下便利店\",\"kind\":\"walk\"}"));
        let (visible, s) = feed(true, &text);
        assert_eq!(visible, "【开心】好呀");
        assert_eq!(s.directives().len(), 1);
        assert_eq!(s.last().unwrap().to, "楼下便利店");
        assert_eq!(s.last().unwrap().kind.as_deref(), Some("walk"));
    }

    /// 指令在中间、在开头都不吃正文。
    #[test]
    fn directive_anywhere_does_not_eat_text() {
        let mid = format!("前{}后", d("{\"to\":\"A\"}"));
        let (v1, s1) = feed(true, &mid);
        assert_eq!(v1, "前后");
        assert_eq!(s1.last().unwrap().to, "A");

        let head = format!("{}【开心】正文", d("{\"to\":\"B\"}"));
        let (v2, s2) = feed(true, &head);
        assert_eq!(v2, "【开心】正文");
        assert_eq!(s2.last().unwrap().to, "B");
    }

    /// 一条指令被切成多个 chunk（含标记本身被切开）也不漏。
    #[test]
    fn directive_split_across_chunks() {
        let mut s = Scanner::new(true);
        let mut visible = String::new();
        for part in ["【开心】去了", "⟦", "w", "m", ":{\"to\":\"咖啡", "馆\"}", "⟧", "，马上到"] {
            visible.push_str(&s.push(part));
        }
        visible.push_str(&s.finish());
        assert_eq!(visible, "【开心】去了，马上到");
        assert_eq!(s.last().unwrap().to, "咖啡馆");
    }

    /// 多条指令：全部收集，派发取最后一条。
    #[test]
    fn multiple_directives_last_wins() {
        let text = format!(
            "{}中间的正文{}",
            d("{\"to\":\"A\"}"),
            d("{\"to\":\"B\",\"kind\":\"bus\"}")
        );
        let (visible, s) = feed(true, &text);
        assert_eq!(visible, "中间的正文");
        assert_eq!(s.directives().len(), 2);
        assert_eq!(s.last().unwrap().to, "B");
        assert_eq!(s.last().unwrap().kind.as_deref(), Some("bus"));

        let mut s2 = s.clone();
        assert_eq!(s2.take_directives().len(), 2);
        assert!(s2.directives().is_empty());
    }

    /// 残缺：JSON 少个括号、值被截断 —— 能救就救（`to` 完整即可）。
    #[test]
    fn truncated_json_is_parsed_leniently() {
        let (v1, s1) = feed(true, &d("{\"to\":\"便利店\",\"kind\":\"wal"));
        assert_eq!(v1, "");
        assert_eq!(s1.last().unwrap().to, "便利店");
        assert_eq!(s1.last().unwrap().kind, None, "截断的 kind 不该瞎猜");

        let (v2, s2) = feed(true, &d("{\"to\":\"便利店\""));
        assert_eq!(v2, "");
        assert_eq!(s2.last().unwrap().to, "便利店");
    }

    /// 残缺：完全没法解析的 —— 吞掉、不派发、不算崩。
    #[test]
    fn unparseable_directive_is_swallowed_silently() {
        let cases = ["{}", "{\"kind\":\"walk\"}", "{\"to\":\"\"}", "{\"to\":\"   \"}", "   ", "{\"to\":123}"];
        for c in cases {
            let (visible, s) = feed(true, &format!("A{}B", d(c)));
            assert_eq!(visible, "AB", "坏指令不该漏进正文: {c:?}");
            assert!(s.directives().is_empty(), "不该派发: {c:?}");
            assert_eq!(s.invalid_count(), 1, "要计数: {c:?}");
        }
    }

    /// 裸地名兜底 + 护栏（正文不像地名就不能认）。
    #[test]
    fn bare_name_fallback_with_guard() {
        let (v1, s1) = feed(true, &d("楼下便利店"));
        assert_eq!(v1, "");
        assert_eq!(s1.last().unwrap().to, "楼下便利店");
        assert_eq!(s1.last().unwrap().kind, None);

        // 带 JSON 标点的长正文：不认，原文吐回
        let junk = format!("{}这是一段正文，不是地名", d("{\"nope\":1}"));
        let (v2, s2) = feed(true, &junk);
        assert!(v2.contains("这是一段正文"), "{v2}");
        assert!(s2.directives().is_empty());
        assert_eq!(s2.invalid_count(), 1);
    }

    /// 装饰性的单个 `⟦`（后面不是 wm:）不该吃掉后面的正文。
    #[test]
    fn stray_open_mark_is_text() {
        let (v1, s1) = feed(true, "⟦这是一句正文");
        assert_eq!(v1, "⟦这是一句正文");
        assert!(s1.directives().is_empty());
        assert_eq!(s1.invalid_count(), 0);

        // ⟦⟦ 连着来（第一个作废后第二个还能起头）
        let (v2, s2) = feed(true, &format!("⟦{}", d("{\"to\":\"便利店\"}")));
        assert_eq!(v2, "⟦");
        assert_eq!(s2.last().unwrap().to, "便利店");
    }

    /// 流结束时的挂起态：候选吐回、body 丢弃。
    #[test]
    fn finish_flushes_cand_and_drops_body() {
        // ⟦ / ⟦w / ⟦wm 还没确认是指令 → 当正文吐回（末尾可见字符数 ≤ 3）
        for tail in ["⟦", "⟦w", "⟦wm"] {
            let (visible, s) = feed(true, &format!("正文{tail}"));
            assert_eq!(visible, format!("正文{tail}"), "尾部 {tail:?} 应原样吐回");
            assert!(s.directives().is_empty());
            assert_eq!(s.invalid_count(), 0, "未确认的候选不该算坏指令");
        }
        // ⟦wm: 已确认但被截断 → 丢弃（露出来就是残留）
        let (v2, s2) = feed(true, "正文⟦wm:");
        assert_eq!(v2, "正文");
        assert_eq!(s2.invalid_count(), 1, "截断的指令要计数");
    }

    /// 超长未闭合：原文吐回，不吞正文。
    #[test]
    fn overlong_unclosed_body_is_flushed_back() {
        let long = "字".repeat(MAX_BODY_CHARS + 10);
        // 注意：这里**故意不带 ⟧**，模拟"模型写了 ⟦wm: 之后接着写正文"
        let text = format!("前{OPEN}{PREFIX}{long}");
        let (visible, s) = feed(true, &text);
        let head: String = visible.chars().take(12).collect();
        assert!(visible.starts_with(&format!("前{OPEN}{PREFIX}")), "残缺标记要吐回: {head}");
        assert!(
            visible.chars().count() >= MAX_BODY_CHARS,
            "正文不能被吃掉: {}",
            visible.chars().count()
        );
        assert!(s.directives().is_empty());
        assert_eq!(s.invalid_count(), 1);
    }

    /// 嵌套：JSON 里再出现标记 —— 不许 panic，后面的正文要保住。
    #[test]
    fn nested_marks_do_not_panic() {
        let text = format!("A{}B", d("{\"to\":\"a\",\"note\":\"⟦x⟧\"}"));
        let (visible, s) = feed(true, &text);
        assert!(visible.starts_with('A'), "{visible}");
        assert!(visible.ends_with('B'), "指令之后的正文要保住: {visible}");
        // 首个 ⟧ 收口 → `to` 能救出来（残留的 `"}` 属畸形输入，见模块注释）
        assert_eq!(s.last().unwrap().to, "a");

        // 双层标记
        let (v2, _s2) = feed(true, &format!("{}{}", d("{\"to\":\"A\"}"), d("{\"to\":\"B\"}")));
        assert_eq!(v2, "");
    }

    /// 解析器单测：别名键、空白、大小写、空值。
    #[test]
    fn parse_accepts_aliases_and_rejects_junk() {
        assert_eq!(parse("{\"target\":\"A\"}").unwrap().to, "A");
        assert_eq!(parse("{\"destination\":\"A\",\"mode\":\"bike\"}").unwrap().kind.as_deref(), Some("bike"));
        assert_eq!(parse("  {\"to\" : \" A \" }  ").unwrap().to, "A");
        assert_eq!(parse("{\"place\":\"A\",\"way\":\"bus\"}").unwrap().kind.as_deref(), Some("bus"));
        // to 在 kind 后面也能扫到
        assert_eq!(parse("{\"kind\":\"walk\",\"to\":\"A\"}").unwrap().to, "A");
        assert!(parse("").is_none());
        assert!(parse("{}").is_none());
        assert!(parse("null").is_none());
        assert!(parse("[1,2]").is_none());
        assert!(parse("{\"to\":null}").is_none());
        assert!(parse("{\"to\":[\"A\"]}").is_none());
    }

    /// 转义引号/反斜杠：别在扫描时 panic，也别切错边界。
    #[test]
    fn escapes_and_unicode_boundaries() {
        assert_eq!(parse("{\"to\":\"A\\\"B\"}").unwrap().to, "A\"B");
        assert_eq!(parse("{\"to\":\"咖啡\\n馆\"}").unwrap().to, "咖啡\n馆");
        let text = format!("中文{}emoji🎈", d("{\"to\":\"星巴克(东山口店)\"}"));
        let (visible, s) = feed(true, &text);
        assert_eq!(visible, "中文emoji🎈");
        assert_eq!(s.last().unwrap().to, "星巴克(东山口店)");
    }

    /// 大量随机切分点：不管怎么切，可见正文与指令都必须和整串一致。
    #[test]
    fn arbitrary_chunk_splits_are_consistent() {
        let text = format!(
            "【开心】我出门了{}顺路买瓶水{}好",
            d("{\"to\":\"楼下便利店\",\"kind\":\"walk\"}"),
            d("{\"to\":\"公园\"}")
        );
        let (want_visible, want) = feed(true, &text);
        assert_eq!(want_visible, "【开心】我出门了顺路买瓶水好");
        assert_eq!(want.directives().len(), 2);

        for step in 1..=6 {
            let mut s = Scanner::new(true);
            let mut visible = String::new();
            let chars: Vec<char> = text.chars().collect();
            for piece in chars.chunks(step) {
                let part: String = piece.iter().collect();
                visible.push_str(&s.push(&part));
            }
            visible.push_str(&s.finish());
            assert_eq!(visible, want_visible, "步长 {step} 的切分结果不一致");
            assert_eq!(s.directives(), want.directives(), "步长 {step} 的指令不一致");
        }
    }
}
