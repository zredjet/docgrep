//! Output formats. Shared helpers for context windows and char slicing.

pub mod json;
pub mod pretty;

use crate::model::{Format, Match, PageMode, TextUnit};

/// Everything the output layer needs to render one file.
#[derive(Debug)]
pub struct FileHits {
    pub display: String,
    pub format: Format,
    pub page_mode: Option<PageMode>,
    pub units: Vec<TextUnit>,
    pub matches: Vec<Match>,
}

#[derive(Debug, Clone, Copy)]
pub struct ContextOpts {
    /// Chars shown on each side of a match.
    pub context: usize,
    /// Show the whole paragraph instead of a window.
    pub paragraph: bool,
}

/// A display entry: one window of one unit, possibly covering several matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub unit_index: usize,
    pub start: usize,
    pub end: usize,
    /// Matches inside the window as `(start, end)` char offsets into the unit.
    pub matches: Vec<(usize, usize)>,
}

/// Window of chars to show around `[start, end)` in a unit of `len` chars.
pub fn window(start: usize, end: usize, len: usize, opts: ContextOpts) -> (usize, usize) {
    if opts.paragraph {
        (0, len)
    } else {
        (
            start.saturating_sub(opts.context),
            end.saturating_add(opts.context).min(len),
        )
    }
}

/// Groups matches into display entries. Windows in the same unit that overlap or
/// touch are merged; all matches keep their own highlight.
pub fn build_entries(matches: &[Match], units: &[TextUnit], opts: ContextOpts) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for m in matches {
        let len = units
            .get(m.unit_index)
            .map(|u| u.text.chars().count())
            .unwrap_or(0);
        let (start, end) = window(m.start, m.end, len, opts);
        if let Some(last) = entries.last_mut()
            && last.unit_index == m.unit_index
            && start <= last.end
        {
            last.end = last.end.max(end);
            last.matches.push((m.start, m.end));
            continue;
        }
        entries.push(Entry {
            unit_index: m.unit_index,
            start,
            end,
            matches: vec![(m.start, m.end)],
        });
    }
    entries
}

/// Char-indexed view of a string for safe slicing on char boundaries.
pub struct CharIndex<'a> {
    text: &'a str,
    /// Byte offset of each char, plus `text.len()` at the end.
    offsets: Vec<usize>,
}

impl<'a> CharIndex<'a> {
    pub fn new(text: &'a str) -> Self {
        let mut offsets: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
        offsets.push(text.len());
        CharIndex { text, offsets }
    }

    pub fn len(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Slice by char positions; out-of-range positions are clamped.
    pub fn slice(&self, start: usize, end: usize) -> &'a str {
        let byte = |i: usize| self.offsets.get(i).copied().unwrap_or(self.text.len());
        let (s, e) = (byte(start), byte(end));
        if s >= e {
            return "";
        }
        self.text.get(s..e).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Location, Part};

    fn unit(text: &str) -> TextUnit {
        TextUnit::new(text.to_string(), Location::new(Part::Body))
    }

    fn m(unit_index: usize, start: usize, end: usize) -> Match {
        Match {
            unit_index,
            start,
            end,
        }
    }

    const C2: ContextOpts = ContextOpts {
        context: 2,
        paragraph: false,
    };

    #[test]
    fn window_is_clamped() {
        assert_eq!(window(5, 7, 10, C2), (3, 9));
        assert_eq!(window(1, 2, 3, C2), (0, 3));
        let p = ContextOpts {
            context: 2,
            paragraph: true,
        };
        assert_eq!(window(5, 7, 10, p), (0, 10));
    }

    #[test]
    fn overlapping_and_touching_windows_merge() {
        let units = vec![unit("0123456789abcdefghij")];
        // windows (0,5) and (4,9) overlap
        let e = build_entries(&[m(0, 1, 3), m(0, 6, 7)], &units, C2);
        assert_eq!(e.len(), 1);
        assert_eq!((e[0].start, e[0].end), (0, 9));
        assert_eq!(e[0].matches, vec![(1, 3), (6, 7)]);
        // windows (0,5) and (5,10) touch
        let e = build_entries(&[m(0, 1, 3), m(0, 7, 8)], &units, C2);
        assert_eq!(e.len(), 1);
        // windows (0,5) and (6,11) are apart
        let e = build_entries(&[m(0, 1, 3), m(0, 8, 9)], &units, C2);
        assert_eq!(e.len(), 2);
    }

    #[test]
    fn different_units_never_merge() {
        let units = vec![unit("abc"), unit("abc")];
        let e = build_entries(&[m(0, 0, 1), m(1, 0, 1)], &units, C2);
        assert_eq!(e.len(), 2);
    }

    #[test]
    fn paragraph_mode_merges_all_matches_of_a_unit() {
        let units = vec![unit(&"x".repeat(200))];
        let p = ContextOpts {
            context: 0,
            paragraph: true,
        };
        let e = build_entries(&[m(0, 0, 1), m(0, 199, 200)], &units, p);
        assert_eq!(e.len(), 1);
        assert_eq!((e[0].start, e[0].end), (0, 200));
    }

    #[test]
    fn zero_context_merges_only_adjacent_matches() {
        let units = vec![unit("abab")];
        let c0 = ContextOpts {
            context: 0,
            paragraph: false,
        };
        let e = build_entries(&[m(0, 0, 2), m(0, 2, 4)], &units, c0);
        assert_eq!(e.len(), 1);
    }

    #[test]
    fn char_index_slices_on_char_boundaries() {
        let ci = CharIndex::new("あい😀う");
        assert_eq!(ci.len(), 4);
        assert_eq!(ci.slice(1, 3), "い😀");
        assert_eq!(ci.slice(3, 99), "う");
        assert_eq!(ci.slice(3, 1), "");
    }
}
