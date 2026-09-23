//! numbering.xml: list definitions, counters and number formatting (SPEC §6.5 (b)–(d)).

use std::collections::{HashMap, HashSet};
use std::io::BufRead;

use super::styles::Styles;
use super::xml::{self, Alt, Step};

/// Number of list levels (ilvl 0..=8).
pub const LEVELS: usize = 9;

/// Number format of a list level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumFmt {
    Decimal,
    DecimalZero,
    DecimalFullWidth,
    UpperRoman,
    LowerRoman,
    UpperLetter,
    LowerLetter,
    DecimalEnclosedCircle,
    Aiueo,
    AiueoFullWidth,
    Iroha,
    IrohaFullWidth,
    JapaneseCounting,
    IdeographDigital,
    IdeographTraditional,
    IdeographZodiac,
    Bullet,
    None,
    /// Word's `custom` format such as `001, 002, 003, ...` (zero-padded decimal).
    ZeroPadded(usize),
}

impl NumFmt {
    /// Parses `w:numFmt`. Returns `None` for a `custom` format we cannot interpret,
    /// so that the caller can fall back to another candidate.
    pub fn parse(val: &str, format: Option<&str>) -> Option<NumFmt> {
        Some(match val {
            "decimal" => NumFmt::Decimal,
            "decimalZero" => NumFmt::DecimalZero,
            "decimalFullWidth" | "decimalFullWidth2" => NumFmt::DecimalFullWidth,
            "upperRoman" => NumFmt::UpperRoman,
            "lowerRoman" => NumFmt::LowerRoman,
            "upperLetter" => NumFmt::UpperLetter,
            "lowerLetter" => NumFmt::LowerLetter,
            "decimalEnclosedCircle" => NumFmt::DecimalEnclosedCircle,
            "aiueo" => NumFmt::Aiueo,
            "aiueoFullWidth" => NumFmt::AiueoFullWidth,
            "iroha" => NumFmt::Iroha,
            "irohaFullWidth" => NumFmt::IrohaFullWidth,
            "japaneseCounting" => NumFmt::JapaneseCounting,
            "japaneseDigitalTenThousand" | "ideographDigital" => NumFmt::IdeographDigital,
            "ideographTraditional" => NumFmt::IdeographTraditional,
            "ideographZodiac" => NumFmt::IdeographZodiac,
            "bullet" => NumFmt::Bullet,
            "none" => NumFmt::None,
            "custom" => return parse_custom(format?),
            _ => NumFmt::Decimal,
        })
    }

    /// Formats a counter value. `Bullet` yields an empty string.
    pub fn format(self, n: u32) -> String {
        match self {
            NumFmt::Decimal => n.to_string(),
            NumFmt::DecimalZero => format!("{n:02}"),
            NumFmt::ZeroPadded(width) => format!("{n:0width$}"),
            NumFmt::DecimalFullWidth => n
                .to_string()
                .chars()
                .filter_map(|c| c.to_digit(10))
                .filter_map(|d| char::from_u32(0xFF10 + d))
                .collect(),
            NumFmt::UpperRoman => roman(n).unwrap_or_else(|| n.to_string()),
            NumFmt::LowerRoman => roman(n)
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_else(|| n.to_string()),
            NumFmt::UpperLetter => letters(n, b'A').unwrap_or_else(|| n.to_string()),
            NumFmt::LowerLetter => letters(n, b'a').unwrap_or_else(|| n.to_string()),
            NumFmt::DecimalEnclosedCircle => enclosed_circle(n).unwrap_or_else(|| n.to_string()),
            NumFmt::Aiueo => cycle(n, AIUEO_HALF),
            NumFmt::AiueoFullWidth => cycle(n, AIUEO_FULL),
            NumFmt::Iroha => cycle(n, IROHA_HALF),
            NumFmt::IrohaFullWidth => cycle(n, IROHA_FULL),
            NumFmt::JapaneseCounting => japanese_counting(n),
            NumFmt::IdeographDigital => n
                .to_string()
                .chars()
                .filter_map(|c| c.to_digit(10))
                .filter_map(|d| KANJI_DIGITS.chars().nth(d as usize))
                .collect(),
            NumFmt::IdeographTraditional => cycle(n, "甲乙丙丁戊己庚辛壬癸"),
            NumFmt::IdeographZodiac => cycle(n, "子丑寅卯辰巳午未申酉戌亥"),
            NumFmt::Bullet | NumFmt::None => String::new(),
        }
    }
}

