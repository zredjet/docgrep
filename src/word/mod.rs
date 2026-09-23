//! Word (.docx family) extraction: open the package, then extract each part.
//!
//! Output order (SPEC §6.4): body → footnotes → endnotes → comments → headers → footers.

pub mod extract;
pub mod headings;
pub mod numbering;
pub mod package;
pub mod pages;
pub mod styles;
pub mod xml;

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::Path;

use crate::error::FileError;
use crate::model::{Extracted, Format, PageMode, Part, TextUnit};

use extract::{NoteRef, PartKind, RefKind};
use headings::DocContext;
use numbering::Numbering;
use package::{Package, Relationship};
use styles::Styles;

/// Extracts all searchable text units from a Word file.
///
/// Broken or missing secondary parts are reported as a partial error; everything
/// that could be read is still returned.
pub fn extract(path: &Path) -> Result<Extracted, FileError> {
    let mut pkg = Package::open(path)?;
    let main = pkg.main_document_part()?;
    let rels = pkg.relationships(&main)?;
    let mut errors: Vec<FileError> = Vec::new();

    let mut ctx = DocContext::default();
    if let Some(part) = first_target(&rels, "styles").filter(|p| pkg.has_part(p)) {
        let (styles, err) = Styles::parse(pkg.open_part(&part)?);
        ctx.styles = styles;
        errors.extend(err.map(|message| FileError::Xml { part, message }));
    }
    if let Some(part) = first_target(&rels, "numbering").filter(|p| pkg.has_part(p)) {
        let (numbering, err) = Numbering::parse(pkg.open_part(&part)?);
        ctx.numbering = numbering;
        errors.extend(err.map(|message| FileError::Xml { part, message }));
    }

    let body = extract::extract_part(pkg.open_part(&main)?, &main, &ctx, PartKind::Body);
    errors.extend(body.error);
    let page_mode = body.page_mode;
    let mut units = body.units;

    let refs = index_refs(body.refs);
    let secondary = [
        ("footnotes", PartKind::Footnotes),
        ("endnotes", PartKind::Endnotes),
        ("comments", PartKind::Comments),
    ];
    let mut parts: Vec<(String, PartKind)> = secondary
        .iter()
        .filter_map(|&(rel, kind)| first_target(&rels, rel).map(|t| (t, kind)))
        .collect();
    parts.extend(all_targets(&rels, "header").map(|t| (t, PartKind::Header)));
    parts.extend(all_targets(&rels, "footer").map(|t| (t, PartKind::Footer)));

    for (part, kind) in parts {
        if !pkg.has_part(&part) {
            continue;
        }
        let text = extract::extract_part(pkg.open_part(&part)?, &part, &ctx, kind);
        errors.extend(text.error);
        for mut unit in text.units {
            link_to_reference(&mut unit, &refs, page_mode);
            units.push(unit);
        }
    }

    Ok(Extracted {
        format: Format::Word,
        page_mode,
        units,
        partial_error: errors.into_iter().next(),
    })
}

fn first_target(rels: &[Relationship], kind: &str) -> Option<String> {
    rels.iter()
        .find(|r| r.rel_type == kind)
        .map(|r| r.target.clone())
}

/// Distinct targets of a relationship type in natural order (header2 before header10).
/// A part shared by several sections is searched once.
fn all_targets(rels: &[Relationship], kind: &str) -> impl Iterator<Item = String> {
    let mut targets: Vec<String> = rels
        .iter()
        .filter(|r| r.rel_type == kind)
        .map(|r| r.target.clone())
        .collect();
    targets.sort_by(|a, b| natural_cmp(a, b));
    targets.dedup();
    targets.into_iter()
}

/// The first reference of each note or comment.
fn index_refs(refs: Vec<NoteRef>) -> HashMap<(RefKind, String), NoteRef> {
    let mut map = HashMap::new();
    for r in refs {
        map.entry((r.kind, r.id.clone())).or_insert(r);
    }
    map
}

/// Gives a note or comment the page and heading of its reference in the body.
/// Unreferenced ones keep neither.
fn link_to_reference(
    unit: &mut TextUnit,
    refs: &HashMap<(RefKind, String), NoteRef>,
    mode: Option<PageMode>,
) {
    let key = match &unit.location.part {
        Part::Footnote { id } => (RefKind::Footnote, id.clone()),
        Part::Endnote { id } => (RefKind::Endnote, id.clone()),
        Part::Comment { id, .. } => (RefKind::Comment, id.clone()),
        _ => return,
    };
    if let Some(r) = refs.get(&key) {
        unit.location.page_at_start = Some(r.page);
        unit.location.page_mode = mode;
        unit.location.heading = r.heading.clone();
    }
}

/// Compares strings treating runs of ASCII digits as numbers.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a, b);
    loop {
        let (Some(ca), Some(cb)) = (a.chars().next(), b.chars().next()) else {
            return a.len().cmp(&b.len());
        };
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let da = a.len() - a.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            let db = b.len() - b.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            let (na, ra) = a.split_at_checked(da).unwrap_or((a, ""));
            let (nb, rb) = b.split_at_checked(db).unwrap_or((b, ""));
            let (na, nb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
            let ord = na.len().cmp(&nb.len()).then_with(|| na.cmp(nb));
            if ord != Ordering::Equal {
                return ord;
            }
            (a, b) = (ra, rb);
        } else {
            if ca != cb {
                return ca.cmp(&cb);
            }
            a = a.get(ca.len_utf8()..).unwrap_or_default();
            b = b.get(cb.len_utf8()..).unwrap_or_default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["word/header10.xml", "word/header2.xml", "word/header1.xml"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            v,
            ["word/header1.xml", "word/header2.xml", "word/header10.xml"]
        );
        assert_eq!(natural_cmp("a01", "a1"), Ordering::Equal);
        assert_eq!(natural_cmp("a", "ab"), Ordering::Less);
    }
}
