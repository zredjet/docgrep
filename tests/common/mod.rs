//! Fixture generation for integration tests.
#![allow(dead_code)]

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

pub const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub const W_STRICT_NS: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
const REL_TRANSITIONAL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const REL_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";

/// Namespace declarations put on every generated root element.
fn ns_decls(w_ns: &str, r_ns: &str) -> String {
    format!(
        concat!(
            r#"xmlns:w="{w}" xmlns:r="{r}" "#,
            r#"xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" "#,
            r#"xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape" "#,
            r#"xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" "#,
            r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" "#,
            r#"xmlns:v="urn:schemas-microsoft-com:vml" "#,
            r#"xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" "#,
            r#"xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" "#,
            r#"mc:Ignorable="w14""#
        ),
        w = w_ns,
        r = r_ns
    )
}

/// Builds a minimal .docx from XML fragments.
///
/// `[Content_Types].xml`, `_rels/.rels` and the main part's `.rels` are generated
/// from the parts that were set. Any entry can be replaced or removed with `raw`.
#[derive(Debug, Clone)]
pub struct DocxBuilder {
    body: String,
    document_xml: Option<String>,
    styles: Option<String>,
    numbering: Option<String>,
    footnotes: Option<String>,
    endnotes: Option<String>,
    comments: Option<String>,
    headers: Vec<String>,
    footers: Vec<String>,
    main_part: String,
    strict: bool,
    raw: Vec<(String, Option<Vec<u8>>)>,
}

impl Default for DocxBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DocxBuilder {
    pub fn new() -> Self {
        DocxBuilder {
            body: String::new(),
            document_xml: None,
            styles: None,
            numbering: None,
            footnotes: None,
            endnotes: None,
            comments: None,
            headers: Vec::new(),
            footers: Vec::new(),
            main_part: "word/document.xml".to_string(),
            strict: false,
            raw: Vec::new(),
        }
    }

    /// Inner XML of `<w:body>`.
    pub fn body(mut self, xml: &str) -> Self {
        self.body = xml.to_string();
        self
    }

    /// Replaces the whole main document part (for custom prefixes etc.).
    pub fn document_xml(mut self, xml: &str) -> Self {
        self.document_xml = Some(xml.to_string());
        self
    }

    /// Inner XML of `<w:styles>`.
    pub fn styles(mut self, xml: &str) -> Self {
        self.styles = Some(xml.to_string());
        self
    }

    /// Inner XML of `<w:numbering>`.
    pub fn numbering(mut self, xml: &str) -> Self {
        self.numbering = Some(xml.to_string());
        self
    }

    /// Inner XML of `<w:footnotes>`.
    pub fn footnotes(mut self, xml: &str) -> Self {
        self.footnotes = Some(xml.to_string());
        self
    }

    /// Inner XML of `<w:endnotes>`.
    pub fn endnotes(mut self, xml: &str) -> Self {
        self.endnotes = Some(xml.to_string());
        self
    }

    /// Inner XML of `<w:comments>`.
    pub fn comments(mut self, xml: &str) -> Self {
        self.comments = Some(xml.to_string());
        self
    }

    /// Inner XML of a `<w:hdr>`; becomes `word/headerN.xml`.
    pub fn header(mut self, xml: &str) -> Self {
        self.headers.push(xml.to_string());
        self
    }

    /// Inner XML of a `<w:ftr>`; becomes `word/footerN.xml`.
    pub fn footer(mut self, xml: &str) -> Self {
        self.footers.push(xml.to_string());
        self
    }

    /// Path of the main document part inside the package.
    pub fn main_part(mut self, path: &str) -> Self {
        self.main_part = path.to_string();
        self
    }

    /// Use Strict namespaces and relationship types.
    pub fn strict(mut self) -> Self {
        self.strict = true;
        self
    }

    /// Adds or replaces an entry verbatim.
    pub fn raw(mut self, name: &str, bytes: impl Into<Vec<u8>>) -> Self {
        self.raw.push((name.to_string(), Some(bytes.into())));
        self
    }

    /// Removes an entry (including generated ones).
    pub fn without(mut self, name: &str) -> Self {
        self.raw.push((name.to_string(), None));
        self
    }

