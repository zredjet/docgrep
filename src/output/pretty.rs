//! Human-readable colored output (SPEC §5.1, §5.2).
//!
//! Everything is written with ANSI styles; the caller wraps the destination in an
//! `anstream::AutoStream`, which strips or converts them according to `--color`.

use std::io::{self, Write};

use anstyle::{AnsiColor, Color, Style};
use unicode_width::UnicodeWidthChar;

use super::{CharIndex, ContextOpts, FileHits, build_entries};
use crate::model::{Format, Location, PageMode, Part};

const BOLD: Style = Style::new().bold();
const DIM: Style = Style::new().dimmed();
const PAGE: Style = AnsiColor::Yellow.on_default();
const HEADING: Style = AnsiColor::Cyan.on_default();
const LABEL: Style = AnsiColor::Magenta.on_default();
const SHEET: Style = AnsiColor::Cyan.on_default();
const CELL: Style = AnsiColor::Yellow.on_default();
const MATCH: Style = Style::new()
    .fg_color(Some(Color::Ansi(AnsiColor::White)))
    .bg_color(Some(Color::Ansi(AnsiColor::Red)))
    .bold();
pub const WARNING: Style = AnsiColor::Yellow.on_default();
pub const ERROR: Style = AnsiColor::Red.on_default();

const HEADING_WIDTH: usize = 40;

/// Writes one file block: the file line, then a location line and a context line per entry.
pub fn write_file(w: &mut impl Write, hits: &FileHits, opts: ContextOpts) -> io::Result<()> {
    write!(w, "{BOLD}■ {}{BOLD:#}", hits.display)?;
    write!(w, "  {DIM}{}件{DIM:#}", hits.matches.len())?;
    if hits.format == Format::Word
        && let Some(mode) = hits.page_mode
    {
        let desc = match mode {
            PageMode::Rendered => "ページ: Word保存時のレイアウト情報",
            PageMode::Explicit => "ページ: 明示改ページのみ・参考値",
        };
        write!(w, "  {DIM}({desc}){DIM:#}")?;
    }
    writeln!(w)?;

    for entry in build_entries(&hits.matches, &hits.units, opts) {
        let Some(unit) = hits.units.get(entry.unit_index) else {
            continue;
        };
        let first_start = entry.matches.first().map(|m| m.0).unwrap_or(0);
        write_location(w, &unit.location, unit.page_at(first_start))?;

        let text = CharIndex::new(&unit.text);
        write!(w, "        ")?;
        if entry.start > 0 {
            write!(w, "{DIM}…{DIM:#}")?;
        }
        let mut pos = entry.start;
        for &(s, e) in &entry.matches {
            write_plain(w, text.slice(pos, s))?;
            write!(w, "{MATCH}")?;
            write_sanitized(w, text.slice(s, e), None)?;
            write!(w, "{MATCH:#}")?;
            pos = e;
        }
        write_plain(w, text.slice(pos, entry.end))?;
        if entry.end < text.len() {
            write!(w, "{DIM}…{DIM:#}")?;
        }
        writeln!(w)?;
    }
    Ok(())
}

