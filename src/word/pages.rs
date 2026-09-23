//! Page estimation along the body stream (SPEC §6.6).
//!
//! Both modes are tracked in one pass; the mode is chosen at the end, depending on
//! whether any `w:lastRenderedPageBreak` was seen.

use crate::model::PageMode;

pub const RENDERED: usize = 0;
pub const EXPLICIT: usize = 1;

/// Page of a paragraph start and the char positions of breaks inside it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageInfo {
    pub start: u32,
    pub breaks: Vec<usize>,
}

/// Page information for both modes, indexed by [`RENDERED`] / [`EXPLICIT`].
pub type Pages = [PageInfo; 2];

/// A table row. Every cell starts on the row's start page; the row ends on the
/// furthest page reached by any cell (Word repeats a rendered break in each cell).
#[derive(Debug, Clone, Copy)]
struct Row {
    start: u32,
    end: u32,
}

#[derive(Debug)]
pub struct PageTracker {
    page: [u32; 2],
    rows: Vec<[Row; 2]>,
    /// Explicit mode: nothing has been laid out on the current page yet.
    at_top: bool,
    /// Explicit mode: the previous paragraph ended a section on a new page.
    section_pending: bool,
    rendered_seen: bool,
}

impl Default for PageTracker {
    fn default() -> Self {
        PageTracker {
            page: [1, 1],
            rows: Vec::new(),
            at_top: true,
            section_pending: false,
            rendered_seen: false,
        }
    }
}

impl PageTracker {
    pub fn mode(&self) -> PageMode {
        if self.rendered_seen {
            PageMode::Rendered
        } else {
            PageMode::Explicit
        }
    }

    pub fn current(&self) -> [u32; 2] {
        self.page
    }

    pub fn at_top(&self) -> bool {
        self.at_top
    }

    /// A break that only takes effect when something is already on the page
    /// (`pageBreakBefore`, next-page section breaks).
    fn soft_break(&mut self) {
        if !self.at_top {
            self.page[EXPLICIT] += 1;
            self.at_top = true;
        }
    }

    /// Starts a body-stream paragraph and returns its start pages.
    pub fn paragraph_start(&mut self, page_break_before: bool) -> [u32; 2] {
        if std::mem::take(&mut self.section_pending) {
            self.soft_break();
        }
        if page_break_before {
            self.soft_break();
        }
        self.page
    }

    /// Ends a body-stream paragraph.
    pub fn paragraph_end(
        &mut self,
        started_at_top: bool,
        had_page_break: bool,
        section_break: bool,
    ) {
        // An empty paragraph at the top of a page still occupies a line.
        if started_at_top && !had_page_break {
            self.at_top = false;
        }
        if section_break {
            self.section_pending = true;
        }
    }

    /// Some text was laid out.
    pub fn text(&mut self) {
        self.at_top = false;
    }

    pub fn rendered_break(&mut self) {
        self.page[RENDERED] += 1;
        self.rendered_seen = true;
    }

    /// `w:br w:type="page"`: always starts a new page.
    pub fn explicit_break(&mut self) {
        self.page[EXPLICIT] += 1;
        self.at_top = true;
    }

    pub fn row_start(&mut self) {
        let row = |p: u32| Row { start: p, end: p };
        self.rows
            .push([row(self.page[RENDERED]), row(self.page[EXPLICIT])]);
    }

    pub fn cell_start(&mut self) {
        if let Some(rows) = self.rows.last() {
            for (page, row) in self.page.iter_mut().zip(rows) {
                *page = row.start;
            }
        }
    }

    pub fn cell_end(&mut self) {
        if let Some(rows) = self.rows.last_mut() {
            for (page, row) in self.page.iter().zip(rows.iter_mut()) {
                row.end = row.end.max(*page);
            }
        }
    }

    pub fn row_end(&mut self) {
        if let Some(rows) = self.rows.pop() {
            for (page, row) in self.page.iter_mut().zip(rows) {
                *page = row.end.max(*page);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_breaks_are_ignored_at_page_top() {
        let mut t = PageTracker::default();
        // First paragraph: pageBreakBefore does nothing at the top of page 1.
        assert_eq!(t.paragraph_start(true)[EXPLICIT], 1);
        t.text();
        t.explicit_break();
        t.paragraph_end(true, true, false);
        // Right after a hard break the next pageBreakBefore is absorbed.
        assert_eq!(t.paragraph_start(true)[EXPLICIT], 2);
        t.paragraph_end(true, false, true);
        // The section break of the (empty) previous paragraph counts: it held a line.
        assert_eq!(t.paragraph_start(false)[EXPLICIT], 3);
    }

    #[test]
    fn rows_advance_by_the_furthest_cell() {
        let mut t = PageTracker::default();
        t.row_start();
        t.cell_start();
        t.rendered_break();
        t.cell_end();
        t.cell_start();
        assert_eq!(t.current()[RENDERED], 1);
        t.rendered_break();
        t.cell_end();
        t.row_end();
        assert_eq!(t.current()[RENDERED], 2);
        assert_eq!(t.mode(), PageMode::Rendered);
    }
}