    fn w_ns(&self) -> &'static str {
        if self.strict { W_STRICT_NS } else { W_NS }
    }

    fn rel_ns(&self) -> &'static str {
        if self.strict {
            REL_STRICT
        } else {
            REL_TRANSITIONAL
        }
    }

    fn wrap(&self, root: &str, inner: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:{root} {}>{inner}</w:{root}>"#,
            ns_decls(self.w_ns(), self.rel_ns())
        )
    }

    /// Main-part-relative parts: (target relative to main part folder, rel type, content).
    fn sub_parts(&self) -> Vec<(String, &'static str, String)> {
        let mut parts = Vec::new();
        let singles = [
            ("styles.xml", "styles", "styles", &self.styles),
            ("numbering.xml", "numbering", "numbering", &self.numbering),
            ("footnotes.xml", "footnotes", "footnotes", &self.footnotes),
            ("endnotes.xml", "endnotes", "endnotes", &self.endnotes),
            ("comments.xml", "comments", "comments", &self.comments),
        ];
        for (file, rel, root, content) in singles {
            if let Some(c) = content {
                parts.push((file.to_string(), rel, self.wrap(root, c)));
            }
        }
        for (i, h) in self.headers.iter().enumerate() {
            parts.push((
                format!("header{}.xml", i + 1),
                "header",
                self.wrap("hdr", h),
            ));
        }
        for (i, f) in self.footers.iter().enumerate() {
            parts.push((
                format!("footer{}.xml", i + 1),
                "footer",
                self.wrap("ftr", f),
            ));
        }
        parts
    }

    fn entries(&self) -> Vec<(String, Vec<u8>)> {
        let main_dir = self
            .main_part
            .rsplit_once('/')
            .map(|(d, _)| d.to_string())
            .unwrap_or_default();
        let in_main_dir = |file: &str| {
            if main_dir.is_empty() {
                file.to_string()
            } else {
                format!("{main_dir}/{file}")
            }
        };
        let main_file = self
            .main_part
            .rsplit_once('/')
            .map(|(_, f)| f.to_string())
            .unwrap_or_else(|| self.main_part.clone());

        let document = self
            .document_xml
            .clone()
            .unwrap_or_else(|| self.wrap("document", &format!("<w:body>{}</w:body>", self.body)));
        let sub_parts = self.sub_parts();

        let mut content_types = String::from(
            r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>"#,
        );
        content_types.push_str(&format!(
            r#"<Override PartName="/{}" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
            self.main_part
        ));

        let rels_open = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#;
        let root_rels = format!(
            r#"{rels_open}<Relationship Id="rId1" Type="{}/officeDocument" Target="{}"/></Relationships>"#,
            self.rel_ns(),
            self.main_part
        );
        let mut doc_rels = String::from(rels_open);
        for (i, (target, rel, _)) in sub_parts.iter().enumerate() {
            doc_rels.push_str(&format!(
                r#"<Relationship Id="rId{}" Type="{}/{rel}" Target="{target}"/>"#,
                i + 1,
                self.rel_ns()
            ));
        }
        doc_rels.push_str("</Relationships>");

        let mut entries: Vec<(String, Vec<u8>)> = vec![
            (
                "[Content_Types].xml".to_string(),
                content_types.into_bytes(),
            ),
            ("_rels/.rels".to_string(), root_rels.into_bytes()),
            (self.main_part.clone(), document.into_bytes()),
            (
                in_main_dir(&format!("_rels/{main_file}.rels")),
                doc_rels.into_bytes(),
            ),
        ];
        for (target, _, content) in sub_parts {
            entries.push((in_main_dir(&target), content.into_bytes()));
        }
        for (name, bytes) in &self.raw {
            entries.retain(|(n, _)| n != name);
            if let Some(b) = bytes {
                entries.push((name.clone(), b.clone()));
            }
        }
        entries
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .last_modified_time(DateTime::default());
        for (name, bytes) in self.entries() {
            zip.start_file(name, options).unwrap();
            zip.write_all(&bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    pub fn write(&self, dir: &Path, name: &str) -> PathBuf {
        write_bytes(dir, name, &self.to_bytes())
    }
}

/// Paragraph with a single run.
pub fn p(text: &str) -> String {
    format!("<w:p>{}</w:p>", r(text))
}

/// Run with a single `w:t` (text is XML-escaped, spaces preserved).
pub fn r(text: &str) -> String {
    format!(
        r#"<w:r><w:t xml:space="preserve">{}</w:t></w:r>"#,
        xml_escape(text)
    )
}

pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn write_bytes(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, bytes).unwrap();
    path
}

/// A file starting with the OLE compound file signature (like an encrypted docx).
pub fn write_cfb(dir: &Path, name: &str) -> PathBuf {
    let mut bytes = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    bytes.extend_from_slice(&[0u8; 504]);
    write_bytes(dir, name, &bytes)
}
