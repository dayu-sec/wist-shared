//! 记录边界界定：把**行流**切成**记录**。
//!
//! 一行 ≠ 一条记录。同一行文本，可能是一条记录的开头，也可能是上一条的续行。所以
//! 处理多行日志的本质不是"识别多行"，而是**先定「记录边界」** —— 边界不定，后面一切
//! （解析、去重、计数）都建在流沙上。
//!
//! ## 判据是「边界信号」，不是「形态」
//!
//! 逐行只需回答一句话：**这一行开不开新记录？**可靠依据是这行**自身**有没有边界信号：
//!
//! - [`Boundary::Starts`]（**开始型**）：这行开一条新记录 —— 适用于边界信号落在**记录里面**
//!   的格式（如行首时间戳，它是首行的一部分）；
//! - [`Boundary::Ends`]（**结束型**）：上一条到此结束 —— 适用于边界信号落在**记录之间**的格式
//!   （如空行，它不属于任何一条）；
//! - [`Boundary::Neither`]：续行。
//!
//! 缩进、空行这些**形态**只是边界信号的**代理**。代理在某些文件上成立（续行恰好都缩进），
//! 换个文件就错（空行分隔的记录、顶格闭合的 `}`）。用形态当判据，等于把"格式知识"降级成
//! "排版巧合"。
//!
//! 本模块**不猜**边界信号长什么样 —— 那是格式知识，只有策展知道。调用方把它作为函数传进来
//! （[`indented`] 是常见读法的现成实现）。
//!
//! ## 算法的四条要点
//!
//! 1. **边界是倒推出来的**：一条记录要等到**下一个边界信号**到达才确定结束 —— 所以最后一条
//!    永远悬着，必须有 [`Delimiter::flush`]（空闲 / 来源变化 / 停机）强制封口；
//! 2. **起点不许发半条**：从文件中间落地时，那截内容没有头。宁可丢半条，不可发半条 ——
//!    半条记录长得像完整记录，会一路骗过下游的字段解析，而且查不出来；
//! 3. **有状态就有边界**：累积不能无限增长（[`Limits`]），到限**封口并标记**，不静默截断；
//! 4. **丢弃要可数**：被丢的是"没有头"的部分。计数是**累计**的 —— 要判"连续丢了很久"
//!    （即边界信号可能配错），由调用方在每次起头后取快照做差。
//!
//! ## 无 IO、无时间、无配置
//!
//! 时间是调用方的事（该 [`Delimiter::flush`] 时调），所以它既能用在采集侧（从文件尾读），
//! 也能用在从字节流任意位置读的地方。同一条记录**不会超过** [`Limits`]。

/// 一行在**记录边界**上的角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    /// 开始信号：这一行开一条新记录。
    Starts,
    /// 结束信号：上一条到此结束，之后重新开始在下一行。这一行本身**不是内容**。
    Ends,
    /// 都不是 —— 这一行是上一条的续行。
    Neither,
}

/// 常见读法：行首缩进/制表符是上一条的续行（[`Boundary`] 的现成实现）。
///
/// 这是"边界信号 ≈ 缩进"的**代理读法**：适用于"续行都缩进、非续行都带锚"的文件
/// （实测 `/var/log/install.log` 上误判 6/338592）。
///
/// 语义严格、无例外：**只有**行首是空格或制表符才算续行，其余（含空行）一律算开始信号。
/// 行内的空行会因此被当成一条空记录 —— 那不是本函数的意外，而是代理读法本身的局限：
/// 想要别的行为，自己写一个 [`Boundary`] 判定函数即可。
pub fn indented(line: &str) -> Boundary {
    match line.as_bytes().first() {
        Some(b' ' | b'\t') => Boundary::Neither,
        _ => Boundary::Starts,
    }
}

/// 送进界定器的一行。
///
/// `text` 原样带上（是否含行尾换行由调用方决定），`start_offset`/`end_offset` 是它在
/// 来源里的字节区间 —— 界定器只搬运、不改写，所以调用方拿到的记录区间能与来源对账。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line<'a> {
    pub text: &'a str,
    pub start_offset: u64,
    pub end_offset: u64,
}

/// 一条记录**怎么被封口的** —— 下游据此判它可不可信。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Completion {
    /// 下一个边界信号到来 —— 记录的边界是**确定**的，内容完整。
    Boundary,
    /// 到期封口（空闲 / 来源变化 / 停机）—— 内容完整，但边界是推断的。
    Deadline,
    /// 到上限被截 —— **内容可能不全**，下游要当心（解析失败不该归罪于格式）。
    Oversized,
}

/// 界定出来的一条记录。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Record {
    /// 记录正文：由各行的 `text` 原样拼接（不插入、不删改任何字符）。
    pub body: String,
    /// 本记录在来源里的字节区间：首行的 `start_offset` 到末行的 `end_offset`。
    pub start_offset: u64,
    pub end_offset: u64,
    /// 拼成这条记录的行数。
    pub lines: usize,
    /// 只有从 [`Delimiter::push`]/[`Delimiter::flush`] 交出来的记录，这个值才有意义。
    pub completion: Completion,
}

/// 累积上限：先到者胜。
///
/// 同一条记录**不会超过**这两个数（唯一例外见 [`Delimiter::push`]：单行本身超限时，
/// 那条单行不被切开 —— 单行的上限归读取器管）。上限是"某个块永不结束"的兜底：
/// 一个没有边界信号的缩进块会一直涨，不给上限就等于把内存交给日志内容决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Limits {
    pub max_lines: usize,
    pub max_bytes: usize,
}

