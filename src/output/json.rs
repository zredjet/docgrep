//! JSON Lines output: one object per match, raw text, no display merging (SPEC §5.3).

use std::io::{self, Write};

use serde::Serialize;

use super::{CharIndex, ContextOpts, FileHits, window};

#[derive(Serialize)]
struct JsonHeading<'a> {
    level: u8,
    number: Option<&'a str>,
    text: &'a str,
}

#[derive(Serialize)]
struct JsonMatch<'a> {
    file: &'a str,
    format: &'static str,
    part: &'static str,
    part_label: Option<String>,
    page: Option<u32>,
    page_mode: Option<&'static str>,
    heading: Option<JsonHeading<'a>>,
    sheet: Option<&'a str>,
    sheet_hidden: Option<bool>,
    cell: Option<&'a str>,
    before: &'a str,
    #[serde(rename = "match")]
    matched: &'a str,
    after: &'a str,
    offset: usize,
}

pub fn write_file(w: &mut impl Write, hits: &FileHits, opts: ContextOpts) -> io::Result<()> {
    for m in &hits.matches {
        let Some(unit) = hits.units.get(m.unit_index) else {
            continue;
        };
        let text = CharIndex::new(&unit.text);
        let (ws, we) = window(m.start, m.end, text.len(), opts);
        let loc = &unit.location;
        let page_less = loc.part.is_page_less();
        let record = JsonMatch {
            file: &hits.display,
            format: hits.format.as_str(),
            part: loc.part.kind(),
            part_label: loc.part.label(),
            page: if page_less {
                None
            } else {
                unit.page_at(m.start)
            },
            page_mode: if page_less {
                None
            } else {
                loc.page_mode.map(|p| p.as_str())
            },
            heading: loc.heading.as_ref().map(|h| JsonHeading {
                level: h.level,
                number: h.number.as_deref(),
                text: &h.text,
            }),
            sheet: loc.sheet.as_ref().map(|s| s.name.as_str()),
            sheet_hidden: loc.sheet.as_ref().map(|s| s.hidden),
            cell: loc.cell.as_deref(),
            before: text.slice(ws, m.start),
            matched: text.slice(m.start, m.end),
            after: text.slice(m.end, we),
            offset: m.start,
        };
        serde_json::to_writer(&mut *w, &record)?;
        writeln!(w)?;
    }
    Ok(())
}
