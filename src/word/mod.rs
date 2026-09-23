//! Word (.docx family) extraction: open the package, then extract each part.

pub mod extract;
pub mod headings;
pub mod numbering;
pub mod package;
pub mod styles;
pub mod xml;

use std::path::Path;

use crate::error::FileError;
use crate::model::{Extracted, Format};

use headings::DocContext;
use numbering::Numbering;
use package::Package;
use styles::Styles;

/// Extracts all searchable text units from a Word file.
///
/// Broken auxiliary parts (styles, numbering) are reported as a partial error;
/// the body is still searched.
pub fn extract(path: &Path) -> Result<Extracted, FileError> {
    let mut pkg = Package::open(path)?;
    let main = pkg.main_document_part()?;
    let rels = pkg.relationships(&main)?;
    let target = |kind: &str| {
        rels.iter()
            .find(|r| r.rel_type == kind)
            .map(|r| r.target.clone())
    };
    let mut errors: Vec<FileError> = Vec::new();

    let mut ctx = DocContext::default();
    if let Some(part) = target("styles").filter(|p| pkg.has_part(p)) {
        let (styles, err) = Styles::parse(pkg.open_part(&part)?);
        ctx.styles = styles;
        errors.extend(err.map(|message| FileError::Xml { part, message }));
    }
    if let Some(part) = target("numbering").filter(|p| pkg.has_part(p)) {
        let (numbering, err) = Numbering::parse(pkg.open_part(&part)?);
        ctx.numbering = numbering;
        errors.extend(err.map(|message| FileError::Xml { part, message }));
    }

    let body = extract::extract_part(pkg.open_part(&main)?, &main, &ctx);
    errors.extend(body.error);
    Ok(Extracted {
        format: Format::Word,
        page_mode: None,
        units: body.units,
        partial_error: errors.into_iter().next(),
    })
}
