//! styles.xml: style chains for outline levels and numbering (SPEC §6.5 (a)(b)).
//!
//! Headings are never recognised by styleId, which depends on the UI language.

use std::collections::HashMap;
use std::io::BufRead;

use super::xml::{self, Alt, Step};

/// `basedOn` chains longer than this are cut (guards against cycles).
const MAX_CHAIN: usize = 20;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Style {
    pub id: String,
    pub kind: String,
    pub name: Option<String>,
    pub based_on: Option<String>,
    pub outline_lvl: Option<u8>,
    pub num_id: Option<u32>,
    pub ilvl: Option<u8>,
    pub page_break_before: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct Styles {
    by_id: HashMap<String, Style>,
    default_paragraph: Option<String>,
}

impl Styles {
    /// Parses styles.xml. On malformed XML, returns what was read so far with the error.
    pub fn parse<R: BufRead>(reader: R) -> (Styles, Option<String>) {
        let mut styles = Styles::default();
        let mut current: Option<Style> = None;
        let result = xml::walk(reader, |step| match step {
            Step::Open { path, node, attr } => {
                let alt = xml::alt_state(path);
                if matches!(alt, Alt::Fallback | Alt::Ignored) {
                    return;
                }
                let Some(name) = node.w.as_deref() else {
                    return;
                };
                let parents = xml::w_path(path);
                match (parents.as_slice(), name) {
                    ([Some("styles")], "style") => {
                        let id = attr("styleId").unwrap_or_default();
                        let kind = attr("type").unwrap_or_else(|| "paragraph".to_string());
                        if kind == "paragraph"
                            && attr("default").is_some_and(|d| xml::on_off(Some(&d)))
                        {
                            styles.default_paragraph = Some(id.clone());
                        }
                        current = Some(Style {
                            id,
                            kind,
                            ..Style::default()
                        });
                    }
                    ([.., Some("styles"), Some("style")], "name") => {
                        if let Some(s) = current.as_mut() {
                            s.name = attr("val");
                        }
                    }
                    ([.., Some("styles"), Some("style")], "basedOn") => {
                        if let Some(s) = current.as_mut() {
                            s.based_on = attr("val");
                        }
                    }
                    ([.., Some("style"), Some("pPr")], "outlineLvl") => {
                        if let Some(s) = current.as_mut() {
                            s.outline_lvl = attr("val").and_then(|v| v.parse().ok());
                        }
                    }
                    ([.., Some("style"), Some("pPr")], "pageBreakBefore") => {
                        if let Some(s) = current.as_mut() {
                            s.page_break_before = Some(xml::on_off(attr("val").as_deref()));
                        }
                    }
                    ([.., Some("style"), Some("pPr"), Some("numPr")], "numId") => {
                        if let Some(s) = current.as_mut() {
                            s.num_id = attr("val").and_then(|v| v.parse().ok());
                        }
                    }
                    ([.., Some("style"), Some("pPr"), Some("numPr")], "ilvl") => {
                        if let Some(s) = current.as_mut() {
                            s.ilvl = attr("val").and_then(|v| v.parse().ok());
                        }
                    }
                    _ => {}
                }
            }
            Step::Close { path, node } => {
                if node.w.as_deref() == Some("style")
                    && xml::w_path(path).last() == Some(&Some("styles"))
                    && let Some(style) = current.take()
                {
                    // The first definition of an id wins.
                    styles.by_id.entry(style.id.clone()).or_insert(style);
                }
            }
        });
        (styles, result.err())
    }

    pub fn get(&self, id: &str) -> Option<&Style> {
        self.by_id.get(id)
    }

    /// The style a paragraph actually uses: its `pStyle` when defined, else the default
    /// paragraph style.
    pub fn effective_id<'a>(&'a self, p_style: Option<&'a str>) -> Option<&'a str> {
        match p_style {
            Some(id) if self.by_id.contains_key(id) => Some(id),
            _ => self.default_paragraph.as_deref(),
        }
    }

    /// The style and its `basedOn` ancestors, nearest first.
    pub fn chain(&self, id: Option<&str>) -> Vec<&Style> {
        let mut out: Vec<&Style> = Vec::new();
        let mut next = id;
        while let Some(id) = next {
            if out.len() >= MAX_CHAIN || out.iter().any(|s| s.id == id) {
                break;
            }
            let Some(style) = self.by_id.get(id) else {
                break;
            };
            out.push(style);
            next = style.based_on.as_deref();
        }
        out
    }

    /// Outline level (0-based) from the style chain: the first `outlineLvl`, else a
    /// `heading N` style name anywhere in the chain.
    pub fn outline_level(&self, id: Option<&str>) -> Option<u8> {
        let chain = self.chain(id);
        chain.iter().find_map(|s| s.outline_lvl).or_else(|| {
            chain
                .iter()
                .find_map(|s| s.name.as_deref().and_then(heading_level))
        })
    }

    /// `numId` from the style chain.
    pub fn num_id(&self, id: Option<&str>) -> Option<u32> {
        self.chain(id).iter().find_map(|s| s.num_id)
    }

    /// `pageBreakBefore` from the style chain.
    pub fn page_break_before(&self, id: Option<&str>) -> Option<bool> {
        self.chain(id).iter().find_map(|s| s.page_break_before)
    }

    /// `ilvl` from the style chain.
    pub fn ilvl(&self, id: Option<&str>) -> Option<u8> {
        self.chain(id).iter().find_map(|s| s.ilvl)
    }

    /// Whether the style itself is one of the built-in TOC styles (`toc 1`–`toc 9`).
    pub fn is_toc(&self, id: Option<&str>) -> bool {
        id.and_then(|id| self.by_id.get(id))
            .and_then(|s| s.name.as_deref())
            .is_some_and(|n| numbered_name(n, "toc").is_some())
    }

    /// Resolves a `numStyleLink` target (a numbering style) to its `numId`.
    /// The value is a styleId; a style name is accepted as a fallback.
    pub fn numbering_style_num_id(&self, link: &str) -> Option<u32> {
        let id = if self.by_id.contains_key(link) {
            Some(link)
        } else {
            self.by_id
                .values()
                .find(|s| s.kind == "numbering" && s.name.as_deref() == Some(link))
                .map(|s| s.id.as_str())
        };
        self.num_id(id)
    }
}

/// `heading N` (case-insensitive, N = 1..9) → outline level N−1.
fn heading_level(name: &str) -> Option<u8> {
    numbered_name(name, "heading").map(|n| n - 1)
}

/// Parses `<prefix> N` with N in 1..=9, ignoring ASCII case.
fn numbered_name(name: &str, prefix: &str) -> Option<u8> {
    let lower = name.trim().to_ascii_lowercase();
    let rest = lower.strip_prefix(prefix)?.strip_prefix(' ')?;
    match rest.parse::<u8>() {
        Ok(n @ 1..=9) => Some(n),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::word::xml::W_TRANSITIONAL;

    fn parse(inner: &str) -> Styles {
        let xml = format!(r#"<w:styles xmlns:w="{W_TRANSITIONAL}">{inner}</w:styles>"#);
        let (styles, err) = Styles::parse(xml.as_bytes());
        assert!(err.is_none(), "{err:?}");
        styles
    }

    #[test]
    fn names() {
        assert_eq!(heading_level("heading 1"), Some(0));
        assert_eq!(heading_level("Heading 9"), Some(8));
        assert_eq!(heading_level("heading 10"), None);
        assert_eq!(heading_level("heading1"), None);
        assert_eq!(heading_level("見出し 1"), None);
        assert_eq!(numbered_name("TOC 3", "toc"), Some(3));
    }

    #[test]
    fn chain_and_cycle() {
        let s = parse(
            r#"<w:style w:type="paragraph" w:styleId="a"><w:name w:val="A"/><w:basedOn w:val="b"/></w:style>
               <w:style w:type="paragraph" w:styleId="b"><w:name w:val="B"/><w:basedOn w:val="a"/><w:pPr><w:outlineLvl w:val="2"/></w:pPr></w:style>"#,
        );
        let ids: Vec<_> = s.chain(Some("a")).iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
        assert_eq!(s.outline_level(Some("a")), Some(2));
    }

    #[test]
    fn outline_level_prefers_outline_over_name() {
        let s = parse(
            r#"<w:style w:type="paragraph" w:styleId="x"><w:name w:val="heading 3"/><w:basedOn w:val="y"/></w:style>
               <w:style w:type="paragraph" w:styleId="y"><w:name w:val="base"/><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:style>"#,
        );
        assert_eq!(s.outline_level(Some("x")), Some(0));
    }

    #[test]
    fn default_paragraph_style() {
        let s = parse(
            r#"<w:style w:type="character" w:default="1" w:styleId="c"/><w:style w:type="paragraph" w:default="1" w:styleId="n"><w:name w:val="Normal"/></w:style>"#,
        );
        assert_eq!(s.effective_id(None), Some("n"));
        assert_eq!(s.effective_id(Some("missing")), Some("n"));
    }

    #[test]
    fn table_style_ppr_is_not_the_style_ppr() {
        let s = parse(
            r#"<w:style w:type="table" w:styleId="t"><w:tblStylePr w:type="firstRow"><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:tblStylePr></w:style>"#,
        );
        assert_eq!(s.outline_level(Some("t")), None);
    }
}
