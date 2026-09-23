//! Word (.docx family) extraction: open the package, then extract each part.

pub mod extract;
pub mod package;

use std::path::Path;

use crate::error::FileError;
use crate::model::{Extracted, Format};

use package::Package;

/// Extracts all searchable text units from a Word file.
pub fn extract(path: &Path) -> Result<Extracted, FileError> {
    let mut pkg = Package::open(path)?;
    let main = pkg.main_document_part()?;
    let reader = pkg.open_part(&main)?;
    let body = extract::extract_part(reader, &main);
    Ok(Extracted {
        format: Format::Word,
        page_mode: None,
        units: body.units,
        partial_error: body.error,
    })
}
