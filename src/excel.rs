//! Excel extraction with calamine: one text unit per non-empty cell (SPEC §7).

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use calamine::{
    Data, ExcelDateTime, Ods, Reader, SheetType, SheetVisible, Sheets, Xls, Xlsb, Xlsx,
    open_workbook,
};

use crate::error::FileError;
use crate::model::{Extracted, Format, Location, Part, SheetRef, TextUnit};

/// Signature of an OLE compound file (encrypted OOXML, or legacy .xls).
const CFB_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Collects every cell of a workbook. Convenient for tests; searching uses
/// [`extract_with`] so that non-matching cells are not kept in memory.
pub fn extract(path: &Path) -> Result<Extracted, FileError> {
    let mut units = Vec::new();
    let partial_error = extract_with(path, |unit| {
        units.push(unit);
        true
    })?;
    Ok(Extracted {
        format: Format::Excel,
        page_mode: None,
        units,
        partial_error,
    })
}

/// Streams cells to `sink` in sheet → row → column order. `sink` returns false to stop.
///
/// Returns `Ok(Some(error))` when some sheets could not be read (the others were).
pub fn extract_with(
    path: &Path,
    mut sink: impl FnMut(TextUnit) -> bool,
) -> Result<Option<FileError>, FileError> {
    let mut workbook = open(path)?;
    let sheets: Vec<(String, bool)> = workbook
        .sheets_metadata()
        .iter()
        .filter(|s| s.typ == SheetType::WorkSheet)
        .map(|s| (s.name.clone(), s.visible != SheetVisible::Visible))
        .collect();

    let mut partial = None;
    for (name, hidden) in sheets {
        let range = match workbook.worksheet_range(&name) {
            Ok(range) => range,
            Err(e) => {
                partial.get_or_insert(FileError::Excel(format!("シート「{name}」: {e}")));
                continue;
            }
        };
        let (row0, col0) = range.start().unwrap_or((0, 0));
        for (row, col, data) in range.used_cells() {
            let Some(text) = cell_text(data) else {
                continue;
            };
            let mut location = Location::new(Part::Cell);
            location.sheet = Some(SheetRef {
                name: name.clone(),
                hidden,
            });
            location.cell = Some(cell_address(
                row0.saturating_add(u32::try_from(row).unwrap_or(u32::MAX)),
                col0.saturating_add(u32::try_from(col).unwrap_or(u32::MAX)),
            ));
            if !sink(TextUnit::new(text, location)) {
                return Ok(partial);
            }
        }
    }
    Ok(partial)
}

/// Opens a workbook by extension. Encrypted OOXML files are reported as warnings.
fn open(path: &Path) -> Result<Sheets<BufReader<File>>, FileError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if ext != "xls" && has_cfb_signature(path)? {
        return Err(FileError::EncryptedOrLegacy);
    }
    match ext.as_str() {
        "xls" => open_workbook::<Xls<_>, _>(path)
            .map(Sheets::Xls)
            .map_err(|e| match e {
                calamine::XlsError::Password => FileError::EncryptedOrLegacy,
                calamine::XlsError::Io(io) => FileError::Io(io),
                other => FileError::Excel(other.to_string()),
            }),
        "xlsb" => open_workbook::<Xlsb<_>, _>(path)
            .map(Sheets::Xlsb)
            .map_err(|e| match e {
                calamine::XlsbError::Password => FileError::EncryptedOrLegacy,
                calamine::XlsbError::Io(io) => FileError::Io(io),
                other => FileError::Excel(other.to_string()),
            }),
        "ods" => open_workbook::<Ods<_>, _>(path)
            .map(Sheets::Ods)
            .map_err(|e| match e {
                calamine::OdsError::Io(io) => FileError::Io(io),
                other => FileError::Excel(other.to_string()),
            }),
        _ => open_workbook::<Xlsx<_>, _>(path)
            .map(Sheets::Xlsx)
            .map_err(|e| match e {
                calamine::XlsxError::Password => FileError::EncryptedOrLegacy,
                calamine::XlsxError::Io(io) => FileError::Io(io),
                other => FileError::Excel(other.to_string()),
            }),
    }
}

fn has_cfb_signature(path: &Path) -> Result<bool, FileError> {
    let mut head = Vec::with_capacity(8);
    File::open(path)?.take(8).read_to_end(&mut head)?;
    Ok(head == CFB_SIGNATURE)
}

