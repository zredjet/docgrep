//! OPC package access: zip container, relationship resolution, encryption check.

use std::fs::File;
use std::io::{BufRead, BufReader, Cursor, Read};
use std::path::Path;

use quick_xml::NsReader;
use quick_xml::XmlVersion;
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use zip::ZipArchive;

use crate::error::FileError;

/// Signature of an OLE compound file (encrypted OOXML, IRM, or legacy .doc).
const CFB_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

const RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

const REL_TYPE_PREFIXES: [&str; 2] = [
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/",
    "http://purl.oclc.org/ooxml/officeDocument/relationships/",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    pub id: String,
    /// Type URI with the Transitional/Strict prefix removed (e.g. `styles`).
    /// Unknown URIs are kept as-is.
    pub rel_type: String,
    /// Resolved part name inside the package (no leading slash).
    pub target: String,
}

pub struct Package {
    zip: ZipArchive<BufReader<File>>,
}

impl Package {
    pub fn open(path: &Path) -> Result<Self, FileError> {
        let mut file = File::open(path)?;
        let mut head = [0u8; 8];
        let mut read = 0;
        while read < head.len() {
            let n = file.read(head.get_mut(read..).unwrap_or_default())?;
            if n == 0 {
                break;
            }
            read += n;
        }
        if read == head.len() && head == CFB_SIGNATURE {
            return Err(FileError::EncryptedOrLegacy);
        }
        let file = File::open(path)?;
        let zip = ZipArchive::new(BufReader::new(file))?;
        Ok(Package { zip })
    }

    /// Part name of the main document, from the package-level officeDocument relationship.
    pub fn main_document_part(&mut self) -> Result<String, FileError> {
        let rels = self.relationships("")?;
        rels.into_iter()
            .find(|r| r.rel_type == "officeDocument")
            .map(|r| r.target)
            .ok_or(FileError::MissingMainPart)
    }

    /// Relationships of `part` (empty string = package root). A missing .rels means none.
    pub fn relationships(&mut self, part: &str) -> Result<Vec<Relationship>, FileError> {
        let rels_name = rels_part_name(part);
        let Some(actual) = self.find_part(&rels_name) else {
            return Ok(Vec::new());
        };
        let reader = self.open_actual(&actual)?;
        parse_relationships(reader, part, &rels_name)
    }

    /// Opens a part for streaming. UTF-16 parts are transcoded to UTF-8 in memory.
    pub fn open_part(&mut self, name: &str) -> Result<Box<dyn BufRead + '_>, FileError> {
        let actual = self.find_part(name).ok_or(FileError::MissingMainPart)?;
        self.open_actual(&actual)
    }

    pub fn has_part(&self, name: &str) -> bool {
        self.find_part(name).is_some()
    }

    fn open_actual(&mut self, actual: &str) -> Result<Box<dyn BufRead + '_>, FileError> {
        let entry = self.zip.by_name(actual)?;
        let mut reader = BufReader::new(entry);
        let head = reader.fill_buf()?;
        let utf16 = match head {
            [0xFF, 0xFE, ..] => Some(false),
            [0xFE, 0xFF, ..] => Some(true),
            _ => None,
        };
        match utf16 {
            None => Ok(Box::new(reader)),
            Some(big_endian) => {
                let mut bytes = Vec::new();
                reader.read_to_end(&mut bytes)?;
                Ok(Box::new(Cursor::new(decode_utf16(&bytes, big_endian))))
            }
        }
    }

    /// Finds a zip entry for an OPC part name. Part names are ASCII case-insensitive.
    fn find_part(&self, name: &str) -> Option<String> {
        if self.zip.index_for_name(name).is_some() {
            return Some(name.to_string());
        }
        self.zip
            .file_names()
            .find(|n| n.trim_start_matches('/').eq_ignore_ascii_case(name))
            .map(str::to_string)
    }
}