/// `001, 002, 003, ...` → zero-padded decimal of width 3.
fn parse_custom(format: &str) -> Option<NumFmt> {
    let first = format.split(',').next()?.trim();
    if !first.is_empty() && first.chars().all(|c| c.is_ascii_digit()) {
        Some(NumFmt::ZeroPadded(first.len()))
    } else {
        None
    }
}

const KANJI_DIGITS: &str = "〇一二三四五六七八九";

// V3: order and wrap-around of the kana formats still need checking against Word.
const AIUEO_FULL: &str =
    "アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワヲン";
const AIUEO_HALF: &str = "ｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉﾊﾋﾌﾍﾎﾏﾐﾑﾒﾓﾔﾕﾖﾗﾘﾙﾚﾛﾜｦﾝ";
const IROHA_FULL: &str = "イロハニホヘトチリヌルヲワカヨタレソツネナラムウヰノオクヤマケフコエテアサキユメミシヱヒモセス";
// Half-width katakana has no ヰ/ヱ; ｲ/ｴ stand in for them.
const IROHA_HALF: &str = "ｲﾛﾊﾆﾎﾍﾄﾁﾘﾇﾙｦﾜｶﾖﾀﾚｿﾂﾈﾅﾗﾑｳｲﾉｵｸﾔﾏｹﾌｺｴﾃｱｻｷﾕﾒﾐｼｴﾋﾓｾｽ";

/// 1-based cyclic lookup; 0 falls back to decimal.
fn cycle(n: u32, table: &str) -> String {
    let len = table.chars().count();
    if n == 0 || len == 0 {
        return n.to_string();
    }
    let idx = (n as usize - 1) % len;
    table
        .chars()
        .nth(idx)
        .map(String::from)
        .unwrap_or_else(|| n.to_string())
}

