//! Shared XML helpers: namespaces, attribute access, and a small element walker
//! for auxiliary parts (styles.xml, numbering.xml).

use std::io::BufRead;

use quick_xml::NsReader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::{Namespace, ResolveResult};

pub const W_TRANSITIONAL: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub const W_STRICT: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
pub const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
pub const MATH_TRANSITIONAL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
pub const MATH_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/math";

pub fn is_w(ns: &str) -> bool {
    ns == W_TRANSITIONAL || ns == W_STRICT
}

/// Value of a WordprocessingML attribute (`w:<local>`), matched by namespace URI.
/// Unprefixed attributes are accepted too.
pub fn w_attr<R>(reader: &NsReader<R>, e: &BytesStart, local: &str) -> Option<String> {
    for attr in e.attributes().flatten() {
        let (ns, name) = reader.resolver().resolve_attribute(attr.key);
        let in_w = match ns {
            ResolveResult::Bound(Namespace(ns)) => is_w(ns),
            ResolveResult::Unbound => true,
            ResolveResult::Unknown(_) => false,
        };
        if in_w && name.as_ref() == local {
            return attr
                .normalized_value(XmlVersion::Implicit1_0)
                .map(|v| v.into_owned())
                .ok();
        }
    }
    None
}

/// ST_OnOff: element alone (no `val`), `true`, `1`, `on` are true; `false`, `0`, `off` are false.
pub fn on_off(val: Option<&str>) -> bool {
    !matches!(val, Some("false" | "0" | "off"))
}

/// Markup Compatibility role of an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mc {
    None,
    AlternateContent,
    /// `first` is false for a second or later `mc:Choice`, which must be ignored.
    Choice {
        first: bool,
    },
    Fallback,
}

/// One open element as seen by [`walk`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// Local name when the element is in the WordprocessingML namespace.
    pub w: Option<String>,
    pub mc: Mc,
    /// For `mc:AlternateContent`: whether a Choice has been seen.
    choice_seen: bool,
}

/// Where an element sits relative to `mc:AlternateContent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alt {
    Normal,
    Choice,
    Fallback,
    /// Inside a second or later `mc:Choice`.
    Ignored,
}

/// Innermost Markup Compatibility context of a path.
pub fn alt_state(path: &[Node]) -> Alt {
    for node in path.iter().rev() {
        match node.mc {
            Mc::Choice { first: true } => return Alt::Choice,
            Mc::Choice { first: false } => return Alt::Ignored,
            Mc::Fallback => return Alt::Fallback,
            _ => {}
        }
    }
    Alt::Normal
}

/// WordprocessingML ancestry of a path with `mc:*` wrappers removed.
/// Elements of other namespaces appear as `None`.
pub fn w_path(path: &[Node]) -> Vec<Option<&str>> {
    path.iter()
        .filter(|n| n.mc == Mc::None)
        .map(|n| n.w.as_deref())
        .collect()
}

/// An element event reported by [`walk`]. `path` holds the ancestors of `node`.
pub enum Step<'a> {
    Open {
        path: &'a [Node],
        node: &'a Node,
        /// Reads a WordprocessingML attribute of the element.
        attr: &'a dyn Fn(&str) -> Option<String>,
    },
    Close {
        path: &'a [Node],
        node: &'a Node,
    },
}

/// Streams a part and reports every element opening and closing (empty elements
/// close immediately). On malformed XML the walk stops and the error is returned.
pub fn walk<R: BufRead>(reader: R, mut on: impl FnMut(Step)) -> Result<(), String> {
    let mut reader = NsReader::from_reader(reader);
    let mut buf = Vec::new();
    let mut stack: Vec<Node> = Vec::new();
    loop {
        let (ns, event) = reader
            .read_resolved_event_into(&mut buf)
            .map_err(|e| e.to_string())?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(event, Event::Empty(_));
                let local = e.local_name();
                let local = local.as_ref();
                let (w, mc) = match &ns {
                    ResolveResult::Bound(Namespace(n)) if is_w(n) => {
                        (Some(local.to_string()), Mc::None)
                    }
                    ResolveResult::Bound(Namespace(n)) if *n == MC => {
                        let mc = match local {
                            "AlternateContent" => Mc::AlternateContent,
                            "Choice" => {
                                let first = match stack.last_mut() {
                                    Some(parent) if parent.mc == Mc::AlternateContent => {
                                        let first = !parent.choice_seen;
                                        parent.choice_seen = true;
                                        first
                                    }
                                    _ => true,
                                };
                                Mc::Choice { first }
                            }
                            "Fallback" => Mc::Fallback,
                            _ => Mc::None,
                        };
                        (None, mc)
                    }
                    _ => (None, Mc::None),
                };
                let node = Node {
                    w,
                    mc,
                    choice_seen: false,
                };
                let attr = |name: &str| w_attr(&reader, e, name);
                on(Step::Open {
                    path: &stack,
                    node: &node,
                    attr: &attr,
                });
                if empty {
                    on(Step::Close {
                        path: &stack,
                        node: &node,
                    });
                } else {
                    stack.push(node);
                }
            }
            Event::End(_) => {
                if let Some(node) = stack.pop() {
                    on(Step::Close {
                        path: &stack,
                        node: &node,
                    });
                }
            }
            Event::Eof => {
                return if stack.is_empty() {
                    Ok(())
                } else {
                    Err("XML が途中で終わっています".to_string())
                };
            }
            _ => {}
        }
        buf.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_off_values() {
        assert!(on_off(None));
        assert!(on_off(Some("true")));
        assert!(on_off(Some("1")));
        assert!(on_off(Some("on")));
        assert!(!on_off(Some("false")));
        assert!(!on_off(Some("0")));
        assert!(!on_off(Some("off")));
    }

    #[test]
    fn walk_reports_paths_and_alternate_content() {
        let xml = format!(
            r#"<w:a xmlns:w="{W_TRANSITIONAL}" xmlns:mc="{MC}"><mc:AlternateContent><mc:Choice><w:b w:val="1"/></mc:Choice><mc:Choice><w:b w:val="2"/></mc:Choice><mc:Fallback><w:b w:val="3"/></mc:Fallback></mc:AlternateContent></w:a>"#
        );
        let mut seen = Vec::new();
        walk(xml.as_bytes(), |step| {
            if let Step::Open { path, node, attr } = step
                && node.w.as_deref() == Some("b")
            {
                let parents: Vec<_> = w_path(path).into_iter().flatten().collect();
                seen.push((
                    attr("val").unwrap_or_default(),
                    alt_state(path),
                    parents.join("/"),
                ));
            }
        })
        .unwrap();
        assert_eq!(
            seen,
            vec![
                ("1".to_string(), Alt::Choice, "a".to_string()),
                ("2".to_string(), Alt::Ignored, "a".to_string()),
                ("3".to_string(), Alt::Fallback, "a".to_string()),
            ]
        );
    }

    #[test]
    fn walk_reports_truncation() {
        let xml = format!(r#"<w:a xmlns:w="{W_TRANSITIONAL}"><w:b>"#);
        assert!(walk(xml.as_bytes(), |_| {}).is_err());
    }
}
