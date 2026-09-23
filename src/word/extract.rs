//! Streaming extraction of WordprocessingML paragraphs into text units (SPEC §6.3),
//! with headings (SPEC §6.5) and estimated pages (SPEC §6.6) attached to body-stream
//! paragraphs.

use std::io::BufRead;

use quick_xml::NsReader;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::{Namespace, ResolveResult};

use super::headings::{DocContext, HeadingTracker, ParaProps};
use super::pages::{EXPLICIT, PageInfo, PageTracker, Pages, RENDERED};
use super::xml::{MATH_STRICT, MATH_TRANSITIONAL, MC, is_w, on_off, w_attr};
use crate::error::FileError;
use crate::model::{Heading, Location, PageMode, Part, TextUnit};

/// Which WordprocessingML part is being read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartKind {
    Body,
    Footnotes,
    Endnotes,
    Comments,
    Header,
    Footer,
}

/// Kind of a reference from the body to a note or comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefKind {
    Footnote,
    Endnote,
    Comment,
}

/// Where a footnote, endnote or comment is referenced in the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteRef {
    pub kind: RefKind,
    pub id: String,
    pub page: u32,
    pub heading: Option<Heading>,
}

/// A reference while the body is being read: pages for both modes.
#[derive(Debug)]
struct RefRecord {
    kind: RefKind,
    id: String,
    pages: [u32; 2],
    heading: Option<Heading>,
}

/// Elements we react to. Everything else is descended into transparently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tag {
    P,
    Tbl,
    Tr,
    Tc,
    T,
    MathT,
    Tab,
    Br,
    Cr,
    NoBreakHyphen,
    Sym,
    FldChar,
    LastRenderedPageBreak,
    /// `w:footnote` / `w:endnote` / `w:comment` in their own parts.
    NoteContainer,
    Reference(RefKind),
    /// Paragraph properties: skipped for text, but a paragraph's own `w:pPr` is read.
    PPr,
    /// `w:del` / `w:moveFrom`: text below is not part of the current revision.
    Deleted,
    TxbxContent,
    AlternateContent,
    Choice,
    /// Subtrees that never contribute text: property elements, field codes,
    /// deleted text, ruby text, `mc:Fallback`.
    Skip,
    Other,
}

fn classify(ns: &ResolveResult, local: &str) -> Tag {
    let &ResolveResult::Bound(Namespace(ns)) = ns else {
        return Tag::Other;
    };
    if is_w(ns) {
        match local {
            "p" => Tag::P,
            "tbl" => Tag::Tbl,
            "tr" => Tag::Tr,
            "tc" => Tag::Tc,
            "t" => Tag::T,
            "tab" | "ptab" => Tag::Tab,
            "br" => Tag::Br,
            "cr" => Tag::Cr,
            "noBreakHyphen" => Tag::NoBreakHyphen,
            "sym" => Tag::Sym,
            "fldChar" => Tag::FldChar,
            "lastRenderedPageBreak" => Tag::LastRenderedPageBreak,
            "footnote" | "endnote" | "comment" => Tag::NoteContainer,
            "footnoteReference" => Tag::Reference(RefKind::Footnote),
            "endnoteReference" => Tag::Reference(RefKind::Endnote),
            "commentReference" => Tag::Reference(RefKind::Comment),
            "del" | "moveFrom" => Tag::Deleted,
            "txbxContent" => Tag::TxbxContent,
            "pPr" => Tag::PPr,
            "rPr" | "tblPr" | "trPr" | "tcPr" | "sectPr" | "tblGrid" | "tblPrEx" | "sdtPr"
            | "sdtEndPr" | "rubyPr" | "customXmlPr" | "smartTagPr" | "fldData" | "instrText"
            | "delInstrText" | "delText" | "rt" => Tag::Skip,
            _ => Tag::Other,
        }
    } else if ns == MC {
        match local {
            "AlternateContent" => Tag::AlternateContent,
            "Choice" => Tag::Choice,
            "Fallback" => Tag::Skip,
            _ => Tag::Other,
        }
    } else if ns == MATH_TRANSITIONAL || ns == MATH_STRICT {
        match local {
            "t" => Tag::MathT,
            _ => Tag::Other,
        }
    } else {
        Tag::Other
    }
}