/// `word/document.xml` -> `word/_rels/document.xml.rels`; `` -> `_rels/.rels`.
fn rels_part_name(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

fn parse_relationships<R: BufRead>(
    reader: R,
    source_part: &str,
    rels_name: &str,
) -> Result<Vec<Relationship>, FileError> {
    let mut reader = NsReader::from_reader(reader);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    loop {
        let (ns, event) =
            reader
                .read_resolved_event_into(&mut buf)
                .map_err(|e| FileError::Xml {
                    part: rels_name.to_string(),
                    message: e.to_string(),
                })?;
        match event {
            Event::Start(e) | Event::Empty(e) => {
                let is_rel = matches!(ns, ResolveResult::Bound(n) if n.0 == RELS_NS)
                    && e.local_name().as_ref() == "Relationship";
                if !is_rel {
                    buf.clear();
                    continue;
                }
                let mut id = String::new();
                let mut rel_type = String::new();
                let mut target = String::new();
                let mut external = false;
                for attr in e.attributes().flatten() {
                    let value = attr
                        .normalized_value(XmlVersion::Implicit1_0)
                        .map(|v| v.into_owned())
                        .unwrap_or_default();
                    match attr.key.local_name().as_ref() {
                        "Id" => id = value,
                        "Type" => rel_type = value,
                        "Target" => target = value,
                        "TargetMode" => external = value.eq_ignore_ascii_case("External"),
                        _ => {}
                    }
                }
                if !external && !target.is_empty() {
                    out.push(Relationship {
                        id,
                        rel_type: normalize_rel_type(&rel_type),
                        target: resolve_target(source_part, &target),
                    });
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

fn normalize_rel_type(uri: &str) -> String {
    REL_TYPE_PREFIXES
        .iter()
        .find_map(|p| uri.strip_prefix(p))
        .unwrap_or(uri)
        .to_string()
}

/// Resolves a relationship target against the folder of its source part.
/// Handles `../`, `./`, a leading `/` (package root) and percent-encoding.
pub fn resolve_target(source_part: &str, target: &str) -> String {
    let target = percent_decode(target);
    let target = target.split(['#', '?']).next().unwrap_or_default();
    let mut segments: Vec<&str> = Vec::new();
    let relative = match target.strip_prefix('/') {
        Some(rest) => rest,
        None => {
            if let Some((dir, _)) = source_part.rsplit_once('/') {
                segments.extend(dir.split('/').filter(|s| !s.is_empty()));
            }
            target
        }
    };
    for seg in relative.split(['/', '\\']) {
        match seg {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }
    segments.join("/")
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%'
            && let (Some(h), Some(l)) = (
                bytes.get(i + 1).and_then(|c| (*c as char).to_digit(16)),
                bytes.get(i + 2).and_then(|c| (*c as char).to_digit(16)),
            )
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

fn decode_utf16(bytes: &[u8], big_endian: bool) -> Vec<u8> {
    let units = bytes.chunks_exact(2).skip(1).map(|c| match c {
        [a, b] if big_endian => u16::from_be_bytes([*a, *b]),
        [a, b] => u16::from_le_bytes([*a, *b]),
        _ => 0xFFFD,
    });
    char::decode_utf16(units)
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect::<String>()
        .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rels_names() {
        assert_eq!(rels_part_name(""), "_rels/.rels");
        assert_eq!(
            rels_part_name("word/document.xml"),
            "word/_rels/document.xml.rels"
        );
        assert_eq!(rels_part_name("main.xml"), "_rels/main.xml.rels");
    }

    #[test]
    fn targets_resolve_relative_to_source_folder() {
        assert_eq!(resolve_target("", "word/document.xml"), "word/document.xml");
        assert_eq!(
            resolve_target("word/document.xml", "styles.xml"),
            "word/styles.xml"
        );
        assert_eq!(
            resolve_target("word/document.xml", "../customXml/item1.xml"),
            "customXml/item1.xml"
        );
        assert_eq!(
            resolve_target("word/document.xml", "/word/footnotes.xml"),
            "word/footnotes.xml"
        );
        assert_eq!(
            resolve_target("word/document.xml", "./sub/../header1.xml"),
            "word/header1.xml"
        );
        assert_eq!(
            resolve_target("word/document.xml", "my%20header.xml"),
            "word/my header.xml"
        );
    }

    #[test]
    fn rel_types_accept_transitional_and_strict() {
        assert_eq!(
            normalize_rel_type(
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument"
            ),
            "officeDocument"
        );
        assert_eq!(
            normalize_rel_type("http://purl.oclc.org/ooxml/officeDocument/relationships/styles"),
            "styles"
        );
    }

    #[test]
    fn utf16_is_transcoded() {
        let mut bytes = vec![0xFF, 0xFE];
        for u in "<a>字</a>".encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode_utf16(&bytes, false), "<a>字</a>".as_bytes());
    }
}