fn write_location(w: &mut impl Write, loc: &Location, page: Option<u32>) -> io::Result<()> {
    write!(w, "  ")?;
    if loc.part == Part::Cell {
        if let Some(sheet) = &loc.sheet {
            let hidden = if sheet.hidden { "(非表示)" } else { "" };
            write!(w, "{SHEET}[{}{hidden}]{SHEET:#} ", sheet.name)?;
        }
        if let Some(cell) = &loc.cell {
            write!(w, "{CELL}{cell}{CELL:#}")?;
        }
        return writeln!(w);
    }

    // Page field: at least 4 wide plus two spaces, longer values stretch it.
    match page.filter(|_| !loc.part.is_page_less()) {
        Some(p) => {
            let plus = if loc.page_mode == Some(PageMode::Explicit) {
                "+"
            } else {
                ""
            };
            let s = format!("p.{p}{plus}");
            write!(w, "{PAGE}{s}{PAGE:#}{}", pad(&s, 4))?;
        }
        None => write!(w, "{DIM}--{DIM:#}{}", pad("--", 4))?,
    }

    // Headers/footers have no heading; neither do notes whose reference was not found.
    let has_heading = !(loc.part.is_page_less() || (loc.part.is_note() && page.is_none()));
    if has_heading {
        match &loc.heading {
            Some(h) => {
                let full = match &h.number {
                    Some(n) if !n.is_empty() => format!("{n} {}", h.text),
                    _ => h.text.clone(),
                };
                write!(
                    w,
                    "{HEADING}{}{HEADING:#}",
                    truncate_width(&sanitize(&full), HEADING_WIDTH)
                )?;
            }
            None => write!(w, "{DIM}(冒頭){DIM:#}")?,
        }
    }
    if let Some(label) = loc.part.label() {
        let sep = if has_heading { "  " } else { "" };
        write!(w, "{sep}{LABEL}[{label}]{LABEL:#}")?;
    }
    writeln!(w)
}

fn pad(s: &str, width: usize) -> String {
    " ".repeat(width.saturating_sub(s.chars().count()) + 2)
}

/// Plain context text: `\n` becomes a dimmed `↵`.
fn write_plain(w: &mut impl Write, s: &str) -> io::Result<()> {
    write_sanitized(w, s, Some(DIM))
}

/// Writes text with `\n` → `↵`, `\t` → space and other control chars removed.
/// `newline_style` styles the `↵`; `None` leaves the surrounding style in effect.
fn write_sanitized(w: &mut impl Write, s: &str, newline_style: Option<Style>) -> io::Result<()> {
    let mut run = String::new();
    for c in s.chars() {
        match c {
            '\n' => {
                w.write_all(run.as_bytes())?;
                run.clear();
                match newline_style {
                    Some(st) => write!(w, "{st}↵{st:#}")?,
                    None => write!(w, "↵")?,
                }
            }
            '\t' => run.push(' '),
            c if c.is_control() => {}
            c => run.push(c),
        }
    }
    w.write_all(run.as_bytes())
}

/// Same replacement rules as the context line, as a plain string.
pub fn sanitize(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '\n' => Some('↵'),
            '\t' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

/// Truncates to at most `max` display columns, ending with `…` when cut.
pub fn truncate_width(s: &str, max: usize) -> String {
    let width: usize = s.chars().map(|c| c.width().unwrap_or(0)).sum();
    if width <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if used + cw + 1 > max {
            break;
        }
        used += cw;
        out.push(c);
    }
    out.push('…');
    out
}