/// What to undo when an element closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    Paragraph,
    Table,
    /// `w:tr` / `w:tc`; `true` when in the main story (they drive page tracking).
    Row(bool),
    Cell(bool),
    Container,
    Text,
    Deleted,
    TextBox,
    AlternateContent,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldState {
    Code,
    Result,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoryKind {
    Main,
    TextBox,
}

#[derive(Debug, Clone, Copy)]
struct TableCtx {
    index: u32,
    row: u32,
    col: u32,
}

/// A finished unit with page candidates for both modes, resolved at the end of the part.
#[derive(Debug)]
struct Unit {
    unit: TextUnit,
    pages: Option<Pages>,
}

/// Which child group of the paragraph's `w:pPr` is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PPrGroup {
    NumPr,
    SectPr,
}

#[derive(Debug, Default)]
struct ParaBuilder {
    text: String,
    char_len: usize,
    props: ParaProps,
    /// Page state is set up lazily, once the paragraph's `w:pPr` has been read.
    started: bool,
    start_pages: [u32; 2],
    started_at_top: bool,
    breaks: [Vec<usize>; 2],
    /// Text box paragraphs anchored in this paragraph, emitted after it.
    anchored: Vec<Unit>,
    /// References (indexes into the extractor's list) that take this paragraph's heading.
    refs: Vec<usize>,
}

/// An independent flow of text (the main body, or one text box).
/// Field state and table nesting do not leak between stories.
#[derive(Debug)]
struct Story {
    kind: StoryKind,
    fields: Vec<FieldState>,
    paras: Vec<ParaBuilder>,
    tables: Vec<TableCtx>,
    out: Vec<Unit>,
    /// Text boxes: pages (both modes) at the anchor position in the body.
    anchor_pages: Option<[u32; 2]>,
    /// References made outside any paragraph of this story.
    pending_refs: Vec<usize>,
}

impl Story {
    fn new(kind: StoryKind, anchor_pages: Option<[u32; 2]>) -> Self {
        Story {
            kind,
            fields: Vec::new(),
            paras: Vec::new(),
            tables: Vec::new(),
            out: Vec::new(),
            anchor_pages,
            pending_refs: Vec::new(),
        }
    }

    fn fields_allow_text(&self) -> bool {
        self.fields.iter().all(|f| *f == FieldState::Result)
    }

    fn part(&self) -> Part {
        match self.kind {
            StoryKind::TextBox => Part::TextBox,
            StoryKind::Main => match self.tables.last() {
                Some(t) => Part::Table {
                    index: t.index,
                    row: t.row,
                    col: t.col,
                },
                None => Part::Body,
            },
        }
    }
}

struct Extractor<'a> {
    ctx: &'a DocContext,
    kind: PartKind,
    /// The footnote / endnote / comment being read (notes and comments parts).
    container: Option<Part>,
    refs: Vec<RefRecord>,
    headings: HeadingTracker,
    /// Reading the current paragraph's own `w:pPr` (inside a skipped subtree).
    in_ppr: bool,
    /// Open child group of the current paragraph's `w:pPr`.
    ppr_group: Option<PPrGroup>,
    pages: PageTracker,
    /// Main story at index 0, open text boxes above it.
    stories: Vec<Story>,
    frames: Vec<Frame>,
    /// Depth inside a skipped subtree (0 = not skipping).
    skip: usize,
    /// Depth of `w:del` / `w:moveFrom` nesting.
    deleted: usize,
    in_text: bool,
    table_counter: u32,
    /// For each open `mc:AlternateContent`: whether a Choice was already taken.
    alternates: Vec<bool>,
}

/// Result of extracting one story part.
pub struct PartText {
    pub units: Vec<TextUnit>,
    /// Page mode of the body; `None` for other parts.
    pub page_mode: Option<PageMode>,
    /// References to notes and comments found in the body.
    pub refs: Vec<NoteRef>,
    /// Set when the XML broke midway; `units` holds paragraphs completed before that.
    pub error: Option<FileError>,
}

