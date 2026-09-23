//! Excel extraction (SPEC §7) on workbooks generated with rust_xlsxwriter.

mod common;

use std::path::{Path, PathBuf};

use docgrep::error::FileError;
use docgrep::model::{Part, TextUnit};
use rust_xlsxwriter::{Chart, ChartType, ExcelDateTime, Format, Formula, Workbook};

/// (sheet, hidden, cell, text) of every unit.
fn cells(path: &Path) -> Vec<(String, bool, String, String)> {
    let x = docgrep::excel::extract(path).unwrap();
    assert!(x.partial_error.is_none(), "{:?}", x.partial_error);
    x.units.into_iter().map(describe).collect()
}

fn describe(u: TextUnit) -> (String, bool, String, String) {
    assert_eq!(u.location.part, Part::Cell);
    assert!(u.location.page_at_start.is_none());
    assert!(u.location.heading.is_none());
    let sheet = u.location.sheet.unwrap();
    (sheet.name, sheet.hidden, u.location.cell.unwrap(), u.text)
}

fn row(sheet: &str, cell: &str, text: &str) -> (String, bool, String, String) {
    (sheet.into(), false, cell.into(), text.into())
}

fn save(wb: &mut Workbook, dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    wb.save(&path).unwrap();
    path
}

#[test]
fn sheets_rows_and_columns_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet().set_name("一覧").unwrap();
    ws.write_string(1, 1, "B2").unwrap();
    ws.write_string(0, 2, "C1").unwrap();
    ws.write_string(1, 0, "A2").unwrap();
    let ws = wb.add_worksheet().set_name("内訳").unwrap();
    ws.write_string(0, 0, "次のシート").unwrap();
    let path = save(&mut wb, dir.path(), "a.xlsx");
    assert_eq!(
        cells(&path),
        vec![
            row("一覧", "C1", "C1"),
            row("一覧", "A2", "A2"),
            row("一覧", "B2", "B2"),
            row("内訳", "A1", "次のシート"),
        ]
    );
}

#[test]
fn addresses_account_for_range_start_offset() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet().set_name("内訳").unwrap();
    ws.write_string(13, 2, "サーバ保守費用（年額）").unwrap();
    ws.write_string(4, 27, "AB5").unwrap();
    let path = save(&mut wb, dir.path(), "a.xlsx");
    assert_eq!(
        cells(&path),
        vec![
            row("内訳", "AB5", "AB5"),
            row("内訳", "C14", "サーバ保守費用（年額）"),
        ]
    );
}

#[test]
fn value_types_are_stringified() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet().set_name("値").unwrap();
    let yen = Format::new().set_num_format("¥#,##0");
    ws.write_number_with_format(0, 0, 1234.0, &yen).unwrap();
    ws.write_number(1, 0, 0.1).unwrap();
    ws.write_number(2, 0, -3.5).unwrap();
    ws.write_number(3, 0, 1e20).unwrap();
    ws.write_boolean(4, 0, true).unwrap();
    ws.write_boolean(5, 0, false).unwrap();
    ws.write_string(6, 0, "改行\nあり").unwrap();
    ws.write_string(7, 0, "").unwrap();
    ws.write_blank(8, 0, &Format::new().set_bold()).unwrap();
    ws.write_string(9, 0, "最後").unwrap();
    let path = save(&mut wb, dir.path(), "a.xlsx");
    let texts: Vec<_> = cells(&path).into_iter().map(|c| (c.2, c.3)).collect();
    let expected = [
        ("A1", "1234"),
        ("A2", "0.1"),
        ("A3", "-3.5"),
        ("A4", "100000000000000000000"),
        ("A5", "TRUE"),
        ("A6", "FALSE"),
        ("A7", "改行\nあり"),
        ("A10", "最後"),
    ]
    .map(|(a, b)| (a.to_string(), b.to_string()));
    assert_eq!(texts, expected);
}