impl Limits {
    /// 两个值都被抬到至少 1：0 会表示"连一行都不许"，那不是一条能用的规则。
    pub fn new(max_lines: usize, max_bytes: usize) -> Self {
        Self {
            max_lines: max_lines.max(1),
            max_bytes: max_bytes.max(1),
        }
    }
}

/// 起点状态：取决于边界信号是**开始型**还是**结束型**。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Start {
    /// 等第一个**开始信号**才起头 —— 配**开始型**格式（如行首时间戳）。
    ///
    /// 这样从记录中间落地时，那一截不会被当成记录发出（不发半条）。
    WaitForStart,
    /// 没在累积时，下一行就起头 —— 配**结束型**格式（如空行分隔）。
    ///
    /// 这种格式没有可辨的开始信号，所以无法识别"落地在记录中间"：第一条可能无头。
    /// 这是格式本身的代价，不是界定器能补救的。
    Collect,
}

/// 记录边界界定器：把行流切成记录。
///
/// # 序列化
///
/// 可以整体存盘、跨进程接着算（`agentd` 的 checkpoint 里就存着它）。
/// 存的是**完整状态含策略**（[`Limits`] / [`Start`] 一起落）—— 因为它们描述的是
/// "这条未封口的记录是按什么规则在攒"，属于状态而不只是配置，且会随着这条记录结束而自然失效。
/// 想改用当前配置接续也可以：新建一个，再把 [`Delimiter::pending`] 拿走的那条搬过去。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Delimiter {
    limits: Limits,
    start: Start,
    /// 正在累积的那条；`None` = 手里没有未封口的记录。
    buffer: Option<Record>,
    dropped_lines: usize,
    dropped_bytes: usize,
    emitted: usize,
}

impl Delimiter {
    pub fn new(limits: Limits, start: Start) -> Self {
        Self {
            limits,
            start,
            buffer: None,
            dropped_lines: 0,
            dropped_bytes: 0,
            emitted: 0,
        }
    }

    /// 用一份未封口的记录恢复（跨进程接着同一条）。
    ///
    /// \(record.completion\) 被忽略 —— 它只在封口那一刻才有意义。计数器从零起：
    /// 它们是"本次运行"的诊断，不跟着那条记录走。
    pub fn resume(limits: Limits, start: Start, record: Record) -> Self {
        Self {
            limits,
            start,
            buffer: Some(record),
            dropped_lines: 0,
            dropped_bytes: 0,
            emitted: 0,
        }
    }

    /// 送一行进来。若这一行**封上了**上一条记录，返回它。
    ///
    /// 一次至多封上一条：一条记录只在**下一个边界信号**处结束。
    ///
    /// 到上限时：把已累积的那条以 [`Completion::Oversized`] 封口，**并丢掉这一行**
    /// （它此刻已经没有头了，与"从记录中间落地"同一处置）。所以产出的记录不会超过上限，
    /// 唯一例外是本就是**单行**的记录 —— 单行是读取器该管的边界，界定器不替它切半条。
    pub fn push(&mut self, line: Line<'_>, signal: impl Fn(&str) -> Boundary) -> Option<Record> {
        match signal(line.text) {
            Boundary::Starts => {
                let sealed = self.seal(Completion::Boundary);
                self.begin(line);
                sealed
            }
            // 结束信号是**间隔**不是内容：封上当前这条即可。它不算"丢弃" ——
            // 它是一条正常的边界，不该污染"信号可能配错"的诊断计数。
            Boundary::Ends => self.seal(Completion::Boundary),
            Boundary::Neither => {
                // 先算结论再动状态：把"借用 buffer 判断"与"改 buffer / 封口"分开，
                // 否则封口与追加不能同时出现。
                let verdict = match self.buffer.as_ref() {
                    Some(record) => Some(over_limit(self.limits, record, line.text)),
                    None => None,
                };
                match verdict {
                    Some(true) => {
                        let sealed = self.seal(Completion::Oversized);
                        self.drop_line(line);
                        sealed
                    }
                    Some(false) => {
                        let record = self.buffer.as_mut().expect("accumulating");
                        record.body.push_str(line.text);
                        record.end_offset = line.end_offset;
                        record.lines += 1;
                        None
                    }
                    None if self.start == Start::Collect => {
                        self.begin(line);
                        None
                    }
                    // 落在记录中间、又没有头：丢。宁可丢半条，不可发半条。
                    None => {
                        self.drop_line(line);
                        None
                    }
                }
            }
        }
    }

    /// 到期封口：这一段不会再有续行了（空闲到点 / 来源变化 / 停机）。
    ///
    /// 不封口的话，最后一条记录会一直悬着 —— 低频文件上就是"日志采了但看不到"。
    pub fn flush(&mut self) -> Option<Record> {
        self.seal(Completion::Deadline)
    }

    /// 手里有没有还没封口的记录。
    pub fn is_accumulating(&self) -> bool {
        self.buffer.is_some()
    }

    /// 未封口那条记录的只读视图（`None` = 手里没有）。
    ///
    /// 它的 `completion` 还没定，别当真 —— 要的是"这条攒到哪了"（正文、区间、行数）。
    pub fn pending(&self) -> Option<&Record> {
        self.buffer.as_ref()
    }

    /// 交出未封口的那条记录（`None` = 手里没有），并清空界定器。
    ///
    /// 给"把状态存下来、下次接着算"的调用方用 —— 与 [`Delimiter::pending`] 相比
    /// 它**拿走**了内容，省掉一次正文本的拷贝。
    pub fn into_pending(mut self) -> Option<Record> {
        self.buffer.take()
    }