/// Summary line printed to stderr after pretty output.
pub fn write_summary(w: &mut impl Write, stats: &crate::Stats) -> io::Result<()> {
    writeln!(
        w,
        "{DIM}検索 {}ファイル / ヒット {}ファイル {}件 / スキップ {} / エラー {}{DIM:#}",
        stats.searched, stats.hit_files, stats.hits, stats.skipped, stats.errors
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Heading, Match, SheetRef, TextUnit};

    fn render(hits: &FileHits, opts: ContextOpts) -> String {
        let mut buf = Vec::new();
        write_file(&mut buf, hits, opts).unwrap();
        anstream::adapter::strip_str(&String::from_utf8(buf).unwrap()).to_string()
    }

    fn hits(units: Vec<TextUnit>, matches: Vec<Match>) -> FileHits {
        FileHits {
            display: "a.docx".to_string(),
            format: Format::Word,
            page_mode: None,
            units,
            matches,
        }
    }

    const C3: ContextOpts = ContextOpts {
        context: 3,
        paragraph: false,
    };

    #[test]
    fn ellipsis_only_on_truncated_sides() {
        let u = TextUnit::new("0123456789".into(), Location::new(Part::Body));
        let m = |s, e| Match {
            unit_index: 0,
            start: s,
            end: e,
        };
        let out = render(&hits(vec![u.clone()], vec![m(6, 7)]), C3);
        assert!(out.contains("        …3456789\n"), "{out}");
        let out = render(&hits(vec![u.clone()], vec![m(8, 9)]), C3);
        assert!(out.contains("        …56789\n"), "{out}");
        let out = render(&hits(vec![u.clone()], vec![m(0, 1)]), C3);
        assert!(out.contains("        0123…\n"), "{out}");
        let out = render(&hits(vec![u], vec![m(4, 5)]), C3);
        assert!(out.contains("        …1234567…\n"), "{out}");
    }

    #[test]
    fn control_characters_are_replaced() {
        let u = TextUnit::new("a\nb\tc\u{7}d".into(), Location::new(Part::Body));
        let out = render(
            &hits(
                vec![u],
                vec![Match {
                    unit_index: 0,
                    start: 2,
                    end: 3,
                }],
            ),
            ContextOpts {
                context: 10,
                paragraph: false,
            },
        );
        assert!(out.contains("        a↵b cd\n"), "{out}");
    }

    #[test]
    fn location_line_variants() {
        let mut loc = Location::new(Part::Table {
            index: 2,
            row: 3,
            col: 1,
        });
        loc.page_at_start = Some(12);
        loc.page_mode = Some(PageMode::Rendered);
        loc.heading = Some(Heading {
            level: 3,
            number: Some("3.2.1".into()),
            text: "システム構成".into(),
        });
        let mut buf = Vec::new();
        write_location(&mut buf, &loc, Some(12)).unwrap();
        let s = anstream::adapter::strip_str(&String::from_utf8(buf).unwrap()).to_string();
        assert_eq!(s, "  p.12  3.2.1 システム構成  [表2 3行1列]\n");

        let loc = Location::new(Part::Header);
        let mut buf = Vec::new();
        write_location(&mut buf, &loc, None).unwrap();
        let s = anstream::adapter::strip_str(&String::from_utf8(buf).unwrap()).to_string();
        assert_eq!(s, "  --    [ヘッダー]\n");

        let loc = Location::new(Part::Body);
        let mut buf = Vec::new();
        write_location(&mut buf, &loc, None).unwrap();
        let s = anstream::adapter::strip_str(&String::from_utf8(buf).unwrap()).to_string();
        assert_eq!(s, "  --    (冒頭)\n");

        let mut loc = Location::new(Part::Cell);
        loc.sheet = Some(SheetRef {
            name: "内訳".into(),
            hidden: true,
        });
        loc.cell = Some("C14".into());
        let mut buf = Vec::new();
        write_location(&mut buf, &loc, None).unwrap();
        let s = anstream::adapter::strip_str(&String::from_utf8(buf).unwrap()).to_string();
        assert_eq!(s, "  [内訳(非表示)] C14\n");
    }

    #[test]
    fn explicit_pages_get_plus_and_long_pages_stretch() {
        let mut loc = Location::new(Part::Body);
        loc.page_mode = Some(PageMode::Explicit);
        let mut buf = Vec::new();
        write_location(&mut buf, &loc, Some(123)).unwrap();
        let s = anstream::adapter::strip_str(&String::from_utf8(buf).unwrap()).to_string();
        assert_eq!(s, "  p.123+  (冒頭)\n");
    }

    #[test]
    fn truncation_uses_display_width() {
        assert_eq!(truncate_width("abc", 3), "abc");
        assert_eq!(truncate_width("abcd", 3), "ab…");
        assert_eq!(truncate_width("あいう", 6), "あいう");
        assert_eq!(truncate_width("あいうえ", 6), "あい…");
        let long = "見".repeat(30);
        let t = truncate_width(&long, 40);
        assert_eq!(t, format!("{}…", "見".repeat(19)));
    }
}