/// Extracts every paragraph of a WordprocessingML part in document order.
///
/// Headings, pages and note references are only tracked for [`PartKind::Body`].
/// In other parts every unit is labelled with the part itself (tables and text
/// boxes included).
pub fn extract_part<R: BufRead>(
    reader: R,
    part_name: &str,
    ctx: &DocContext,
    kind: PartKind,
) -> PartText {
    let mut ex = Extractor {
        ctx,
        kind,
        container: None,
        refs: Vec::new(),
        headings: HeadingTracker::default(),
        in_ppr: false,
        ppr_group: None,
        pages: PageTracker::default(),
        stories: vec![Story::new(StoryKind::Main, None)],
        frames: Vec::new(),
        skip: 0,
        deleted: 0,
        in_text: false,
        table_counter: 0,
        alternates: Vec::new(),
    };
    let mut reader = NsReader::from_reader(reader);
    let mut buf = Vec::new();
    let mut error = None;
    loop {
        match reader.read_resolved_event_into(&mut buf) {
            Ok((ns, event)) => match event {
                Event::Start(e) => {
                    if ex.skip > 0 {
                        if ex.in_ppr {
                            let name = e.local_name();
                            let local = w_local(&ns, name.as_ref());
                            ex.ppr_child(&reader, &e, local, true);
                        }
                        ex.skip += 1;
                    } else {
                        let tag = classify(&ns, e.local_name().as_ref());
                        let attrs = Attrs::read(&reader, &e, tag);
                        match ex.start(tag, &attrs) {
                            Some(frame) => ex.frames.push(frame),
                            None => ex.skip = 1,
                        }
                    }
                }
                Event::Empty(e) => {
                    if ex.skip > 0 {
                        if ex.in_ppr {
                            let name = e.local_name();
                            let local = w_local(&ns, name.as_ref());
                            ex.ppr_child(&reader, &e, local, false);
                        }
                    } else {
                        let tag = classify(&ns, e.local_name().as_ref());
                        let attrs = Attrs::read(&reader, &e, tag);
                        match ex.start(tag, &attrs) {
                            Some(frame) => ex.end(frame),
                            // An empty skipped element (e.g. `<w:pPr/>`) has no subtree.
                            None => ex.in_ppr = false,
                        }
                    }
                }
                Event::End(_) => {
                    if ex.skip > 0 {
                        ex.skip -= 1;
                        match ex.skip {
                            0 => ex.in_ppr = false,
                            1 => ex.ppr_group = None,
                            _ => {}
                        }
                    } else if let Some(frame) = ex.frames.pop() {
                        ex.end(frame);
                    }
                }
                Event::Text(t) => ex.text(&t.xml10_content()),
                Event::CData(t) => ex.text(&t.xml10_content()),
                Event::GeneralRef(r) => {
                    if let Ok(Some(c)) = r.resolve_char_ref() {
                        ex.text(c.encode_utf8(&mut [0u8; 4]));
                    } else if let Some(s) = resolve_predefined_entity(&r) {
                        ex.text(s);
                    }
                }
                Event::Eof => {
                    if !ex.frames.is_empty() || ex.skip > 0 {
                        error = Some(FileError::Xml {
                            part: part_name.to_string(),
                            message: "XML が途中で終わっています".to_string(),
                        });
                    }
                    break;
                }
                _ => {}
            },
            Err(e) => {
                error = Some(FileError::Xml {
                    part: part_name.to_string(),
                    message: e.to_string(),
                });
                break;
            }
        }
        buf.clear();
    }
    let page_mode = ex.pages.mode();
    let mode = match page_mode {
        PageMode::Rendered => RENDERED,
        PageMode::Explicit => EXPLICIT,
    };
    let refs = ex
        .refs
        .into_iter()
        .map(|r| NoteRef {
            kind: r.kind,
            id: r.id,
            page: r.pages.get(mode).copied().unwrap_or(1),
            heading: r.heading,
        })
        .collect();
    let body = kind == PartKind::Body;
    let units = ex
        .stories
        .into_iter()
        .next()
        .map(|s| s.out)
        .unwrap_or_default()
        .into_iter()
        .map(|Unit { mut unit, pages }| {
            if let Some(info) = pages.and_then(|p| p.into_iter().nth(mode)) {
                unit.location.page_at_start = Some(info.start);
                unit.location.page_mode = Some(page_mode);
                unit.page_breaks = info.breaks;
            }
            unit
        })
        .collect();
    PartText {
        units,
        page_mode: body.then_some(page_mode),
        refs,
        error,
    }
}