    /// 累计丢弃的**行数**：**没有头**的续行（落在记录中间落地，或被上限挡在门外）。
    ///
    /// 它一直涨却从不产出记录，说明边界信号可能配错了 —— 那不该表现成"这个文件没日志"。
    pub fn dropped_lines(&self) -> usize {
        self.dropped_lines
    }

    /// 累计丢弃的**正文**字节数（按各行的 `text` 长度算，不是来源区间宽度）。
    ///
    /// 计数是累计的：要判"**连续**丢了很久"，由调用方在每次起头后取快照做差。
    pub fn dropped_bytes(&self) -> usize {
        self.dropped_bytes
    }

    /// 已产出的记录数。
    pub fn emitted(&self) -> usize {
        self.emitted
    }

    /// 起一条新记录。`completion` 只是占位 —— 所有对外交出的记录都在 [`Delimiter::seal`]
    /// 里被改写，这个值不会流出去。
    fn begin(&mut self, line: Line<'_>) {
        self.buffer = Some(Record {
            body: line.text.to_string(),
            start_offset: line.start_offset,
            end_offset: line.end_offset,
            lines: 1,
            completion: Completion::Boundary,
        });
    }

    fn drop_line(&mut self, line: Line<'_>) {
        self.dropped_lines += 1;
        self.dropped_bytes += line.text.len();
    }

    fn seal(&mut self, completion: Completion) -> Option<Record> {
        let mut record = self.buffer.take()?;
        record.completion = completion;
        self.emitted += 1;
        Some(record)
    }
}