fn roman(n: u32) -> Option<String> {
    if n == 0 || n >= 4000 {
        return None;
    }
    const TABLE: [(u32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut rest = n;
    let mut out = String::new();
    for (value, sym) in TABLE {
        while rest >= value {
            out.push_str(sym);
            rest -= value;
        }
    }
    Some(out)
}

/// A..Z, then AA..ZZ, AAA.. (the same letter repeated).
fn letters(n: u32, base: u8) -> Option<String> {
    if n == 0 {
        return None;
    }
    let letter = char::from(base + ((n - 1) % 26) as u8);
    let count = (n - 1) / 26 + 1;
    Some(std::iter::repeat_n(letter, count as usize).collect())
}

/// ①..⑳ (U+2460), ㉑..㉟ (U+3251), ㊱..㊿ (U+32B1); beyond 50 → decimal.
fn enclosed_circle(n: u32) -> Option<String> {
    let code = match n {
        1..=20 => 0x2460 + n - 1,
        21..=35 => 0x3251 + n - 21,
        36..=50 => 0x32B1 + n - 36,
        _ => return None,
    };
    char::from_u32(code).map(String::from)
}

/// 一, 十, 十一, 二十, 百, 百一, 千, 一万 …
fn japanese_counting(n: u32) -> String {
    if n == 0 {
        return "〇".to_string();
    }
    let digit = |d: u32| KANJI_DIGITS.chars().nth(d as usize).unwrap_or('〇');
    // Below 10,000: 千/百/十 without a leading 一.
    let below_man = |mut v: u32| {
        let mut out = String::new();
        for (unit, sym) in [(1000, '千'), (100, '百'), (10, '十')] {
            let d = v / unit;
            if d > 0 {
                if d > 1 {
                    out.push(digit(d));
                }
                out.push(sym);
            }
            v %= unit;
        }
        if v > 0 {
            out.push(digit(v));
        }
        out
    };
    let mut out = String::new();
    let mut rest = n;
    for (unit, sym) in [(100_000_000, '億'), (10_000, '万')] {
        let d = rest / unit;
        if d > 0 {
            out.push_str(&below_man(d));
            out.push(sym);
        }
        rest %= unit;
    }
    out.push_str(&below_man(rest));
    out
}

/// One level of a list definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Level {
    pub start: u32,
    pub num_fmt: NumFmt,
    pub lvl_text: Option<String>,
    /// `w:lvlRestart` (1-based level after which this level restarts; 0 = never).
    pub restart: Option<u32>,
    pub is_lgl: bool,
    pub p_style: Option<String>,
}

impl Default for Level {
    fn default() -> Self {
        Level {
            start: 0,
            num_fmt: NumFmt::Decimal,
            lvl_text: None,
            restart: None,
            is_lgl: false,
            p_style: None,
        }
    }
}

/// `w:lvl` while it is being read: numFmt candidates by Markup Compatibility context.
#[derive(Debug, Default)]
struct LevelBuilder {
    ilvl: usize,
    level: Level,
    fmt_choice: Option<Option<NumFmt>>,
    fmt_plain: Option<Option<NumFmt>>,
    fmt_fallback: Option<Option<NumFmt>>,
}

impl LevelBuilder {
    /// Picks the numFmt: a usable Choice, else the plain element, else the Fallback.
    fn finish(mut self) -> (usize, Level) {
        self.level.num_fmt = [self.fmt_choice, self.fmt_plain, self.fmt_fallback]
            .into_iter()
            .flatten()
            .flatten()
            .next()
            .unwrap_or(NumFmt::Decimal);
        (self.ilvl, self.level)
    }
}

#[derive(Debug, Clone, Default)]
pub struct AbstractNum {
    pub levels: [Option<Level>; LEVELS],
    pub num_style_link: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LevelOverride {
    pub start_override: Option<u32>,
    pub level: Option<Level>,
}

#[derive(Debug, Clone, Default)]
pub struct Num {
    pub abstract_id: Option<u32>,
    pub overrides: [Option<LevelOverride>; LEVELS],
}

#[derive(Debug, Clone, Default)]
pub struct Numbering {
    pub abstracts: HashMap<u32, AbstractNum>,
    pub nums: HashMap<u32, Num>,
}

fn ilvl_index(v: Option<String>) -> Option<usize> {
    v.and_then(|v| v.parse::<usize>().ok())
        .filter(|i| *i < LEVELS)
}

impl Numbering {
    /// Parses numbering.xml. On malformed XML, returns what was read so far with the error.
    pub fn parse<R: BufRead>(reader: R) -> (Numbering, Option<String>) {
        let mut out = Numbering::default();
        let mut abs: Option<(u32, AbstractNum)> = None;
        let mut num: Option<(u32, Num)> = None;
        let mut over: Option<(usize, LevelOverride)> = None;
        let mut lvl: Option<LevelBuilder> = None;

        let result = xml::walk(reader, |step| match step {
            Step::Open { path, node, attr } => {
                let alt = xml::alt_state(path);
                if alt == Alt::Ignored {
                    return;
                }
                let Some(name) = node.w.as_deref() else {
                    return;
                };
                // Only numFmt is taken from mc:Fallback (see SPEC decision log, B5).
                if alt == Alt::Fallback && name != "numFmt" {
                    return;
                }
                let parents = xml::w_path(path);
                let parent = parents.last().copied().flatten();
                match (parent, name) {
                    (Some("numbering"), "abstractNum") => {
                        abs = attr("abstractNumId")
                            .and_then(|v| v.parse().ok())
                            .map(|id| (id, AbstractNum::default()));
                    }
                    (Some("abstractNum"), "numStyleLink") => {
                        if let Some((_, a)) = abs.as_mut() {
                            a.num_style_link = attr("val");
                        }
                    }
                    (Some("numbering"), "num") => {
                        num = attr("numId")
                            .and_then(|v| v.parse().ok())
                            .map(|id| (id, Num::default()));
                    }
                    (Some("num"), "abstractNumId") => {
                        if let Some((_, n)) = num.as_mut() {
                            n.abstract_id = attr("val").and_then(|v| v.parse().ok());
                        }
                    }
                    (Some("num"), "lvlOverride") => {
                        over = ilvl_index(attr("ilvl")).map(|i| (i, LevelOverride::default()));
                    }
                    (Some("lvlOverride"), "startOverride") => {
                        if let Some((_, o)) = over.as_mut() {
                            o.start_override = attr("val").and_then(|v| v.parse().ok());
                        }
                    }
                    (Some("abstractNum" | "lvlOverride"), "lvl") => {
                        lvl = ilvl_index(attr("ilvl")).map(|ilvl| LevelBuilder {
                            ilvl,
                            ..LevelBuilder::default()
                        });
                    }
                    (Some("lvl"), _) => {
                        let Some(b) = lvl.as_mut() else { return };
                        let val = attr("val");
                        match name {
                            "start" => {
                                if let Some(v) = val.and_then(|v| v.parse().ok()) {
                                    b.level.start = v;
                                }
                            }
                            "numFmt" => {
                                let fmt = val.map(|v| NumFmt::parse(&v, attr("format").as_deref()));
                                let fmt = fmt.unwrap_or(Some(NumFmt::Decimal));
                                match alt {
                                    Alt::Choice => b.fmt_choice = Some(fmt),
                                    Alt::Fallback => b.fmt_fallback = Some(fmt),
                                    _ => b.fmt_plain = Some(fmt),
                                }
                            }
                            "lvlText" => b.level.lvl_text = val,
                            "lvlRestart" => b.level.restart = val.and_then(|v| v.parse().ok()),
                            "isLgl" => b.level.is_lgl = xml::on_off(val.as_deref()),
                            "pStyle" => b.level.p_style = val,
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            Step::Close { path, node } => {
                let parent = xml::w_path(path).last().copied().flatten();
                match (parent, node.w.as_deref()) {
                    (Some("abstractNum"), Some("lvl")) => {
                        if let (Some(b), Some((_, a))) = (lvl.take(), abs.as_mut()) {
                            let (i, level) = b.finish();
                            if let Some(slot) = a.levels.get_mut(i) {
                                slot.get_or_insert(level);
                            }
                        }
                    }
                    (Some("lvlOverride"), Some("lvl")) => {
                        if let (Some(b), Some((_, o))) = (lvl.take(), over.as_mut()) {
                            o.level = Some(b.finish().1);
                        }
                    }
                    (Some("num"), Some("lvlOverride")) => {
                        if let (Some((i, o)), Some((_, n))) = (over.take(), num.as_mut())
                            && let Some(slot) = n.overrides.get_mut(i)
                        {
                            *slot = Some(o);
                        }
                    }
                    (Some("numbering"), Some("abstractNum")) => {
                        if let Some((id, a)) = abs.take() {
                            out.abstracts.entry(id).or_insert(a);
                        }
                    }
                    (Some("numbering"), Some("num")) => {
                        if let Some((id, n)) = num.take() {
                            out.nums.entry(id).or_insert(n);
                        }
                    }
                    _ => {}
                }
            }
        });
        (out, result.err())
    }

    /// The abstractNum that actually defines the levels of `num_id`, following one
    /// `numStyleLink` hop through a numbering style.
    pub fn resolve_abstract(&self, num_id: u32, styles: &Styles) -> Option<u32> {
        let abs_id = self.nums.get(&num_id)?.abstract_id?;
        let abs = self.abstracts.get(&abs_id)?;
        match &abs.num_style_link {
            Some(link) => {
                let linked_num = styles.numbering_style_num_id(link)?;
                let linked_abs = self.nums.get(&linked_num)?.abstract_id?;
                self.abstracts
                    .contains_key(&linked_abs)
                    .then_some(linked_abs)
            }
            None => Some(abs_id),
        }
    }

    /// Level definition of `num_id` at `ilvl`, with `lvlOverride/lvl` applied.
    pub fn level(&self, num_id: u32, abs_id: u32, ilvl: usize) -> Option<&Level> {
        let overridden = self
            .nums
            .get(&num_id)
            .and_then(|n| n.overrides.get(ilvl))
            .and_then(|o| o.as_ref())
            .and_then(|o| o.level.as_ref());
        overridden.or_else(|| {
            self.abstracts
                .get(&abs_id)
                .and_then(|a| a.levels.get(ilvl))
                .and_then(|l| l.as_ref())
        })
    }

    /// The level whose `w:pStyle` names `style_id`, for paragraphs without an ilvl.
    pub fn level_for_style(&self, abs_id: u32, style_id: &str) -> Option<usize> {
        self.abstracts.get(&abs_id)?.levels.iter().position(|l| {
            l.as_ref()
                .and_then(|l| l.p_style.as_deref())
                .is_some_and(|s| s == style_id)
        })
    }
}

/// Counter state of one abstractNum.
#[derive(Debug, Clone, Default)]
struct CounterState {
    /// Current value per level; `None` = not used yet.
    values: [Option<u32>; LEVELS],
    /// Start value set by a num's `startOverride`.
    start_override: [Option<u32>; LEVELS],
}

/// List counters for a whole document. Kept per abstractNum, so different numIds
/// sharing one abstractNum continue each other's numbering (V2).
#[derive(Debug, Clone, Default)]
pub struct Counters {
    by_abstract: HashMap<u32, CounterState>,
    used_nums: HashSet<u32>,
}

impl Counters {
    /// Advances the counters for a numbered paragraph and returns its number string.
    /// Returns `None` when the level is a bullet or the text comes out empty.
    pub fn advance(
        &mut self,
        numbering: &Numbering,
        styles: &Styles,
        num_id: u32,
        ilvl: usize,
    ) -> Option<String> {
        let abs_id = numbering.resolve_abstract(num_id, styles)?;
        let level = numbering.level(num_id, abs_id, ilvl)?;
        let state = self.by_abstract.entry(abs_id).or_default();

        if self.used_nums.insert(num_id)
            && let Some(num) = numbering.nums.get(&num_id)
        {
            for (i, o) in num.overrides.iter().enumerate() {
                if let Some(start) = o.as_ref().and_then(|o| o.start_override)
                    && let (Some(v), Some(s)) =
                        (state.values.get_mut(i), state.start_override.get_mut(i))
                {
                    *v = None;
                    *s = Some(start);
                }
            }
        }

        let start_of = |state: &CounterState, i: usize| {
            state
                .start_override
                .get(i)
                .copied()
                .flatten()
                .or_else(|| numbering.level(num_id, abs_id, i).map(|l| l.start))
                .unwrap_or(0)
        };

        let next = match state.values.get(ilvl).copied().flatten() {
            Some(v) => v.saturating_add(1),
            None => start_of(state, ilvl),
        };
        if let Some(slot) = state.values.get_mut(ilvl) {
            *slot = Some(next);
        }
        for deeper in ilvl + 1..LEVELS {
            let restart = numbering
                .level(num_id, abs_id, deeper)
                .and_then(|l| l.restart);
            let reset = match restart {
                None => true,
                Some(0) => false,
                Some(k) => (ilvl as u32) < k,
            };
            if reset && let Some(slot) = state.values.get_mut(deeper) {
                *slot = None;
            }
        }

        if level.num_fmt == NumFmt::Bullet {
            return None;
        }
        let text = level.lvl_text.as_deref()?;
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            let placeholder = if c == '%' {
                chars
                    .peek()
                    .and_then(|d| d.to_digit(10))
                    .filter(|d| (1..=9).contains(d))
            } else {
                None
            };
            let Some(n) = placeholder else {
                out.push(c);
                continue;
            };
            chars.next();
            let i = n as usize - 1;
            let value = state
                .values
                .get(i)
                .copied()
                .flatten()
                .unwrap_or_else(|| start_of(state, i));
            let fmt = if level.is_lgl {
                NumFmt::Decimal
            } else {
                numbering
                    .level(num_id, abs_id, i)
                    .map(|l| l.num_fmt)
                    .unwrap_or(NumFmt::Decimal)
            };
            out.push_str(&fmt.format(value));
        }
        (!out.is_empty()).then_some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(fmt: NumFmt, ns: &[u32]) -> Vec<String> {
        ns.iter().map(|&n| fmt.format(n)).collect()
    }

    #[test]
    fn decimal_family() {
        assert_eq!(f(NumFmt::Decimal, &[1, 2, 3, 10]), ["1", "2", "3", "10"]);
        assert_eq!(
            f(NumFmt::DecimalZero, &[1, 2, 3, 10]),
            ["01", "02", "03", "10"]
        );
        assert_eq!(
            f(NumFmt::DecimalFullWidth, &[1, 2, 3, 10]),
            ["１", "２", "３", "１０"]
        );
        assert_eq!(
            f(NumFmt::ZeroPadded(3), &[1, 10, 1000]),
            ["001", "010", "1000"]
        );
    }

    #[test]
    fn roman_and_letters() {
        assert_eq!(
            f(NumFmt::UpperRoman, &[1, 2, 3, 10, 4, 1994]),
            ["I", "II", "III", "X", "IV", "MCMXCIV"]
        );
        assert_eq!(
            f(NumFmt::LowerRoman, &[1, 2, 3, 10]),
            ["i", "ii", "iii", "x"]
        );
        assert_eq!(
            f(NumFmt::UpperLetter, &[1, 2, 3, 10, 26, 27, 28, 53]),
            ["A", "B", "C", "J", "Z", "AA", "BB", "AAA"]
        );
        assert_eq!(f(NumFmt::LowerLetter, &[1, 2, 3, 10]), ["a", "b", "c", "j"]);
        assert_eq!(f(NumFmt::UpperRoman, &[0]), ["0"]);
    }

    #[test]
    fn enclosed_circles() {
        assert_eq!(
            f(
                NumFmt::DecimalEnclosedCircle,
                &[1, 2, 3, 10, 20, 21, 35, 36, 50, 51]
            ),
            ["①", "②", "③", "⑩", "⑳", "㉑", "㉟", "㊱", "㊿", "51"]
        );
    }

    #[test]
    fn kana_cycles() {
        assert_eq!(f(NumFmt::Aiueo, &[1, 2, 3]), ["ｱ", "ｲ", "ｳ"]);
        assert_eq!(
            f(NumFmt::AiueoFullWidth, &[1, 2, 3, 46, 47]),
            ["ア", "イ", "ウ", "ン", "ア"]
        );
        assert_eq!(f(NumFmt::Iroha, &[1, 2, 3]), ["ｲ", "ﾛ", "ﾊ"]);
        assert_eq!(
            f(NumFmt::IrohaFullWidth, &[1, 2, 3, 47, 48]),
            ["イ", "ロ", "ハ", "ス", "イ"]
        );
        assert_eq!(AIUEO_HALF.chars().count(), AIUEO_FULL.chars().count());
        assert_eq!(IROHA_HALF.chars().count(), IROHA_FULL.chars().count());
    }

    #[test]
    fn kanji_numbers() {
        assert_eq!(
            f(
                NumFmt::JapaneseCounting,
                &[
                    1, 2, 3, 10, 11, 20, 21, 100, 101, 110, 1000, 2024, 10000, 12345
                ]
            ),
            [
                "一",
                "二",
                "三",
                "十",
                "十一",
                "二十",
                "二十一",
                "百",
                "百一",
                "百十",
                "千",
                "二千二十四",
                "一万",
                "一万二千三百四十五"
            ]
        );
        assert_eq!(
            f(NumFmt::IdeographDigital, &[1, 2, 3, 10, 105]),
            ["一", "二", "三", "一〇", "一〇五"]
        );
        assert_eq!(
            f(NumFmt::IdeographTraditional, &[1, 2, 3, 10, 11]),
            ["甲", "乙", "丙", "癸", "甲"]
        );
        assert_eq!(
            f(NumFmt::IdeographZodiac, &[1, 2, 3, 12, 13]),
            ["子", "丑", "寅", "亥", "子"]
        );
    }

    #[test]
    fn parse_formats() {
        assert_eq!(
            NumFmt::parse("decimalFullWidth2", None),
            Some(NumFmt::DecimalFullWidth)
        );
        assert_eq!(
            NumFmt::parse("japaneseDigitalTenThousand", None),
            Some(NumFmt::IdeographDigital)
        );
        assert_eq!(NumFmt::parse("ordinal", None), Some(NumFmt::Decimal));
        assert_eq!(
            NumFmt::parse("custom", Some("001, 002, 003, ...")),
            Some(NumFmt::ZeroPadded(3))
        );
        assert_eq!(NumFmt::parse("custom", Some("一, 二, 三, ...")), None);
        assert_eq!(NumFmt::parse("custom", None), None);
        assert_eq!(NumFmt::None.format(3), "");
    }
}
