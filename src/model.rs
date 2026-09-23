//! Core data types shared by extractors, the matcher and the output layer.

use crate::error::FileError;

/// Which kind of source a file was read as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Word,
    Excel,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Word => "word",
            Format::Excel => "excel",
        }
    }
}

/// Where inside the document a text unit lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Body,
    Table { index: u32, row: u32, col: u32 },
    TextBox,
    Toc,
    Footnote { id: String },
    Endnote { id: String },
    Comment { author: String },
    Header,
    Footer,
    Cell,
}

impl Part {
    /// Machine-readable name used in JSON output.
    pub fn kind(&self) -> &'static str {
        match self {
            Part::Body => "body",
            Part::Table { .. } => "table",
            Part::TextBox => "textbox",
            Part::Toc => "toc",
            Part::Footnote { .. } => "footnote",
            Part::Endnote { .. } => "endnote",
            Part::Comment { .. } => "comment",
            Part::Header => "header",
            Part::Footer => "footer",
            Part::Cell => "cell",
        }
    }

    /// Human-readable label shown in brackets on the location line.
    pub fn label(&self) -> Option<String> {
        match self {
            Part::Body | Part::Cell => None,
            Part::Table { index, row, col } => Some(format!("表{index} {row}行{col}列")),
            Part::TextBox => Some("テキストボックス".to_string()),
            Part::Toc => Some("目次".to_string()),
            Part::Footnote { id } => Some(format!("脚注{id}")),
            Part::Endnote { id } => Some(format!("文末脚注{id}")),
            Part::Comment { author } => Some(format!("コメント: {author}")),
            Part::Header => Some("ヘッダー".to_string()),
            Part::Footer => Some("フッター".to_string()),
        }
    }

    /// Header and footer paragraphs have no page or heading.
    pub fn is_page_less(&self) -> bool {
        matches!(self, Part::Header | Part::Footer)
    }
}

/// How page numbers of a Word document were estimated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageMode {
    Rendered,
    Explicit,
}

impl PageMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PageMode::Rendered => "rendered",
            PageMode::Explicit => "explicit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// 1-based outline level.
    pub level: u8,
    pub number: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetRef {
    pub name: String,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub part: Part,
    /// Page at the start of the paragraph.
    pub page_at_start: Option<u32>,
    pub page_mode: Option<PageMode>,
    pub heading: Option<Heading>,
    pub sheet: Option<SheetRef>,
    pub cell: Option<String>,
}

impl Location {
    pub fn new(part: Part) -> Self {
        Location {
            part,
            page_at_start: None,
            page_mode: None,
            heading: None,
            sheet: None,
            cell: None,
        }
    }
}

/// One searchable piece of text: a Word paragraph or an Excel cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextUnit {
    pub text: String,
    pub location: Location,
    /// Page break positions inside the paragraph (char offsets). Empty for Excel.
    pub page_breaks: Vec<usize>,
}

impl TextUnit {
    pub fn new(text: String, location: Location) -> Self {
        TextUnit {
            text,
            location,
            page_breaks: Vec::new(),
        }
    }

    /// Estimated page of a position inside this unit.
    pub fn page_at(&self, pos: usize) -> Option<u32> {
        let start = self.location.page_at_start?;
        let breaks = self.page_breaks.iter().filter(|&&b| b <= pos).count();
        Some(start.saturating_add(u32::try_from(breaks).unwrap_or(u32::MAX)))
    }
}

/// A single match inside a text unit. Offsets are in chars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    pub unit_index: usize,
    pub start: usize,
    pub end: usize,
}

/// Result of extracting one file.
#[derive(Debug)]
pub struct Extracted {
    pub format: Format,
    pub page_mode: Option<PageMode>,
    pub units: Vec<TextUnit>,
    /// Set when extraction stopped midway; `units` holds what was read until then.
    pub partial_error: Option<FileError>,
}