/// 再加这一行会不会越过上限（先到者胜）。**含端点**：正好等于上限是允许的。
fn over_limit(limits: Limits, record: &Record, incoming: &str) -> bool {
    record.lines + 1 > limits.max_lines || record.body.len() + incoming.len() > limits.max_bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: Limits = Limits {
        max_lines: 1000,
        max_bytes: 1 << 20,
    };

    fn anchored() -> Delimiter {
        Delimiter::new(LIMITS, Start::WaitForStart)
    }

    /// 带行尾换行的行（与文件读取器的口径一致）。
    fn line(text: &str, start: u64) -> Line<'_> {
        Line {
            text,
            start_offset: start,
            end_offset: start + text.len() as u64,
        }
    }

    /// 开始型信号：记录开头的锚是 `2026-09-23 20:24:24+08 ...`。
    fn anchor(line: &str) -> Boundary {
        if line.starts_with("20") && line.contains("+08 ") {
            Boundary::Starts
        } else {
            Boundary::Neither
        }
    }

    /// `install.log` 的真实形状：两种时间戳锚 + 缩进续行。
    fn install_log_anchor(line: &str) -> Boundary {
        let iso = line.starts_with("20") && line.contains("+08 ");
        let bsd = line.starts_with("Jul ") || line.starts_with("Aug ");
        if iso || bsd {
            Boundary::Starts
        } else {
            Boundary::Neither
        }
    }

    /// 空行是间隔（结束型）的信号。
    fn blank_separated(line: &str) -> Boundary {
        if line.trim().is_empty() {
            Boundary::Ends
        } else {
            Boundary::Neither
        }
    }

    /// 逐行喂进去，返回所有记录。偏移按输入累加，便于断言区间。
    fn run(delimiter: &mut Delimiter, lines: &[&str]) -> Vec<Record> {
        let mut offset = 0;
        let mut out = Vec::new();
        for text in lines {
            if let Some(record) = delimiter.push(line(text, offset), anchor) {
                out.push(record);
            }
            offset += text.len() as u64;
        }
        out
    }

    /// 把行喂进去再到期封口，返回全部记录。
    fn run_and_flush(delimiter: &mut Delimiter, lines: &[&str]) -> Vec<Record> {
        let mut records = run(delimiter, lines);
        records.extend(delimiter.flush());
        records
    }

    // ── 基本形状 ──────────────────────────────────────────────────────────────

    #[test]
    fn the_next_anchor_seals_the_previous_so_the_last_one_stays_open() {
        let mut delimiter = anchored();
        let out = run(
            &mut delimiter,
            &[
                "2026-09-23 20:24:24+08 host a: one\n",
                "\tcont\n",
                "2026-09-23 20:24:25+08 host a: two\n",
            ],
        );
        // 只有第一条被封上：第二条要等**下一个**锚。
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].body, "2026-09-23 20:24:24+08 host a: one\n\tcont\n");
        assert_eq!(out[0].lines, 2);
        assert_eq!(out[0].completion, Completion::Boundary);
        assert_eq!(out[0].start_offset, 0);
        assert_eq!(out[0].end_offset, 41);
        assert!(delimiter.is_accumulating());

        // 那条悬着的靠到期封口收尾。
        let tail = delimiter.flush().expect("flush");
        assert_eq!(tail.body, "2026-09-23 20:24:25+08 host a: two\n");
        assert_eq!(tail.completion, Completion::Deadline);
        assert!(!delimiter.is_accumulating());
    }

    #[test]
    fn adjacent_anchors_are_adjacent_single_line_records() {
        // 相邻两条锚之间没有续行：各自就是一条一行的记录。
        let mut delimiter = anchored();
        let records = run_and_flush(
            &mut delimiter,
            &[
                "2026-09-23 20:24:24+08 host a: one\n",
                "2026-09-23 20:24:25+08 host a: two\n",
                "2026-09-23 20:24:26+08 host a: three\n",
            ],
        );
        assert_eq!(records.len(), 3);
        assert!(records.iter().all(|record| record.lines == 1));
        assert_eq!(
            records
                .iter()
                .map(|record| record.body.split_once(": ").expect("body").1)
                .collect::<Vec<_>>(),
            vec!["one\n", "two\n", "three\n"]
        );
        // 相邻单行记录首尾相接，没有洞。
        assert_eq!(records[0].end_offset, records[1].start_offset);
    }

    #[test]
    fn a_start_signal_then_an_end_signal_yields_a_single_line_record() {
        // 两种信号混用：行首锚开记录，空行也可作终止符。
        let mut delimiter = anchored();
        let signal = |line: &str| match anchor(line) {
            Boundary::Starts => Boundary::Starts,
            _ if line.trim().is_empty() => Boundary::Ends,
            _ => Boundary::Neither,
        };
        let first = delimiter
            .push(line("2026-09-23 20:24:24+08 host a: one\n", 0), signal)
            .is_none();
        assert!(first);
        let sealed = delimiter
            .push(line("\n", 37), signal)
            .expect("空行把上一条封上");
        assert_eq!(sealed.body, "2026-09-23 20:24:24+08 host a: one\n");
        assert_eq!(sealed.lines, 1);
        assert_eq!(sealed.completion, Completion::Boundary);
        assert!(!delimiter.is_accumulating());
        // 空行是间隔不是内容：既不进正文，也不算丢弃。
        assert_eq!(delimiter.dropped_lines(), 0);
    }

    #[test]
    fn an_end_signal_with_nothing_open_is_neither_a_record_nor_a_drop() {
        // 连着两个空行（或文件以空行开头）：没有任何未封口的记录可断。
        let mut delimiter = Delimiter::new(LIMITS, Start::Collect);
        assert!(delimiter.push(line("\n", 0), blank_separated).is_none());
        assert!(delimiter.push(line("   \n", 1), blank_separated).is_none());
        assert_eq!(delimiter.emitted(), 0);
        assert_eq!(delimiter.dropped_lines(), 0, "空行是边界，不是丢弃");
        assert!(!delimiter.is_accumulating());
    }

    #[test]
    fn an_empty_line_under_the_indented_reader_becomes_a_record_of_nothing() {
        // `indented` 语义严格：空行不是续行，它算开始信号 —— 结果是一条正文为空的记录。
        // 这不是意外，是代理读法本身的局限（要别的行为就自己写判定函数）。
        let mut delimiter = anchored();
        assert!(delimiter.push(line("\n", 0), indented).is_none());
        let sealed = delimiter.flush().expect("flush");
        assert_eq!(sealed.body, "\n");
        assert_eq!(sealed.lines, 1);
        assert_eq!(delimiter.dropped_lines(), 0);
    }

    // ── 起点与"不发半条" ──────────────────────────────────────────────────────

    #[test]
    fn an_indented_first_line_lands_mid_record_and_is_dropped() {
        // 从记录中间落地：那一截没有头。宁可丢半条，不可发半条。
        let mut delimiter = anchored();
        let out = run(
            &mut delimiter,
            &[
                "\tcont of an earlier record\n",
                "\tmore of it\n",
                "2026-09-23 20:24:24+08 host a: real\n",
            ],
        );
        assert!(out.is_empty());
        assert_eq!(delimiter.dropped_lines(), 2);
        assert_eq!(
            delimiter.dropped_bytes(),
            "\tcont of an earlier record\n".len() + "\tmore of it\n".len()
        );
        assert_eq!(
            delimiter.flush().expect("flush").body,
            "2026-09-23 20:24:24+08 host a: real\n"
        );
    }

    #[test]
    fn nothing_ever_arriving_never_becomes_a_record() {
        // 什么都没吃到（或只吃到没有头的碎片）就封口：不该凭空产出记录。
        for start in [Start::WaitForStart, Start::Collect] {
            let mut delimiter = Delimiter::new(LIMITS, start);
            assert!(delimiter.flush().is_none());
            assert_eq!(delimiter.emitted(), 0);
        }
        let mut delimiter = anchored();
        run(&mut delimiter, &["\tno head\n"]);
        assert!(delimiter.flush().is_none());
        assert_eq!(delimiter.dropped_lines(), 1);
    }

    #[test]
    fn a_record_spanning_several_batches_is_emitted_exactly_once() {
        // 一次读到的行不构成完整记录：状态跨批次活着，记录只产出一次（不产生半条）。
        let mut delimiter = anchored();
        assert!(run(&mut delimiter, &["2026-09-23 20:24:24+08 host a: one\n"]).is_empty());
        assert!(run(&mut delimiter, &["\tcont A\n"]).is_empty());
        assert!(run(&mut delimiter, &["\tcont B\n"]).is_empty());
        let out = run(&mut delimiter, &["2026-09-23 20:24:25+08 host a: two\n"]);
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].body,
            "2026-09-23 20:24:24+08 host a: one\n\tcont A\n\tcont B\n"
        );
        assert_eq!(out[0].lines, 3);
    }

    #[test]
    fn a_collecting_start_drops_nothing_up_front() {
        // 结束型格式从第一行就起头：开头那截不是"落在记录中间"，不该被丢。
        let mut delimiter = Delimiter::new(LIMITS, Start::Collect);
        let mut records = Vec::new();
        let mut offset = 0;
        for text in ["first\n", "second\n"] {
            if let Some(record) = delimiter.push(line(text, offset), blank_separated) {
                records.push(record);
            }
            offset += text.len() as u64;
        }
        records.extend(delimiter.flush());
        assert_eq!(delimiter.dropped_lines(), 0);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].body, "first\nsecond\n");
        assert_eq!(records[0].start_offset, 0);
        assert_eq!(records[0].end_offset, 13);
    }

    #[test]
    fn a_start_signal_also_works_in_a_collecting_format() {
        // 结束型起点不排斥开始型信号：两者可以混用（锚开一条、空行也可断一条）。
        let mut delimiter = Delimiter::new(LIMITS, Start::Collect);
        let signal = |line: &str| match anchor(line) {
            Boundary::Starts => Boundary::Starts,
            _ if line.trim().is_empty() => Boundary::Ends,
            _ => Boundary::Neither,
        };
        let mut records = Vec::new();
        let mut offset = 0;
        for text in [
            "junk without an anchor\n",
            "2026-09-23 20:24:24+08 host a: one\n",
            "\n",
            "2026-09-23 20:24:25+08 host a: two\n",
        ] {
            if let Some(record) = delimiter.push(line(text, offset), signal) {
                records.push(record);
            }
            offset += text.len() as u64;
        }
        records.extend(delimiter.flush());
        assert_eq!(
            records
                .iter()
                .map(|record| record.body.as_str())
                .collect::<Vec<_>>(),
            vec![
                "junk without an anchor\n",
                "2026-09-23 20:24:24+08 host a: one\n",
                "2026-09-23 20:24:25+08 host a: two\n",
            ]
        );
        assert_eq!(delimiter.dropped_lines(), 0);
    }

    // ── 上限 ─────────────────────────────────────────────────────────────────

    #[test]
    fn the_line_limit_is_inclusive_and_the_next_line_is_refused() {
        // 上限**含端点**：max_lines = 3 就该允许 3 行，第 4 行才越限。
        let mut delimiter = Delimiter::new(Limits::new(3, 1 << 20), Start::WaitForStart);
        let out = run(
            &mut delimiter,
            &[
                "2026-09-23 20:24:24+08 host a: one\n",
                "\tcont 1\n",
                "\tcont 2\n",
                "\tcont 3\n",
                "2026-09-23 20:24:25+08 host a: two\n",
            ],
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].lines, 3, "正好等于上限要放行");
        assert_eq!(out[0].completion, Completion::Oversized);
        assert_eq!(delimiter.dropped_lines(), 1);
    }

    #[test]
    fn the_byte_limit_is_inclusive() {
        let one = "2026-09-23 20:24:24+08 host a: one\n";
        let cont = "\tcont\n";
        // 正好等于两行之和：第二行要放行；第三行才越限。
        let mut delimiter = Delimiter::new(
            Limits::new(1000, one.len() + cont.len()),
            Start::WaitForStart,
        );
        assert!(delimiter.push(line(one, 0), anchor).is_none());
        assert!(
            delimiter
                .push(line(cont, one.len() as u64), anchor)
                .is_none(),
            "正好等于上限要放行"
        );
        let sealed = delimiter
            .push(line(cont, (one.len() + cont.len()) as u64), anchor)
            .expect("第三行越限");
        assert_eq!(sealed.completion, Completion::Oversized);
        assert_eq!(sealed.lines, 2);
        assert_eq!(sealed.body.len(), one.len() + cont.len());
    }

    #[test]
    fn an_oversized_record_is_sealed_and_marked_and_the_rest_is_dropped() {
        // 一个永不结束的块：到限就封口 + 标记；越限的行不再粘进任何记录。
        let mut delimiter = Delimiter::new(Limits::new(3, 4096), Start::WaitForStart);
        let out = run(
            &mut delimiter,
            &[
                "2026-09-23 20:24:24+08 host a: one\n",
                "\tcont 1\n",
                "\tcont 2\n",
                "\tcont 3\n",
                "\tcont 4\n",
                "2026-09-23 20:24:25+08 host a: two\n",
            ],
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].completion, Completion::Oversized);
        assert_eq!(
            out[0].body,
            "2026-09-23 20:24:24+08 host a: one\n\tcont 1\n\tcont 2\n"
        );
        // 越限的两行都没有头了 → 丢弃，且不粘进下一条。
        assert_eq!(delimiter.dropped_lines(), 2);
        assert_eq!(
            delimiter.flush().expect("flush").body,
            "2026-09-23 20:24:25+08 host a: two\n"
        );
    }

    #[test]
    fn a_single_line_over_the_limit_is_not_cut_in_half() {
        // 上限约束的是**累积**，不是单行。单行的上限归读取器管 ——
        // 界定器不替它把一行切成半条（半条记录会一路骗过下游解析）。
        let mut delimiter = Delimiter::new(Limits::new(10, 8), Start::WaitForStart);
        let long = "2026-09-23 20:24:24+08 host a: a very long line\n";
        assert!(
            delimiter.push(line(long, 0), anchor).is_none(),
            "单行本身就超限：先收下，不假装能截"
        );
        let sealed = delimiter
            .push(line("\tcont\n", long.len() as u64), anchor)
            .expect("第二条续行到限，把上一条封上");
        assert_eq!(sealed.completion, Completion::Oversized);
        assert_eq!(sealed.body, long, "单行原样保留，没被截");
        assert_eq!(sealed.lines, 1);
        assert_eq!(delimiter.dropped_lines(), 1);
    }

    #[test]
    fn after_an_oversized_seal_a_collecting_format_restarts_on_the_next_line() {
        // 结束型格式没有锚可以重新对齐：越限之后剩下的续行只能起一条**无头**记录。
        // 这是 `Start::Collect` 已经声明的代价（它同样无法识别"落地在记录中间"），
        // 不是界定器能补救的 —— 钉住它，免得日后被当成新 bug 改坏。
        let mut delimiter = Delimiter::new(Limits::new(2, 1 << 20), Start::Collect);
        let mut records = Vec::new();
        let mut offset = 0;
        for text in ["r1 a\n", "r1 b\n", "r1 c\n", "r1 d\n", "\n", "r2\n"] {
            if let Some(record) = delimiter.push(line(text, offset), blank_separated) {
                records.push(record);
            }
            offset += text.len() as u64;
        }
        records.extend(delimiter.flush());
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].completion, Completion::Oversized);
        assert_eq!(records[0].body, "r1 a\nr1 b\n");
        assert_eq!(records[1].body, "r1 d\n", "无头的那截");
        assert_eq!(records[2].body, "r2\n");
        assert_eq!(delimiter.dropped_lines(), 1);
    }

    // ── 不变量 ────────────────────────────────────────────────────────────────

    #[test]
    fn a_delimiter_survives_a_round_trip_through_json() {
        // 跨进程接着算：整个界定器存盘再读回来，未封口的记录不许变形，计数也不该被清零
        // （否则重启会把"丢了多少"抹掉，而那正是判断信号配错的依据）。
        let junk = "\tno head\n";
        let one = "2026-09-23 20:24:24+08 host a: one\n";
        let cont = "\tcont A\n";
        let mut delimiter = anchored();
        // 头一段没有锚 → 丢弃（记数）。
        assert!(delimiter.push(line(junk, 0), anchor).is_none());
        // 再开一条未封口的，等着跨进程继续。
        let base = junk.len() as u64;
        assert!(delimiter.push(line(one, base), anchor).is_none());
        assert!(
            delimiter
                .push(line(cont, base + one.len() as u64), anchor)
                .is_none()
        );
        assert_eq!(delimiter.dropped_lines(), 1);

        let json = serde_json::to_string(&delimiter).expect("serialize");
        let mut resumed: Delimiter = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(
            resumed.pending().expect("pending").body,
            format!("{one}{cont}")
        );
        assert_eq!(resumed.dropped_lines(), 1);
        assert_eq!(resumed.dropped_bytes(), junk.len());

        // 接着喂：封口的结果与"从没断过"完全一样。
        let next = base + one.len() as u64 + cont.len() as u64;
        let sealed = resumed
            .push(line("2026-09-23 20:24:25+08 host a: two\n", next), anchor)
            .expect("sealed");
        assert_eq!(sealed.body, format!("{one}{cont}"));
        assert_eq!(sealed.lines, 2);
        assert_eq!(sealed.completion, Completion::Boundary);
        assert_eq!(sealed.start_offset, base);
        assert_eq!(sealed.end_offset, next);
        assert_eq!(resumed.emitted(), 1);
    }

    #[test]
    fn offsets_tile_the_input_without_gaps_or_overlap() {
        // 记录边界必须完整覆盖输入：不能丢内容，也不能让同一段字节落在两条里。
        let mut delimiter = anchored();
        let lines = [
            "junk\n",
            "2026-09-23 20:24:24+08 host a: one\n",
            "\tcont\n",
            "2026-09-23 20:24:25+08 host a: two\n",
            "2026-09-23 20:24:26+08 host a: three\n",
        ];
        let records = run_and_flush(&mut delimiter, &lines);

        // 被丢弃的是开头那截（已知），其余必须首尾相接。
        assert_eq!(records[0].start_offset, "junk\n".len() as u64);
        assert_eq!(delimiter.dropped_bytes(), "junk\n".len());
        for pair in records.windows(2) {
            assert_eq!(
                pair[0].end_offset, pair[1].start_offset,
                "记录之间不许有洞或重叠：{pair:?}"
            );
        }
        let total: u64 = lines.iter().map(|text| text.len() as u64).sum();
        assert_eq!(records.last().expect("records").end_offset, total);
    }

    #[test]
    fn records_come_out_in_order() {
        let mut delimiter = anchored();
        let records = run_and_flush(
            &mut delimiter,
            &[
                "2026-09-23 20:24:24+08 host a: one\n",
                "2026-09-23 20:24:25+08 host a: two\n",
                "2026-09-23 20:24:26+08 host a: three\n",
            ],
        );
        // 保序：偏移严格递增。
        let starts: Vec<u64> = records.iter().map(|record| record.start_offset).collect();
        let mut sorted = starts.clone();
        sorted.sort_unstable();
        assert_eq!(starts, sorted);
        assert!(starts.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(
            records
                .iter()
                .map(|record| record.body.split_once(": ").expect("body").1)
                .collect::<Vec<_>>(),
            vec!["one\n", "two\n", "three\n"]
        );
    }

    /// 确定性伪随机（xorshift）：不引新依赖，但每次跑的是同一串序列。
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn pick(&mut self, bound: usize) -> usize {
            (self.next() % bound as u64) as usize
        }
    }

    #[test]
    fn no_byte_is_ever_silently_lost() {
        // 核心不变量：输入字节流要么落在某条记录里，要么被计入丢弃 —— 没有第三种去处。
        // 混三种形状、随机切批次、用**紧**上限，把跨批次 / 超限 / 丢弃 / 起头几条路一起压。
        let shapes = [
            "2026-09-23 20:24:24+08 host a: anchor line\n",
            "\tcontinuation\n",
            "    another continuation\n",
            "plain line without an anchor\n",
        ];
        let mut rng = Rng(0x5eed_1234_5678_9abc);
        let mut input = String::new();
        let mut lines: Vec<(&str, u64)> = Vec::new();
        for _ in 0..400 {
            let text = shapes[rng.pick(shapes.len())];
            lines.push((text, input.len() as u64));
            input.push_str(text);
        }
        // 以一条锚收尾：保证流尾手里确实有一条未封口的记录，把"到期封口"也压上
        // （否则流尾刚好吃到越限、被丢空，那条路就白测了）。
        lines.push((shapes[0], input.len() as u64));
        input.push_str(shapes[0]);

        let mut delimiter = Delimiter::new(Limits::new(3, 64), Start::WaitForStart);
        let mut records = Vec::new();
        let mut index = 0;
        while index < lines.len() {
            let batch = 1 + rng.pick(5);
            for (text, start) in &lines[index..(index + batch).min(lines.len())] {
                if let Some(record) = delimiter.push(line(text, *start), anchor) {
                    records.push(record);
                }
            }
            index += batch;
        }
        records.extend(delimiter.flush());

        assert!(
            records.len() > 10,
            "流里应当确实产出记录：{}",
            records.len()
        );
        // 自证：这份输入必须真的走到过丢弃/超限/到期这几条路，否则这个性质测试是空转的。
        assert!(delimiter.dropped_bytes() > 0, "应当走到过丢弃路径");
        assert!(
            records
                .iter()
                .any(|record| record.completion == Completion::Oversized),
            "应当走到过超限路径"
        );
        assert!(
            records
                .iter()
                .any(|record| record.completion == Completion::Deadline),
            "应当走到过到期封口路径"
        );
        for record in &records {
            assert_eq!(
                record.body,
                input[record.start_offset as usize..record.end_offset as usize],
                "记录的正文必须与它的区间逐字节对得上"
            );
            assert!(record.start_offset < record.end_offset, "{record:?}");
        }
        for pair in records.windows(2) {
            assert!(
                pair[0].end_offset <= pair[1].start_offset,
                "记录不许重叠或倒序：{pair:?}"
            );
        }
        let covered: u64 = records
            .iter()
            .map(|record| record.end_offset - record.start_offset)
            .sum();
        assert_eq!(
            covered + delimiter.dropped_bytes() as u64,
            input.len() as u64,
            "有字节去向不明（既不在记录里，也没被计入丢弃）"
        );
    }

    // ── 现成读法与真实形状 ────────────────────────────────────────────────────

    #[test]
    fn the_indented_reader_says_what_it_means() {
        assert_eq!(indented("plain line\n"), Boundary::Starts);
        assert_eq!(indented("  spaced\n"), Boundary::Neither);
        assert_eq!(indented("\ttabbed\n"), Boundary::Neither);
        // 语义严格、无例外：空行也算开始信号（它不是续行）。
        assert_eq!(indented("\n"), Boundary::Starts);
    }

    #[test]
    fn an_install_log_shaped_stream_folds_by_its_two_anchors() {
        // `/var/log/install.log` 的真实形状：两种时间戳锚（带年/时区 与 BSD）+ 缩进续行。
        // 缩进读法在这个文件上能蒙对，但把锚写准才是不依赖运气的做法。
        let mut delimiter = Delimiter::new(LIMITS, Start::WaitForStart);
        let lines = [
            "2026-09-23 20:24:24+08 MBP softwareupdated[565]: Setting up (\n",
            "\t\"<SUOSUProduct: MSU>\",\n",
            "\t)\n",
            "Jul 17 12:00:47 MBP Installer Progress[66]: phases set to (\n",
            "\t\"phase one\",\n",
            "\t)\n",
            "2026-09-23 20:24:25+08 MBP loginwindow[428]: policy = 0\n",
        ];
        let mut offset = 0;
        let mut records = Vec::new();
        for text in lines {
            if let Some(record) = delimiter.push(line(text, offset), install_log_anchor) {
                records.push(record);
            }
            offset += text.len() as u64;
        }
        records.extend(delimiter.flush());
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].lines, 3);
        assert_eq!(records[1].lines, 3, "BSD 锚也要认（它同样开一条新记录）");
        assert_eq!(records[2].lines, 1);
        assert_eq!(delimiter.dropped_lines(), 0);
        assert_eq!(
            records[0].body,
            "2026-09-23 20:24:24+08 MBP softwareupdated[565]: Setting up (\n\t\"<SUOSUProduct: MSU>\",\n\t)\n"
        );
    }

    // ── 契约、状态交接与补充不变量 ──────────────────────────────────────────

    #[test]
    fn limits_new_clamps_both_fields_to_at_least_one() {
        let clamped = Limits::new(0, 0);
        assert_eq!(clamped.max_lines, 1);
        assert_eq!(clamped.max_bytes, 1);
        assert_eq!(
            Limits::new(5, 9),
            Limits {
                max_lines: 5,
                max_bytes: 9,
            }
        );
    }

    #[test]
    fn pending_exposes_the_open_record_without_sealing_it() {
        let one = "2026-09-23 20:24:24+08 host a: one\n";
        let cont = "\tcont\n";
        let mut delimiter = anchored();
        assert!(delimiter.push(line(one, 0), anchor).is_none());
        assert!(
            delimiter
                .push(line(cont, one.len() as u64), anchor)
                .is_none()
        );

        let pending = delimiter.pending().expect("pending");
        assert_eq!(pending.body, format!("{one}{cont}"));
        assert_eq!(pending.lines, 2);
        assert_eq!(pending.start_offset, 0);
        assert_eq!(pending.end_offset, (one.len() + cont.len()) as u64);
        assert_eq!(delimiter.emitted(), 0, "pending 不算已产出");
        assert!(delimiter.is_accumulating());
    }

    #[test]
    fn into_pending_hands_over_content_and_is_none_when_idle() {
        assert!(anchored().into_pending().is_none());

        let one = "2026-09-23 20:24:24+08 host a: one\n";
        let mut delimiter = anchored();
        assert!(delimiter.push(line(one, 0), anchor).is_none());
        let taken = delimiter.into_pending().expect("pending");
        assert_eq!(taken.body, one);
        assert_eq!(taken.lines, 1);
    }

    #[test]
    fn resume_takes_the_record_as_is_and_resets_counters() {
        let one = "2026-09-23 20:24:24+08 host a: one\n";
        let record = Record {
            body: one.to_string(),
            start_offset: 10,
            end_offset: 10 + one.len() as u64,
            lines: 1,
            // completion 只是占位：resume / flush 会按封口方式改写它。
            completion: Completion::Oversized,
        };
        let mut delimiter = Delimiter::resume(Limits::new(10, 4096), Start::WaitForStart, record);
        assert_eq!(delimiter.dropped_lines(), 0);
        assert_eq!(delimiter.dropped_bytes(), 0);
        assert_eq!(delimiter.emitted(), 0);
        assert!(delimiter.is_accumulating());

        let sealed = delimiter.flush().expect("flush");
        assert_eq!(sealed.completion, Completion::Deadline);
        assert_eq!(sealed.body, one);
        assert_eq!(sealed.start_offset, 10);
        assert_eq!(sealed.end_offset, 10 + one.len() as u64);
        assert_eq!(delimiter.emitted(), 1);
    }

    #[test]
    fn completion_and_record_serde_contract() {
        // 枚举用外部标签，名字就是契约（checkpoint 存盘依赖它）。
        assert_eq!(
            serde_json::to_string(&Completion::Boundary).expect("serialize"),
            "\"Boundary\""
        );
        assert_eq!(
            serde_json::to_string(&Completion::Deadline).expect("serialize"),
            "\"Deadline\""
        );
        assert_eq!(
            serde_json::to_string(&Completion::Oversized).expect("serialize"),
            "\"Oversized\""
        );
        assert_eq!(
            serde_json::from_str::<Completion>("\"Oversized\"").expect("deserialize"),
            Completion::Oversized
        );

        // 未知字段被忽略；缺字段报错（Record 没有默认值）。
        let with_extra = r#"{"body":"x\n","start_offset":0,"end_offset":2,"lines":1,"completion":"Boundary","future":42}"#;
        let record: Record = serde_json::from_str(with_extra).expect("未知字段应被忽略");
        assert_eq!(record.body, "x\n");
        assert_eq!(record.completion, Completion::Boundary);

        let missing = r#"{"body":"x\n","start_offset":0,"end_offset":2,"lines":1}"#;
        assert!(
            serde_json::from_str::<Record>(missing).is_err(),
            "缺 completion 应当报错"
        );
    }

    /// 混合开始型 / 结束型信号时的守恒律：每个输入字节要么在某条记录里，
    /// 要么被计入丢弃，要么属于结束信号本身（它是间隔，不算内容）。
    #[test]
    fn end_signals_are_separators_so_their_bytes_are_accounted_for_separately() {
        let shapes = [
            "2026-09-23 20:24:24+08 host a: anchor line\n",
            "\tcontinuation\n",
            "    another continuation\n",
            "\n", // 结束信号
        ];
        let signal = |line: &str| {
            if line.trim().is_empty() {
                Boundary::Ends
            } else if anchor(line) == Boundary::Starts {
                Boundary::Starts
            } else {
                Boundary::Neither
            }
        };

        let mut rng = Rng(0x0bad_c0de_dead_beef);
        let mut input = String::new();
        let mut lines: Vec<(&str, u64)> = Vec::new();
        for _ in 0..400 {
            let text = shapes[rng.pick(shapes.len())];
            lines.push((text, input.len() as u64));
            input.push_str(text);
        }
        // 以结束信号收尾，逼出"到期封口"。
        lines.push((shapes[3], input.len() as u64));
        input.push_str(shapes[3]);

        let mut delimiter = Delimiter::new(Limits::new(3, 48), Start::WaitForStart);
        let mut records = Vec::new();
        let mut index = 0;
        while index < lines.len() {
            let batch = 1 + rng.pick(4);
            for (text, start) in &lines[index..(index + batch).min(lines.len())] {
                if let Some(record) = delimiter.push(line(text, *start), signal) {
                    records.push(record);
                }
            }
            index += batch;
        }

        // 自证走到了各条路径，否则这个性质测试是空转的。
        assert!(
            records
                .iter()
                .any(|record| record.completion == Completion::Oversized),
            "应当走到过超限路径"
        );
        assert!(delimiter.dropped_bytes() > 0, "应当走到过丢弃路径");

        let separators: u64 = lines
            .iter()
            .filter(|(text, _)| signal(text) == Boundary::Ends)
            .map(|(text, _)| text.len() as u64)
            .sum();
        assert!(separators > 0, "必须真的出现过结束信号");

        for record in &records {
            assert_eq!(
                record.body,
                input[record.start_offset as usize..record.end_offset as usize],
                "记录的正文必须与它的区间逐字节对得上"
            );
        }
        let covered: u64 = records
            .iter()
            .map(|record| record.end_offset - record.start_offset)
            .sum();
        assert_eq!(
            covered + delimiter.dropped_bytes() as u64 + separators,
            input.len() as u64,
            "有字节去向不明（既不在记录里，也没计入丢弃或间隔）"
        );

        // 换成结束型起点再验一次守恒：此时除了超限外不应再有"没有头"的丢弃。
        let mut collecting = Delimiter::new(Limits::new(3, 48), Start::Collect);
        let mut records = Vec::new();
        for (text, start) in &lines {
            if let Some(record) = collecting.push(line(text, *start), signal) {
                records.push(record);
            }
        }
        records.extend(collecting.flush());
        let covered: u64 = records
            .iter()
            .map(|record| record.end_offset - record.start_offset)
            .sum();
        assert_eq!(
            covered + collecting.dropped_bytes() as u64 + separators,
            input.len() as u64
        );
    }
}
