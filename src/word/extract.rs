//! Streaming extraction of WordprocessingML paragraphs into text units (SPEC §6.3).

use std::io::BufRead;

use quick_xml::NsReader;
use quick_xml::XmlVersion;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::{Namespace, ResolveResult};

use crate::error::FileError;
use crate::model::{Location, Part, TextUnit};

const W_TRANSITIONAL: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const W_STRICT: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const MATH_TRANSITIONAL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
const MATH_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/math";

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

fn is_w(ns: &str) -> bool {
    ns == W_TRANSITIONAL || ns == W_STRICT
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
            "del" | "moveFrom" => Tag::Deleted,
            "txbxContent" => Tag::TxbxContent,
            "pPr" | "rPr" | "tblPr" | "trPr" | "tcPr" | "sectPr" | "tblGrid" | "tblPrEx"
            | "sdtPr" | "sdtEndPr" | "rubyPr" | "customXmlPr" | "smartTagPr" | "fldData"
            | "instrText" | "delInstrText" | "delText" | "rt" => Tag::Skip,
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

#[derive(Debug, Default)]
struct ParaBuilder {
    text: String,
    /// Text box paragraphs anchored in this paragraph, emitted after it.
    anchored: Vec<TextUnit>,
}

/// An independent flow of text (the main body, or one text box).
/// Field state and table nesting do not leak between stories.
#[derive(Debug)]
struct Story {
    kind: StoryKind,
    fields: Vec<FieldState>,
    paras: Vec<ParaBuilder>,
    tables: Vec<TableCtx>,
    out: Vec<TextUnit>,
}

impl Story {
    fn new(kind: StoryKind) -> Self {
        Story {
            kind,
            fields: Vec::new(),
            paras: Vec::new(),
            tables: Vec::new(),
            out: Vec::new(),
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

struct Extractor {
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
    /// Set when the XML broke midway; `units` holds paragraphs completed before that.
    pub error: Option<FileError>,
}

/// Extracts every paragraph of a WordprocessingML part (document.xml etc.) in document order.
pub fn extract_part<R: BufRead>(reader: R, part_name: &str) -> PartText {
    let mut ex = Extractor {
        stories: vec![Story::new(StoryKind::Main)],
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
                    if ex.skip == 0 {
                        let tag = classify(&ns, e.local_name().as_ref());
                        let attrs = Attrs::read(&reader, &e, tag);
                        if let Some(frame) = ex.start(tag, &attrs) {
                            ex.end(frame);
                        }
                    }
                }
                Event::End(_) => {
                    if ex.skip > 0 {
                        ex.skip -= 1;
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
    let units = ex
        .stories
        .into_iter()
        .next()
        .map(|s| s.out)
        .unwrap_or_default();
    PartText { units, error }
}

/// The few attribute values we need, read with namespace resolution.
#[derive(Debug, Default)]
struct Attrs {
    /// `w:type` (br) or `w:fldCharType` (fldChar).
    kind: Option<String>,
    /// `w:char` (sym).
    char_code: Option<String>,
}

impl Attrs {
    fn read<R>(reader: &NsReader<R>, e: &BytesStart, tag: Tag) -> Attrs {
        let wanted: &str = match tag {
            Tag::Br => "type",
            Tag::FldChar => "fldCharType",
            Tag::Sym => "char",
            _ => return Attrs::default(),
        };
        let mut out = Attrs::default();
        for attr in e.attributes().flatten() {
            let (ns, local) = reader.resolver().resolve_attribute(attr.key);
            let in_w = match ns {
                ResolveResult::Bound(Namespace(ns)) => is_w(ns),
                ResolveResult::Unbound => true,
                ResolveResult::Unknown(_) => false,
            };
            if !in_w || local.as_ref() != wanted {
                continue;
            }
            let value = attr
                .normalized_value(XmlVersion::Implicit1_0)
                .map(|v| v.into_owned())
                .ok();
            match tag {
                Tag::Sym => out.char_code = value,
                _ => out.kind = value,
            }
        }
        out
    }
}

impl Extractor {
    fn story(&mut self) -> Option<&mut Story> {
        self.stories.last_mut()
    }

    fn text_allowed(&self) -> bool {
        self.deleted == 0 && self.stories.last().is_some_and(Story::fields_allow_text)
    }

    fn push_str(&mut self, s: &str) {
        if !self.text_allowed() {
            return;
        }
        if let Some(para) = self.story().and_then(|st| st.paras.last_mut()) {
            para.text.push_str(s);
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

    /// Handles an opening tag. Returns `None` if the subtree must be skipped.
    fn start(&mut self, tag: Tag, attrs: &Attrs) -> Option<Frame> {
        let frame = match tag {
            Tag::Skip => return None,
            Tag::P => {
                if let Some(st) = self.story() {
                    st.paras.push(ParaBuilder::default());
                }
                Frame::Paragraph
            }
            Tag::Tbl => {
                let is_main = self
                    .stories
                    .last()
                    .is_some_and(|s| s.kind == StoryKind::Main);
                let index = if is_main {
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
                Frame::Other
            }
            Tag::Tc => {
                if let Some(t) = self.story().and_then(|s| s.tables.last_mut()) {
                    t.col += 1;
                }
                Frame::Other
            }
            Tag::T | Tag::MathT => {
                self.in_text = true;
                Frame::Text
            }
            Tag::Tab => {
                self.push_char('\t');
                Frame::Other
            }
            Tag::Br | Tag::Cr => {
                self.push_char('\n');
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
                self.stories.push(Story::new(StoryKind::TextBox));
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
            Frame::Text => self.in_text = false,
            Frame::Deleted => self.deleted = self.deleted.saturating_sub(1),
            Frame::TextBox => self.end_textbox(),
            Frame::AlternateContent => {
                self.alternates.pop();
            }
            Frame::Other => {}
        }
    }

    fn end_paragraph(&mut self) {
        let Some(st) = self.story() else { return };
        let Some(para) = st.paras.pop() else { return };
        let unit = TextUnit::new(para.text, Location::new(st.part()));
        st.out.push(unit);
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
        // Unclosed paragraphs inside the text box are flushed as-is.
        for para in textbox.paras {
            units.push(TextUnit::new(para.text, Location::new(Part::TextBox)));
            units.extend(para.anchored);
        }
        if let Some(parent) = self.story() {
            match parent.paras.last_mut() {
                Some(anchor) => anchor.anchored.extend(units),
                None => parent.out.extend(units),
            }
        }
    }
}