#[test]
fn dates_times_and_durations() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet().set_name("日付").unwrap();
    let date = ExcelDateTime::from_ymd(2024, 1, 15).unwrap();
    let datetime = ExcelDateTime::from_ymd(2024, 1, 15)
        .unwrap()
        .and_hms(10, 30, 0)
        .unwrap();
    let time = ExcelDateTime::from_hms(12, 0, 0).unwrap();
    ws.write_datetime_with_format(0, 0, &date, &Format::new().set_num_format("yyyy/m/d"))
        .unwrap();
    ws.write_datetime_with_format(
        1,
        0,
        &datetime,
        &Format::new().set_num_format("yyyy-mm-dd hh:mm"),
    )
    .unwrap();
    ws.write_datetime_with_format(2, 0, &time, &Format::new().set_num_format("hh:mm"))
        .unwrap();
    ws.write_number_with_format(3, 0, 1.5, &Format::new().set_num_format("[h]:mm:ss"))
        .unwrap();
    let path = save(&mut wb, dir.path(), "a.xlsx");
    let texts: Vec<_> = cells(&path).into_iter().map(|c| c.3).collect();
    assert_eq!(
        texts,
        vec!["2024-01-15", "2024-01-15 10:30:00", "12:00:00", "36:00:00"]
    );
}

#[test]
fn formulas_use_cached_results() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet().set_name("計算").unwrap();
    ws.write_number(0, 0, 1234.0).unwrap();
    ws.write_formula(0, 1, Formula::new("=A1*2").set_result("2468"))
        .unwrap();
    ws.write_formula(0, 2, Formula::new("=\"合計\"&A1").set_result("合計1234"))
        .unwrap();
    ws.write_formula(0, 3, Formula::new("=NA()").set_result("#N/A"))
        .unwrap();
    let path = save(&mut wb, dir.path(), "a.xlsx");
    let texts: Vec<_> = cells(&path).into_iter().map(|c| c.3).collect();
    assert_eq!(texts, vec!["1234", "2468", "合計1234", "#N/A"]);
}

#[test]
fn hidden_sheets_are_searched_and_marked() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbook::new();
    wb.add_worksheet()
        .set_name("表示")
        .unwrap()
        .write_string(0, 0, "a")
        .unwrap();
    wb.add_worksheet()
        .set_name("非表示")
        .unwrap()
        .set_hidden(true)
        .write_string(0, 0, "b")
        .unwrap();
    wb.add_worksheet()
        .set_name("完全非表示")
        .unwrap()
        .set_very_hidden(true)
        .write_string(0, 0, "c")
        .unwrap();
    let path = save(&mut wb, dir.path(), "a.xlsx");
    let hidden: Vec<_> = cells(&path).into_iter().map(|c| (c.0, c.1)).collect();
    assert_eq!(
        hidden,
        vec![
            ("表示".to_string(), false),
            ("非表示".to_string(), true),
            ("完全非表示".to_string(), true),
        ]
    );
}

#[test]
fn chart_sheets_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet().set_name("データ").unwrap();
    ws.write_number(0, 0, 1.0).unwrap();
    ws.write_number(1, 0, 2.0).unwrap();
    let mut chart = Chart::new(ChartType::Column);
    chart.add_series().set_values("'データ'!$A$1:$A$2");
    let cs = wb.add_chartsheet();
    cs.set_name("グラフ").unwrap();
    cs.insert_chart(0, 0, &chart).unwrap();
    let path = save(&mut wb, dir.path(), "a.xlsx");
    let sheets: Vec<_> = cells(&path).into_iter().map(|c| c.0).collect();
    assert_eq!(sheets, vec!["データ", "データ"]);
}

#[test]
fn template_and_uppercase_extensions() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["t.xltx", "U.XLSX"] {
        let mut wb = Workbook::new();
        wb.add_worksheet().write_string(0, 0, "中身").unwrap();
        let path = save(&mut wb, dir.path(), name);
        assert_eq!(cells(&path), vec![row("Sheet1", "A1", "中身")]);
    }
}

#[test]
fn encrypted_workbook_is_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let path = common::write_cfb(dir.path(), "enc.xlsx");
    let err = docgrep::excel::extract(&path).unwrap_err();
    assert!(matches!(err, FileError::EncryptedOrLegacy));
}

#[test]
fn broken_workbook_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["bad.xlsx", "bad.xls", "bad.xlsb", "bad.ods"] {
        let path = common::write_bytes(dir.path(), name, b"not a workbook");
        let err = docgrep::excel::extract(&path).unwrap_err();
        assert!(!err.is_warning(), "{name}: {err:?}");
    }
}

#[test]
fn sink_can_stop_early() {
    let dir = tempfile::tempdir().unwrap();
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    for r in 0..10 {
        ws.write_string(r, 0, "x").unwrap();
    }
    let path = save(&mut wb, dir.path(), "a.xlsx");
    let mut seen = 0;
    docgrep::excel::extract_with(&path, |_| {
        seen += 1;
        seen < 3
    })
    .unwrap();
    assert_eq!(seen, 3);
}