/// Local name of a WordprocessingML element; `None` for other namespaces.
fn w_local<'n>(ns: &ResolveResult, local: &'n str) -> Option<&'n str> {
    match ns {
        ResolveResult::Bound(Namespace(n)) if is_w(n) => Some(local),
        _ => None,
    }
}

/// The few attribute values we need, read with namespace resolution.
#[derive(Debug, Default)]
struct Attrs {
    /// `w:type` (br, footnote, endnote) or `w:fldCharType` (fldChar).
    kind: Option<String>,
    /// `w:char` (sym).
    char_code: Option<String>,
    /// `w:id` (notes, comments and their references).
    id: Option<String>,
    /// `w:author` (comment).
    author: Option<String>,
}

impl Attrs {
    fn read<R>(reader: &NsReader<R>, e: &BytesStart, tag: Tag) -> Attrs {
        match tag {
            Tag::Br => Attrs {
                kind: w_attr(reader, e, "type"),
                ..Attrs::default()
            },
            Tag::FldChar => Attrs {
                kind: w_attr(reader, e, "fldCharType"),
                ..Attrs::default()
            },
            Tag::Sym => Attrs {
                char_code: w_attr(reader, e, "char"),
                ..Attrs::default()
            },
            Tag::NoteContainer => Attrs {
                kind: w_attr(reader, e, "type"),
                id: w_attr(reader, e, "id"),
                author: w_attr(reader, e, "author"),
                ..Attrs::default()
            },
            Tag::Reference(_) => Attrs {
                id: w_attr(reader, e, "id"),
                ..Attrs::default()
            },
            _ => Attrs::default(),
        }
    }
}

