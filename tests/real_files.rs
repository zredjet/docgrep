//! Verification against real files saved by Word / Excel (SPEC §11).
//!
//! `tests/fixtures/real/expected.toml` lists cases; files that are not present are
//! skipped, so the test passes when no real files have been provided.

mod common;

use std::path::{Path, PathBuf};

use docgrep::matcher::Matcher;
use docgrep::model::TextUnit;
use docgrep::walk::{Kind, kind_of};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Expected {
    #[serde(default)]
    case: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    file: String,
    pattern: String,
    #[serde(default = "first")]
    nth: usize,
    page: Option<u32>,
    /// Prefix of "number text"; empty means "before the first heading".
    heading: Option<String>,
    part: Option<String>,
    sheet: Option<String>,
    cell: Option<String>,
}

fn first() -> usize {
    1
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/real")
}

/// The unit and match start of the nth (1-based) occurrence of the pattern.
fn nth_match(path: &Path, pattern: &str, nth: usize) -> Result<(TextUnit, usize), String> {
    let units = match kind_of(path) {
        Some(Kind::Word) => docgrep::word::extract(path).map(|x| x.units),
        Some(Kind::Excel) => docgrep::excel::extract(path).map(|x| x.units),
        None => return Err("対象外の拡張子です".to_string()),
    }
    .map_err(|e| e.to_string())?;
    let matcher = Matcher::new(pattern, false, false).map_err(|e| e.to_string())?;
    let mut seen = 0;
    for unit in units {
        let found = matcher.find(&unit.text).map_err(|e| e.to_string())?;
        for (start, _) in found {
            seen += 1;
            if seen == nth {
                return Ok((unit, start));
            }
        }
    }
    Err(format!("{nth} 番目のヒットがありません（{seen} 件）"))
}

fn check(case: &Case, path: &Path) -> Vec<String> {
    let (unit, start) = match nth_match(path, &case.pattern, case.nth) {
        Ok(found) => found,
        Err(e) => return vec![e],
    };
    let loc = &unit.location;
    let mut problems = Vec::new();
    let mut compare = |what: &str, expected: String, actual: String| {
        if expected != actual {
            problems.push(format!("{what}: 期待 {expected:?} / 実際 {actual:?}"));
        }
    };
    if let Some(page) = case.page {
        compare(
            "page",
            page.to_string(),
            unit.page_at(start)
                .map(|p| p.to_string())
                .unwrap_or_default(),
        );
    }
    if let Some(part) = &case.part {
        compare("part", part.clone(), loc.part.kind().to_string());
    }
    if let Some(sheet) = &case.sheet {
        compare(
            "sheet",
            sheet.clone(),
            loc.sheet
                .as_ref()
                .map(|s| s.name.clone())
                .unwrap_or_default(),
        );
    }
    if let Some(cell) = &case.cell {
        compare("cell", cell.clone(), loc.cell.clone().unwrap_or_default());
    }
    if let Some(heading) = &case.heading {
        let actual = loc.heading.as_ref().map(|h| match &h.number {
            Some(n) => format!("{n} {}", h.text),
            None => h.text.clone(),
        });
        let ok = match (heading.is_empty(), &actual) {
            (true, None) => true,
            (false, Some(a)) => a.starts_with(heading.as_str()),
            _ => false,
        };
        if !ok {
            problems.push(format!(
                "heading: 期待 {heading:?} で始まる / 実際 {:?}",
                actual.unwrap_or_default()
            ));
        }
    }
    problems
}

/// Result of checking a directory: (cases checked, missing files, failures).
fn verify(dir: &Path) -> Option<(usize, Vec<String>, Vec<String>)> {
    let text = std::fs::read_to_string(dir.join("expected.toml")).ok()?;
    let expected: Expected = toml::from_str(&text).expect("expected.toml の書式が不正です");

    let mut checked = 0;
    let mut skipped = Vec::new();
    let mut failures = Vec::new();
    for case in &expected.case {
        let path = dir.join(&case.file);
        if !path.exists() {
            skipped.push(case.file.clone());
            continue;
        }
        checked += 1;
        for problem in check(case, &path) {
            failures.push(format!(
                "{} 「{}」 {}番目: {problem}",
                case.file, case.pattern, case.nth
            ));
        }
    }
    skipped.sort();
    skipped.dedup();
    Some((checked, skipped, failures))
}

#[test]
fn real_files_match_expectations() {
    let Some((checked, skipped, failures)) = verify(&fixtures()) else {
        eprintln!("skip: tests/fixtures/real/expected.toml がありません");
        return;
    };
    eprintln!(
        "実ファイル検証: 照合 {checked} 件 / ファイルなしでスキップ: {}",
        skipped.join(", ")
    );
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// The harness itself, on generated files: correct cases pass, wrong ones are reported.
#[test]
fn harness_detects_matches_and_mismatches() {
    use common::{DocxBuilder, p, r};

    let dir = tempfile::tempdir().unwrap();
    let styles = r#"<w:style w:type="paragraph" w:styleId="1"><w:name w:val="heading 1"/><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr><w:outlineLvl w:val="0"/></w:pPr></w:style>"#;
    let numbering = r#"<w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#;
    let body = [
        p("冒頭の目印A"),
        format!(
            r#"<w:p><w:pPr><w:pStyle w:val="1"/></w:pPr>{}</w:p>"#,
            r("システム構成")
        ),
        format!(
            "<w:p>{}<w:r><w:lastRenderedPageBreak/></w:r>{}</w:p>",
            r("目印A"),
            r("目印B")
        ),
    ]
    .concat();
    DocxBuilder::new()
        .styles(styles)
        .numbering(numbering)
        .body(&body)
        .write(dir.path(), "doc.docx");
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet().set_name("内訳").unwrap();
    ws.write_string(13, 2, "保守費用").unwrap();
    wb.save(dir.path().join("book.xlsx")).unwrap();

    let expected = r#"
[[case]]
file = "doc.docx"
pattern = "目印A"
nth = 1
page = 1
heading = ""
part = "body"

[[case]]
file = "doc.docx"
pattern = "目印A"
nth = 2
page = 1
heading = "1 システム"

[[case]]
file = "doc.docx"
pattern = "目印B"
page = 2
heading = "2 システム構成"

[[case]]
file = "book.xlsx"
pattern = "保守費用"
sheet = "内訳"
cell = "C15"
part = "cell"

[[case]]
file = "missing.docx"
pattern = "x"
page = 1
"#;
    std::fs::write(dir.path().join("expected.toml"), expected).unwrap();
    let (checked, skipped, failures) = verify(dir.path()).unwrap();
    assert_eq!(checked, 4);
    assert_eq!(skipped, ["missing.docx"]);
    // Case 3 expects the wrong heading number, case 4 the wrong cell.
    assert_eq!(failures.len(), 2, "{failures:#?}");
    assert!(failures[0].contains("目印B") && failures[0].contains("heading"));
    assert!(failures[1].contains("cell") && failures[1].contains("C14"));
}