/// Cell value as searchable text; `None` for empty cells.
pub fn cell_text(data: &Data) -> Option<String> {
    let text = match data {
        Data::Empty => return None,
        Data::String(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => number_text(*f),
        Data::Bool(true) => "TRUE".to_string(),
        Data::Bool(false) => "FALSE".to_string(),
        Data::DateTime(dt) => datetime_text(dt),
        Data::DateTimeIso(s) => iso_datetime_text(s),
        Data::DurationIso(s) => iso_duration_text(s),
        Data::Error(e) => e.to_string(),
    };
    (!text.is_empty()).then_some(text)
}

/// Integral values without a decimal point (1234.0 → `1234`), others in Rust's
/// shortest round-trip form.
pub fn number_text(f: f64) -> String {
    if f == 0.0 {
        // Avoid "-0".
        return "0".to_string();
    }
    f.to_string()
}

/// `YYYY-MM-DD`, with ` HH:MM:SS` when there is a time part. Time-only values
/// (serial < 1) are `HH:MM:SS`; durations are `H:MM:SS` in total hours.
pub fn datetime_text(dt: &ExcelDateTime) -> String {
    let serial = dt.as_f64();
    if dt.is_duration() {
        let total_secs = (serial * 86_400.0).round();
        let sign = if total_secs < 0.0 { "-" } else { "" };
        let total = total_secs.abs() as u64;
        return format!(
            "{sign}{}:{:02}:{:02}",
            total / 3600,
            total / 60 % 60,
            total % 60
        );
    }
    let (y, mo, d, h, mi, s, _ms) = dt.to_ymd_hms_milli();
    let has_time = h != 0 || mi != 0 || s != 0;
    if (0.0..1.0).contains(&serial) {
        return format!("{h:02}:{mi:02}:{s:02}");
    }
    if has_time {
        format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
    } else {
        format!("{y:04}-{mo:02}-{d:02}")
    }
}

/// ODS ISO 8601 datetimes: `2024-01-15T10:30:00` → `2024-01-15 10:30:00`.
fn iso_datetime_text(s: &str) -> String {
    let s = s.split('.').next().unwrap_or(s);
    match s.split_once('T') {
        Some((date, time)) if time.trim_start_matches(['0', ':']).is_empty() => date.to_string(),
        Some((date, time)) => format!("{date} {time}"),
        None => s.to_string(),
    }
}

/// ODS ISO 8601 durations: `PT36H05M00S` → `36:05:00`. Other forms are kept as-is.
fn iso_duration_text(s: &str) -> String {
    let Some(rest) = s.strip_prefix("PT") else {
        return s.to_string();
    };
    let mut parts = [0u64; 3];
    let mut num = String::new();
    for c in rest.chars() {
        let slot = match c {
            'H' => 0,
            'M' => 1,
            'S' => 2,
            c if c.is_ascii_digit() || c == '.' => {
                num.push(c);
                continue;
            }
            _ => return s.to_string(),
        };
        let value = num.split('.').next().and_then(|n| n.parse().ok());
        match (value, parts.get_mut(slot)) {
            (Some(v), Some(p)) => *p = v,
            _ => return s.to_string(),
        }
        num.clear();
    }
    let [h, m, sec] = parts;
    format!("{h}:{m:02}:{sec:02}")
}

/// Zero-based (row, col) → `A1` style. Columns: A…Z, AA…
pub fn cell_address(row: u32, col: u32) -> String {
    let mut letters = Vec::new();
    let mut n = u64::from(col) + 1;
    while n > 0 {
        let rem = (n - 1) % 26;
        letters.push(char::from(b'A' + rem as u8));
        n = (n - 1) / 26;
    }
    letters.reverse();
    let letters: String = letters.into_iter().collect();
    format!("{letters}{}", u64::from(row) + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::{CellErrorType, ExcelDateTimeType};

    #[test]
    fn addresses() {
        assert_eq!(cell_address(0, 0), "A1");
        assert_eq!(cell_address(13, 2), "C14");
        assert_eq!(cell_address(0, 25), "Z1");
        assert_eq!(cell_address(0, 26), "AA1");
        assert_eq!(cell_address(0, 701), "ZZ1");
        assert_eq!(cell_address(0, 702), "AAA1");
        assert_eq!(cell_address(1_048_575, 16_383), "XFD1048576");
    }

    #[test]
    fn numbers() {
        assert_eq!(number_text(1234.0), "1234");
        assert_eq!(number_text(-5.0), "-5");
        assert_eq!(number_text(-0.0), "0");
        assert_eq!(number_text(0.1), "0.1");
        assert_eq!(number_text(1.5), "1.5");
        assert_eq!(number_text(1e20), "100000000000000000000");
        assert_eq!(cell_text(&Data::Int(42)), Some("42".into()));
    }

    #[test]
    fn scalars() {
        assert_eq!(cell_text(&Data::Bool(true)), Some("TRUE".into()));
        assert_eq!(cell_text(&Data::Bool(false)), Some("FALSE".into()));
        assert_eq!(
            cell_text(&Data::Error(CellErrorType::NA)),
            Some("#N/A".into())
        );
        assert_eq!(
            cell_text(&Data::Error(CellErrorType::Div0)),
            Some("#DIV/0!".into())
        );
        assert_eq!(cell_text(&Data::Empty), None);
        assert_eq!(cell_text(&Data::String(String::new())), None);
        assert_eq!(cell_text(&Data::String(" a ".into())), Some(" a ".into()));
    }

    fn dt(value: f64, kind: ExcelDateTimeType) -> String {
        datetime_text(&ExcelDateTime::new(value, kind, false))
    }

    #[test]
    fn datetimes() {
        use ExcelDateTimeType::{DateTime, TimeDelta};
        assert_eq!(dt(45306.0, DateTime), "2024-01-15");
        assert_eq!(dt(45306.4375, DateTime), "2024-01-15 10:30:00");
        assert_eq!(dt(0.5, DateTime), "12:00:00");
        assert_eq!(dt(1.5, TimeDelta), "36:00:00");
        assert_eq!(
            datetime_text(&ExcelDateTime::new(43844.0, DateTime, true)),
            "2024-01-15"
        );
    }

    #[test]
    fn iso_values() {
        assert_eq!(iso_datetime_text("2024-01-15"), "2024-01-15");
        assert_eq!(iso_datetime_text("2024-01-15T00:00:00"), "2024-01-15");
        assert_eq!(
            iso_datetime_text("2024-01-15T10:30:00.5"),
            "2024-01-15 10:30:00"
        );
        assert_eq!(iso_duration_text("PT36H05M00S"), "36:05:00");
        assert_eq!(iso_duration_text("PT1H2M3.5S"), "1:02:03");
        assert_eq!(iso_duration_text("P1D"), "P1D");
    }
}