impl Extractor<'_> {
    fn story(&mut self) -> Option<&mut Story> {
        self.stories.last_mut()
    }

    fn text_allowed(&self) -> bool {
        self.deleted == 0 && self.stories.last().is_some_and(Story::fields_allow_text)
    }

    /// Whether we are in the main story of the body part (not a text box, not
    /// another part). Pages, headings and table numbers are tracked only here.
    fn in_main(&self) -> bool {
        self.kind == PartKind::Body && self.stories.len() == 1
    }

    /// Sets up the current paragraph's start page. Called before its first content,
    /// when its `w:pPr` (with `pageBreakBefore`) has already been read.
    fn start_paragraph_pages(&mut self) {
        let ctx = self.ctx;
        let in_main = self.in_main();
        let Some(st) = self.stories.last_mut() else {
            return;
        };
        let in_table = !st.tables.is_empty();
        let Some(para) = st.paras.last_mut() else {
            return;
        };
        if para.started {
            return;
        }
        para.started = true;
        if !in_main {
            return;
        }
        let styles = &ctx.styles;
        let style_id = styles.effective_id(para.props.style.as_deref());
        let page_break_before = para
            .props
            .page_break_before
            .or_else(|| styles.page_break_before(style_id))
            .unwrap_or(false);
        // Word ignores "page break before" inside tables.
        para.start_pages = self.pages.paragraph_start(page_break_before && !in_table);
        para.started_at_top = self.pages.at_top();
    }

    /// Records a page break at the current position of the paragraph (main story only).
    fn page_break(&mut self, mode: usize) {
        if !self.in_main() {
            return;
        }
        self.start_paragraph_pages();
        let Some(para) = self.stories.last_mut().and_then(|st| st.paras.last_mut()) else {
            return;
        };
        match mode {
            RENDERED => self.pages.rendered_break(),
            _ => self.pages.explicit_break(),
        }
        if let Some(breaks) = para.breaks.get_mut(mode) {
            breaks.push(para.char_len);
        }
    }

    fn push_str(&mut self, s: &str) {
        if !self.text_allowed() || s.is_empty() {
            return;
        }
        self.start_paragraph_pages();
        let in_main = self.in_main();
        if let Some(para) = self.story().and_then(|st| st.paras.last_mut()) {
            para.text.push_str(s);
            para.char_len += s.chars().count();
            if in_main {
                self.pages.text();
            }
        }
    }

    fn push_char(&mut self, c: char) {
        self.push_str(c.encode_utf8(&mut [0u8; 4]));
    }

    fn text(&mut self, s: &str) {
        if self.in_text {
            self.push_str(s);
        }
    }

    /// Records `pStyle`, `outlineLvl`, `numPr`, `pageBreakBefore` and `sectPr` of the
    /// current paragraph's `w:pPr`.
    /// `self.skip` is the depth below `w:pPr` (1 = direct child).
    fn ppr_child<R>(
        &mut self,
        reader: &NsReader<R>,
        e: &BytesStart,
        local: Option<&str>,
        is_start: bool,
    ) {
        let depth = self.skip;
        let group = self.ppr_group;
        let Some(para) = self.story().and_then(|st| st.paras.last_mut()) else {
            return;
        };
        let val = || w_attr(reader, e, "val");
        let mut open = None;
        match (depth, local, group) {
            (1, Some("pStyle"), _) => para.props.style = val(),
            (1, Some("outlineLvl"), _) => {
                para.props.outline_lvl = val().and_then(|v| v.parse().ok());
            }
            (1, Some("pageBreakBefore"), _) => {
                para.props.page_break_before = Some(on_off(val().as_deref()));
            }
            (1, Some("numPr"), _) => open = Some(PPrGroup::NumPr),
            (1, Some("sectPr"), _) => {
                // A section break without w:type starts a new page.
                para.props.section_break = true;
                open = Some(PPrGroup::SectPr);
            }
            (2, Some("numId"), Some(PPrGroup::NumPr)) => {
                para.props.num_id = val().and_then(|v| v.parse().ok());
            }
            (2, Some("ilvl"), Some(PPrGroup::NumPr)) => {
                para.props.ilvl = val().and_then(|v| v.parse().ok());
            }
            (2, Some("type"), Some(PPrGroup::SectPr)) => {
                para.props.section_break = matches!(
                    val().as_deref(),
                    None | Some("nextPage" | "oddPage" | "evenPage")
                );
            }
            _ => {}
        }
        if is_start && open.is_some() {
            self.ppr_group = open;
        }
    }

    /// Handles an opening tag. Returns `None` if the subtree must be skipped.
    fn start(&mut self, tag: Tag, attrs: &Attrs) -> Option<Frame> {
        let frame = match tag {
            Tag::Skip => return None,
            Tag::PPr => {
                // Only a paragraph's own properties matter (not pPrChange etc.).
                self.in_ppr = self.frames.last() == Some(&Frame::Paragraph);
                return None;
            }
            Tag::P => {
                if let Some(st) = self.story() {
                    st.paras.push(ParaBuilder::default());
                }
                Frame::Paragraph
            }
            Tag::Tbl => {
                let index = if self.in_main() {
                    self.table_counter += 1;
                    self.table_counter
                } else {
                    0
                };
                if let Some(st) = self.story() {
                    st.tables.push(TableCtx {
                        index,
                        row: 0,
                        col: 0,
                    });
                }
                Frame::Table
            }
            Tag::Tr => {
                if let Some(t) = self.story().and_then(|s| s.tables.last_mut()) {
                    t.row += 1;
                    t.col = 0;
                }
                let main = self.in_main();
                if main {
                    self.pages.row_start();
                }
                Frame::Row(main)
            }
            Tag::Tc => {
                if let Some(t) = self.story().and_then(|s| s.tables.last_mut()) {
                    t.col += 1;
                }
                let main = self.in_main();
                if main {
                    self.pages.cell_start();
                }
                Frame::Cell(main)
            }
            Tag::T | Tag::MathT => {
                self.in_text = true;
                Frame::Text
            }
            Tag::Tab => {
                self.push_char('\t');
                Frame::Other
            }
            Tag::Br => {
                self.push_char('\n');
                if attrs.kind.as_deref() == Some("page") && self.text_allowed() {
                    self.page_break(EXPLICIT);
                }
                Frame::Other
            }
            Tag::Cr => {
                self.push_char('\n');
                Frame::Other
            }
            Tag::NoteContainer => {
                // Separator "notes" hold only the separator line.
                if matches!(
                    attrs.kind.as_deref(),
                    Some("separator" | "continuationSeparator" | "continuationNotice")
                ) {
                    return None;
                }
                let id = attrs.id.clone().unwrap_or_default();
                self.container = match self.kind {
                    PartKind::Footnotes => Some(Part::Footnote { id }),
                    PartKind::Endnotes => Some(Part::Endnote { id }),
                    PartKind::Comments => Some(Part::Comment {
                        id,
                        author: attrs.author.clone().unwrap_or_default(),
                    }),
                    _ => None,
                };
                Frame::Container
            }
            Tag::Reference(kind) => {
                self.reference(kind, attrs.id.clone());
                Frame::Other
            }
            Tag::LastRenderedPageBreak => {
                // Counted even inside deletions and field codes: it records the
                // layout at save time.
                self.page_break(RENDERED);
                Frame::Other
            }
            Tag::NoBreakHyphen => {
                self.push_char('-');
                Frame::Other
            }
            Tag::Sym => {
                if let Some(c) = attrs
                    .char_code
                    .as_deref()
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .and_then(char::from_u32)
                {
                    self.push_char(c);
                }
                Frame::Other
            }
            Tag::FldChar => {
                if self.deleted == 0 {
                    self.field_char(attrs.kind.as_deref());
                }
                Frame::Other
            }
            Tag::Deleted => {
                self.deleted += 1;
                Frame::Deleted
            }
            Tag::TxbxContent => {
                let anchor_pages = if self.in_main() {
                    self.start_paragraph_pages();
                    Some(self.pages.current())
                } else {
                    self.stories.last().and_then(|s| s.anchor_pages)
                };
                self.stories
                    .push(Story::new(StoryKind::TextBox, anchor_pages));
                Frame::TextBox
            }
            Tag::AlternateContent => {
                self.alternates.push(false);
                Frame::AlternateContent
            }
            Tag::Choice => match self.alternates.last_mut() {
                Some(taken) if *taken => return None,
                Some(taken) => {
                    *taken = true;
                    Frame::Other
                }
                None => Frame::Other,
            },
            Tag::Other => Frame::Other,
        };
        Some(frame)
    }

    fn field_char(&mut self, kind: Option<&str>) {
        let Some(st) = self.story() else { return };
        match kind {
            Some("begin") => st.fields.push(FieldState::Code),
            Some("separate") => {
                if let Some(top) = st.fields.last_mut() {
                    *top = FieldState::Result;
                }
            }
            Some("end") => {
                st.fields.pop();
            }
            _ => {}
        }
    }

    fn end(&mut self, frame: Frame) {
        match frame {
            Frame::Paragraph => self.end_paragraph(),
            Frame::Table => {
                if let Some(st) = self.story() {
                    st.tables.pop();
                }
            }
            Frame::Row(main) => {
                if main {
                    self.pages.row_end();
                }
            }
            Frame::Cell(main) => {
                if main {
                    self.pages.cell_end();
                }
            }
            Frame::Container => self.container = None,
            Frame::Text => self.in_text = false,
            Frame::Deleted => self.deleted = self.deleted.saturating_sub(1),
            Frame::TextBox => self.end_textbox(),
            Frame::AlternateContent => {
                self.alternates.pop();
            }
            Frame::Other => {}
        }
    }

    /// Records a footnote / endnote / comment reference in the body.
    fn reference(&mut self, kind: RefKind, id: Option<String>) {
        if self.kind != PartKind::Body {
            return;
        }
        let Some(id) = id else { return };
        let pages = if self.in_main() {
            self.start_paragraph_pages();
            self.pages.current()
        } else {
            match self.stories.last().and_then(|s| s.anchor_pages) {
                Some(p) => p,
                None => self.pages.current(),
            }
        };
        let index = self.refs.len();
        self.refs.push(RefRecord {
            kind,
            id,
            pages,
            heading: None,
        });
        if let Some(st) = self.stories.last_mut() {
            match st.paras.last_mut() {
                Some(para) => para.refs.push(index),
                None => st.pending_refs.push(index),
            }
        }
    }

    /// Part of a paragraph that just ended in the current story.
    fn paragraph_part(&self, st: &Story, props: &ParaProps) -> Option<Part> {
        match self.kind {
            PartKind::Header => Some(Part::Header),
            PartKind::Footer => Some(Part::Footer),
            PartKind::Footnotes | PartKind::Endnotes | PartKind::Comments => self.container.clone(),
            PartKind::Body => {
                let part = st.part();
                let styles = &self.ctx.styles;
                let toc = styles.is_toc(styles.effective_id(props.style.as_deref()));
                Some(if part == Part::Body && toc {
                    Part::Toc
                } else {
                    part
                })
            }
        }
    }

    fn end_paragraph(&mut self) {
        self.start_paragraph_pages();
        let ctx = self.ctx;
        let in_main = self.in_main();
        let Some(mut para) = self.stories.last_mut().and_then(|st| st.paras.pop()) else {
            return;
        };
        let Some(st) = self.stories.last() else {
            return;
        };
        let Some(part) = self.paragraph_part(st, &para.props) else {
            // A paragraph outside any note or comment: nothing to label it with.
            return;
        };
        let mut location = Location::new(part);
        let pages = if in_main {
            // Text boxes and references in this paragraph take its heading.
            let heading = self.headings.paragraph(ctx, &para.props, &para.text);
            for anchored in &mut para.anchored {
                anchored.unit.location.heading = heading.clone();
            }
            for &i in &para.refs {
                if let Some(r) = self.refs.get_mut(i) {
                    r.heading = heading.clone();
                }
            }
            location.heading = heading;
            let [rendered, explicit] = para.breaks;
            self.pages.paragraph_end(
                para.started_at_top,
                !explicit.is_empty(),
                para.props.section_break,
            );
            let [r, e] = para.start_pages;
            Some([
                PageInfo {
                    start: r,
                    breaks: rendered,
                },
                PageInfo {
                    start: e,
                    breaks: explicit,
                },
            ])
        } else {
            if let Some(st) = self.stories.last_mut() {
                st.pending_refs.append(&mut para.refs);
            }
            self.stories
                .last()
                .and_then(|st| anchored_pages(st.anchor_pages))
        };
        let Some(st) = self.stories.last_mut() else {
            return;
        };
        st.out.push(Unit {
            unit: TextUnit::new(para.text, location),
            pages,
        });
        st.out.extend(para.anchored);
    }

    fn end_textbox(&mut self) {
        // Never pop the main story, even on malformed input.
        if self.stories.len() <= 1 {
            return;
        }
        let Some(textbox) = self.stories.pop() else {
            return;
        };
        let mut units = textbox.out;
        let mut refs = textbox.pending_refs;
        // Unclosed paragraphs inside the text box are flushed as-is.
        let part = match self.kind {
            PartKind::Body => Some(Part::TextBox),
            PartKind::Header => Some(Part::Header),
            PartKind::Footer => Some(Part::Footer),
            _ => self.container.clone(),
        };
        for mut para in textbox.paras {
            if let Some(part) = part.clone() {
                units.push(Unit {
                    unit: TextUnit::new(para.text, Location::new(part)),
                    pages: anchored_pages(textbox.anchor_pages),
                });
            }
            units.extend(para.anchored);
            refs.append(&mut para.refs);
        }
        // Text box content and references attach to the anchor paragraph.
        if let Some(parent) = self.story() {
            match parent.paras.last_mut() {
                Some(anchor) => {
                    anchor.anchored.extend(units);
                    anchor.refs.append(&mut refs);
                }
                None => {
                    parent.out.extend(units);
                    parent.pending_refs.append(&mut refs);
                }
            }
        }
    }
}

/// Text box paragraphs sit on the page of their anchor, without breaks of their own.
fn anchored_pages(anchor: Option<[u32; 2]>) -> Option<Pages> {
    anchor.map(|[r, e]| {
        [
            PageInfo {
                start: r,
                breaks: Vec::new(),
            },
            PageInfo {
                start: e,
                breaks: Vec::new(),
            },
        ]
    })
}
