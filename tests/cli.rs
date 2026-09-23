//! End-to-end behaviour of the binary: exit codes, output modes, error handling.

mod common;

use std::path::Path;

use assert_cmd::cargo::cargo_bin_cmd;
use common::{DocxBuilder, p, r};
use predicates::prelude::*;
use tempfile::TempDir;

fn docgrep(dir: &Path) -> assert_cmd::Command {
    let mut cmd = cargo_bin_cmd!("docgrep");
    cmd.current_dir(dir)
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE");
    cmd
}

/// A document exercising body, table (with a line break) and text box parts.
fn sample(dir: &Path, name: &str) {
    let long = "本システムではWebサーバとアプリケーションサーバを分離し、負荷分散装置の配下に置く構成とする。";
    let cell_with_break = format!(
        "<w:p>{}<w:r><w:br/></w:r>{}</w:p>",
        r("サーバ名"),
        r("役割")
    );
    let body = [
        p(long),
        p("サーバー室の入室手順"),
        format!(
            "<w:tbl><w:tblPr/><w:tr><w:tc>{}</w:tc><w:tc>{cell_with_break}</w:tc></w:tr></w:tbl>",
            p("項目"),
        ),
        format!(
            r#"<w:p>{}<w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wps:txbx><w:txbxContent>{}</w:txbxContent></wps:txbx></w:drawing></mc:Choice><mc:Fallback><w:pict><v:textbox><w:txbxContent>{}</w:txbxContent></v:textbox></w:pict></mc:Fallback></mc:AlternateContent></w:r></w:p>"#,
            r("図1"),
            p("サーバ\t構成図"),
            p("サーバ\t構成図")
        ),
        p("関係なし"),
    ]
    .concat();
    DocxBuilder::new().body(&body).write(dir, name);
}

fn setup() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    sample(dir.path(), "仕様書.docx");
    dir
}

/// A relative path written with `/`, as the OS prints it (`\` on Windows).
fn native(path: &str) -> String {
    path.replace('/', std::path::MAIN_SEPARATOR_STR)
}

fn stdout_of(cmd: &mut assert_cmd::Command) -> String {
    String::from_utf8(cmd.output().unwrap().stdout).unwrap()
}

// --- exit codes ------------------------------------------------------------------

#[test]
fn exit_0_on_match() {
    let dir = setup();
    docgrep(dir.path())
        .args(["サーバ", "仕様書.docx"])
        .assert()
        .code(0);
}

#[test]
fn exit_1_on_no_match() {
    let dir = setup();
    docgrep(dir.path())
        .args(["存在しない語", "仕様書.docx"])
        .assert()
        .code(1)
        .stdout("");
}

#[test]
fn exit_2_on_invalid_regex() {
    let dir = setup();
    docgrep(dir.path())
        .args(["-e", "(abc", "仕様書.docx"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("docgrep: エラー: "))
        .stderr(predicate::str::contains("正規表現が不正です"));
}

#[test]
fn exit_2_on_empty_pattern() {
    let dir = setup();
    docgrep(dir.path())
        .args(["", "仕様書.docx"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("検索パターンが空です"));
}

#[test]
fn broken_file_is_reported_and_others_continue() {
    let dir = setup();
    common::write_bytes(dir.path(), "a_壊れた.docx", b"garbage");
    docgrep(dir.path())
        .args(["サーバ", "a_壊れた.docx", "仕様書.docx"])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("■ 仕様書.docx"))
        .stderr(predicate::str::contains("docgrep: エラー: a_壊れた.docx: "));
}

#[test]
fn encrypted_file_is_a_warning_only() {
    let dir = setup();
    common::write_cfb(dir.path(), "暗号.docx");
    docgrep(dir.path())
        .args(["サーバ", "暗号.docx", "仕様書.docx"])
        .assert()
        .code(0)
        .stderr(predicate::str::contains(
            "docgrep: 警告: 暗号.docx: 暗号化または旧形式のため読めません",
        ));
}

#[test]
fn unsupported_extension_given_explicitly_is_a_warning() {
    let dir = setup();
    common::write_bytes(dir.path(), "古い.doc", b"x");
    docgrep(dir.path())
        .args(["サーバ", "古い.doc"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "docgrep: 警告: 古い.doc: 未対応の形式です",
        ));
}

#[test]
fn missing_file_is_an_error() {
    let dir = setup();
    docgrep(dir.path())
        .args(["サーバ", "無い.docx"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("docgrep: エラー: 無い.docx: "));
}

#[test]
fn partial_xml_reports_hits_and_error() {
    let dir = tempfile::tempdir().unwrap();
    let doc = format!(
        r#"<w:document xmlns:w="{}"><w:body>{}<w:p><w:r><w:t>途中"#,
        common::W_NS,
        p("サーバ")
    );
    DocxBuilder::new()
        .document_xml(&doc)
        .write(dir.path(), "途中.docx");
    docgrep(dir.path())
        .args(["--color", "never", "サーバ", "途中.docx"])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("■ 途中.docx  1件"))
        .stderr(predicate::str::contains("docgrep: エラー: 途中.docx: XML"));
}

// --- output modes ------------------------------------------------------------------

#[test]
fn color_never_has_no_escape_sequences() {
    let dir = setup();
    let out = stdout_of(docgrep(dir.path()).args(["--color", "never", "サーバ", "仕様書.docx"]));
    assert!(!out.contains('\u{1b}'), "{out}");
    let err = docgrep(dir.path())
        .args(["--color", "never", "サーバ", "仕様書.docx"])
        .output()
        .unwrap()
        .stderr;
    assert!(!err.contains(&0x1b));
}

#[test]
fn color_always_has_escape_sequences() {
    let dir = setup();
    let out = stdout_of(docgrep(dir.path()).args(["--color", "always", "サーバ", "仕様書.docx"]));
    assert!(out.contains('\u{1b}'));
}

#[test]
fn auto_color_is_off_when_piped() {
    let dir = setup();
    let out = stdout_of(docgrep(dir.path()).args(["サーバ", "仕様書.docx"]));
    assert!(!out.contains('\u{1b}'));
}

#[test]
fn files_with_matches() {
    let dir = setup();
    sample(dir.path(), "写し.docx");
    DocxBuilder::new()
        .body(&p("無関係"))
        .write(dir.path(), "無関係.docx");
    docgrep(dir.path())
        .args([
            "-l",
            "--color",
            "always",
            "サーバ",
            "仕様書.docx",
            "無関係.docx",
            "写し.docx",
        ])
        .assert()
        .code(0)
        .stdout("仕様書.docx\n写し.docx\n")
        .stderr("");
}

#[test]
fn count_lists_only_files_with_hits() {
    let dir = setup();
    DocxBuilder::new()
        .body(&p("無関係"))
        .write(dir.path(), "無関係.docx");
    docgrep(dir.path())
        .args(["-c", "サーバ", "無関係.docx", "仕様書.docx"])
        .assert()
        .code(0)
        .stdout("仕様書.docx:5\n")
        .stderr("");
}

#[test]
fn output_modes_conflict() {
    let dir = setup();
    docgrep(dir.path())
        .args(["-l", "--json", "サーバ", "仕様書.docx"])
        .assert()
        .code(2);
    docgrep(dir.path())
        .args(["-c", "-l", "サーバ", "仕様書.docx"])
        .assert()
        .code(2);
}

#[test]
fn pattern_starting_with_hyphen_after_double_dash() {
    let dir = tempfile::tempdir().unwrap();
    DocxBuilder::new()
        .body(&p("オプション -foo の説明"))
        .write(dir.path(), "a.docx");
    docgrep(dir.path())
        .args(["-c", "--", "-foo", "a.docx"])
        .assert()
        .code(0)
        .stdout("a.docx:1\n");
}

#[test]
fn ignore_case_and_regex_flags() {
    let dir = tempfile::tempdir().unwrap();
    DocxBuilder::new()
        .body(&(p("ＡＢＣ") + &p("abc") + &p("サーバー") + &p("サーバ")))
        .write(dir.path(), "a.docx");
    let count = |args: &[&str]| stdout_of(docgrep(dir.path()).arg("-c").args(args).arg("a.docx"));
    assert_eq!(count(&["ａｂｃ"]), "");
    assert_eq!(count(&["-i", "ａｂｃ"]), "a.docx:1\n");
    assert_eq!(count(&["-i", "ABC"]), "a.docx:1\n");
    assert_eq!(count(&["サーバ"]), "a.docx:2\n");
    assert_eq!(count(&["-e", "サーバ(?!ー)"]), "a.docx:1\n");
}

#[test]
fn directory_is_walked_in_name_order_and_lock_files_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("docs");
    for name in ["b.docx", "a.docx", "sub/c.DOCX", "~$a.docx"] {
        DocxBuilder::new().body(&p("サーバ")).write(&sub, name);
    }
    common::write_bytes(&sub, "memo.txt", b"x");
    // macOS AppleDouble metadata files are not zip files; they are skipped silently.
    common::write_bytes(&sub, "._a.docx", b"\x00\x05\x16\x07 AppleDouble");
    let expected = ["docs/a.docx", "docs/b.docx", "docs/sub/c.DOCX"]
        .map(|s| native(s) + "\n")
        .concat();
    docgrep(dir.path())
        .args(["-l", "サーバ", "docs"])
        .assert()
        .code(0)
        .stdout(expected)
        .stderr("");
    // Default path is the current directory, shown without "./".
    let expected = ["a.docx", "b.docx", "sub/c.DOCX"]
        .map(|s| native(s) + "\n")
        .concat();
    docgrep(&sub)
        .args(["-l", "サーバ"])
        .assert()
        .code(0)
        .stdout(expected);
}

#[test]
fn summary_goes_to_stderr_in_pretty_mode_only() {
    let dir = setup();
    common::write_cfb(dir.path(), "暗号.docx");
    common::write_bytes(dir.path(), "壊.docx", b"x");
    docgrep(dir.path())
        .args([
            "--color",
            "never",
            "サーバ",
            "仕様書.docx",
            "暗号.docx",
            "壊.docx",
        ])
        .assert()
        .stderr(predicate::str::ends_with(
            "検索 2ファイル / ヒット 1ファイル 5件 / スキップ 1 / エラー 1\n",
        ));
    for flag in ["-l", "-c", "--json"] {
        docgrep(dir.path())
            .args([flag, "サーバ", "仕様書.docx"])
            .assert()
            .stderr("");
    }
}

// --- JSON ----------------------------------------------------------------------------

#[test]
fn json_lines_fields() {
    let dir = setup();
    let out = stdout_of(docgrep(dir.path()).args(["--json", "-C", "3", "サーバ", "仕様書.docx"]));
    let lines: Vec<serde_json::Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 5);

    let first = &lines[0];
    assert_eq!(first["file"], "仕様書.docx");
    assert_eq!(first["format"], "word");
    assert_eq!(first["part"], "body");
    assert!(first["part_label"].is_null());
    assert_eq!(first["page"], 1);
    assert_eq!(first["page_mode"], "explicit");
    assert!(first["heading"].is_null());
    assert!(first["sheet"].is_null());
    assert!(first["sheet_hidden"].is_null());
    assert!(first["cell"].is_null());
    assert_eq!(first["before"], "Web");
    assert_eq!(first["match"], "サーバ");
    assert_eq!(first["after"], "とアプ");
    assert_eq!(first["offset"], 10);

    // Matches are not merged: the second match in the same paragraph is its own line.
    assert_eq!(lines[1]["offset"], 22);

    let cell = &lines[3];
    assert_eq!(cell["part"], "table");
    assert_eq!(cell["part_label"], "表1 1行2列");
    assert_eq!(cell["before"], "");
    // Raw text: the line break stays a newline.
    assert_eq!(cell["after"], "名\n役");

    let tb = &lines[4];
    assert_eq!(tb["part"], "textbox");
    assert_eq!(tb["part_label"], "テキストボックス");
    assert_eq!(tb["after"], "\t構成");
}

#[test]
fn json_paragraph_mode_splits_whole_paragraph() {
    let dir = setup();
    let out = stdout_of(docgrep(dir.path()).args(["--json", "-P", "入室", "仕様書.docx"]));
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["before"], "サーバー室の");
    assert_eq!(v["match"], "入室");
    assert_eq!(v["after"], "手順");
}

#[test]
fn json_has_no_color_even_when_forced() {
    let dir = setup();
    let out = stdout_of(docgrep(dir.path()).args([
        "--json",
        "--color",
        "always",
        "サーバ",
        "仕様書.docx",
    ]));
    assert!(!out.contains('\u{1b}'));
}

#[test]
fn ignored_files_given_explicitly_are_skipped_silently() {
    let dir = setup();
    common::write_bytes(dir.path(), "._仕様書.docx", b"\x00\x05\x16\x07");
    DocxBuilder::new()
        .body(&p("サーバ"))
        .write(dir.path(), "~$仕様書.docx");
    docgrep(dir.path())
        .args([
            "-c",
            "サーバ",
            "._仕様書.docx",
            "~$仕様書.docx",
            "仕様書.docx",
        ])
        .assert()
        .code(0)
        .stdout("仕様書.docx:5\n")
        .stderr("");
}

/// A document with numbered headings (styles use Japanese-Word-like ids).
fn with_headings(dir: &Path, name: &str) {
    let heading_style = |id: &str, lvl: u8| {
        format!(
            r#"<w:style w:type="paragraph" w:styleId="{id}"><w:name w:val="heading {}"/><w:pPr><w:numPr><w:ilvl w:val="{lvl}"/><w:numId w:val="1"/></w:numPr><w:outlineLvl w:val="{lvl}"/></w:pPr></w:style>"#,
            lvl + 1
        )
    };
    let styles = heading_style("1", 0) + &heading_style("2", 1) + &heading_style("3", 2);
    let numbering = r#"<w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1"/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1.%2"/></w:lvl><w:lvl w:ilvl="2"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1.%2.%3"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#;
    let h = |id: &str, text: &str| {
        format!(
            r#"<w:p><w:pPr><w:pStyle w:val="{id}"/></w:pPr>{}</w:p>"#,
            r(text)
        )
    };
    let body = [
        p("サーバ構成資料"),
        h("1", "はじめに"),
        h("1", "システム"),
        h("2", "概要"),
        h("3", "システム構成"),
        p("本システムではWebサーバとアプリケーションサーバを分離する。"),
        format!("<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>", p("サーバ名")),
        h(
            "2",
            "とても長い見出しの例としてサーバの冗長化と障害時の切り替え手順について",
        ),
        p("待機系サーバへ切り替える。"),
    ]
    .concat();
    DocxBuilder::new()
        .styles(&styles)
        .numbering(numbering)
        .body(&body)
        .write(dir, name);
}

#[test]
fn json_heading_fields() {
    let dir = tempfile::tempdir().unwrap();
    with_headings(dir.path(), "見出し.docx");
    let out = stdout_of(docgrep(dir.path()).args(["--json", "サーバ", "見出し.docx"]));
    let lines: Vec<serde_json::Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(lines[0]["heading"].is_null());
    assert_eq!(
        lines[1]["heading"],
        serde_json::json!({"level": 3, "number": "2.1.1", "text": "システム構成"})
    );
    assert_eq!(lines[3]["part"], "table");
    assert_eq!(lines[3]["heading"]["number"], "2.1.1");
    assert_eq!(lines[4]["heading"]["level"], 2);
    assert_eq!(lines[4]["heading"]["number"], "2.2");
}

/// Rendered-mode document: page 2 starts in the middle of the second paragraph.
fn with_pages(dir: &Path, name: &str) {
    let lr = "<w:r><w:lastRenderedPageBreak/></w:r>";
    let body = [
        p("一頁目のサーバ"),
        format!("<w:p>{}{lr}{}</w:p>", r("サーバA"), r("サーバB")),
        p("二頁目のサーバ"),
    ]
    .concat();
    DocxBuilder::new().body(&body).write(dir, name);
}

#[test]
fn json_pages_in_rendered_mode() {
    let dir = tempfile::tempdir().unwrap();
    with_pages(dir.path(), "頁.docx");
    let out = stdout_of(docgrep(dir.path()).args(["--json", "サーバ", "頁.docx"]));
    let pages: Vec<(serde_json::Value, serde_json::Value)> = out
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .map(|v| (v["page"].clone(), v["page_mode"].clone()))
        .collect();
    let expected: Vec<_> = [1, 1, 2, 2]
        .iter()
        .map(|&n| (serde_json::json!(n), serde_json::json!("rendered")))
        .collect();
    assert_eq!(pages, expected);
}

#[test]
fn merged_entry_shows_page_of_first_match() {
    let dir = tempfile::tempdir().unwrap();
    with_pages(dir.path(), "頁.docx");
    let out = stdout_of(docgrep(dir.path()).args(["--color", "never", "サーバ", "頁.docx"]));
    // "サーバA" (page 1) and "サーバB" (page 2) are one entry; it shows page 1.
    assert!(
        out.contains("  p.1   (冒頭)\n        サーバAサーバB\n"),
        "{out}"
    );
}

/// Workbook with a visible and a hidden sheet.
fn workbook(dir: &Path, name: &str) {
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet().set_name("内訳").unwrap();
    ws.write_string(0, 0, "項目").unwrap();
    ws.write_string(13, 2, "サーバ保守費用（年額）").unwrap();
    ws.write_number(13, 3, 1234.0).unwrap();
    let ws = wb.add_worksheet().set_name("旧版").unwrap();
    ws.set_hidden(true);
    ws.write_string(1, 1, "旧サーバ\n廃止済み").unwrap();
    wb.save(dir.join(name)).unwrap();
}

#[test]
fn excel_json_fields() {
    let dir = tempfile::tempdir().unwrap();
    workbook(dir.path(), "見積.xlsx");
    let out = stdout_of(docgrep(dir.path()).args(["--json", "サーバ", "見積.xlsx"]));
    let lines: Vec<serde_json::Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    let v = &lines[0];
    assert_eq!(v["file"], "見積.xlsx");
    assert_eq!(v["format"], "excel");
    assert_eq!(v["part"], "cell");
    assert!(v["part_label"].is_null());
    assert!(v["page"].is_null());
    assert!(v["page_mode"].is_null());
    assert!(v["heading"].is_null());
    assert_eq!(v["sheet"], "内訳");
    assert_eq!(v["sheet_hidden"], false);
    assert_eq!(v["cell"], "C14");
    assert_eq!(v["match"], "サーバ");
    assert_eq!(v["after"], "保守費用（年額）");
    assert_eq!(lines[1]["sheet"], "旧版");
    assert_eq!(lines[1]["sheet_hidden"], true);
    assert_eq!(lines[1]["cell"], "B2");
}

#[test]
fn excel_numbers_are_searchable_without_display_format() {
    let dir = tempfile::tempdir().unwrap();
    workbook(dir.path(), "見積.xlsx");
    docgrep(dir.path())
        .args(["-c", "1234", "見積.xlsx"])
        .assert()
        .code(0)
        .stdout("見積.xlsx:1\n");
}

#[test]
fn word_and_excel_in_one_directory() {
    let dir = tempfile::tempdir().unwrap();
    let docs = dir.path().join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    workbook(&docs, "b_見積.xlsx");
    DocxBuilder::new()
        .body(&p("サーバ"))
        .write(&docs, "a_仕様.docx");
    common::write_cfb(&docs, "c_暗号.xlsx");
    let expected = ["docs/a_仕様.docx:1", "docs/b_見積.xlsx:2"]
        .map(|s| native(s) + "\n")
        .concat();
    docgrep(dir.path())
        .args(["-c", "サーバ", "docs"])
        .assert()
        .code(0)
        .stdout(expected)
        .stderr(predicate::str::contains(
            "暗号化または旧形式のため読めません",
        ));
}

/// A document with every Word part kind containing "サーバ".
fn all_parts(dir: &Path, name: &str) {
    let styles = r#"<w:style w:type="paragraph" w:styleId="1"><w:name w:val="heading 1"/><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="toc1"><w:name w:val="toc 1"/></w:style>"#;
    let sp = |style: &str, text: &str| {
        format!(
            r#"<w:p><w:pPr><w:pStyle w:val="{style}"/></w:pPr>{}</w:p>"#,
            r(text)
        )
    };
    let body = [
        sp("toc1", "1 サーバ構成\t1"),
        sp("1", "サーバ構成"),
        format!(
            r#"<w:p>{}<w:r><w:footnoteReference w:id="3"/></w:r><w:r><w:commentReference w:id="0"/></w:r></w:p>"#,
            r("本文のサーバ")
        ),
        format!("<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>", p("表のサーバ")),
        format!(
            r#"<w:p><w:r><w:pict><v:textbox><w:txbxContent>{}</w:txbxContent></v:textbox></w:pict></w:r></w:p>"#,
            p("箱のサーバ")
        ),
    ]
    .concat();
    DocxBuilder::new()
        .styles(styles)
        .body(&body)
        .footnotes(r#"<w:footnote w:id="3"><w:p><w:r><w:t>脚注のサーバ</w:t></w:r></w:p></w:footnote><w:footnote w:id="5"><w:p><w:r><w:t>参照のないサーバ</w:t></w:r></w:p></w:footnote>"#)
        .endnotes(r#"<w:endnote w:id="1"><w:p><w:r><w:t>文末のサーバ</w:t></w:r></w:p></w:endnote>"#)
        .comments(r#"<w:comment w:id="0" w:author="山田"><w:p><w:r><w:t>コメントのサーバ</w:t></w:r></w:p></w:comment>"#)
        .header(&p("ヘッダーのサーバ"))
        .footer(&p("フッターのサーバ"))
        .write(dir, name);
}

#[test]
fn parts_option_filters_word_parts() {
    let dir = tempfile::tempdir().unwrap();
    all_parts(dir.path(), "全部.docx");
    workbook(dir.path(), "見積.xlsx");
    let count = |parts: &str| {
        stdout_of(docgrep(dir.path()).args([
            "-c",
            "--parts",
            parts,
            "サーバ",
            "全部.docx",
            "見積.xlsx",
        ]))
    };
    assert_eq!(count("all"), "全部.docx:11\n見積.xlsx:2\n");
    assert_eq!(count("body"), "全部.docx:2\n見積.xlsx:2\n");
    assert_eq!(count("toc"), "全部.docx:1\n見積.xlsx:2\n");
    assert_eq!(count("table"), "全部.docx:1\n見積.xlsx:2\n");
    assert_eq!(count("textbox"), "全部.docx:1\n見積.xlsx:2\n");
    assert_eq!(count("note"), "全部.docx:3\n見積.xlsx:2\n");
    assert_eq!(count("comment"), "全部.docx:1\n見積.xlsx:2\n");
    assert_eq!(count("header"), "全部.docx:2\n見積.xlsx:2\n");
    assert_eq!(count("body,header"), "全部.docx:4\n見積.xlsx:2\n");
}

#[test]
fn json_part_names_for_all_parts() {
    let dir = tempfile::tempdir().unwrap();
    all_parts(dir.path(), "全部.docx");
    let out = stdout_of(docgrep(dir.path()).args(["--json", "サーバ", "全部.docx"]));
    let parts: Vec<(String, serde_json::Value)> = out
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .map(|v| {
            (
                v["part"].as_str().unwrap().to_string(),
                v["part_label"].clone(),
            )
        })
        .collect();
    let names: Vec<_> = parts.iter().map(|p| p.0.as_str()).collect();
    assert_eq!(
        names,
        [
            "toc", "body", "body", "table", "textbox", "footnote", "footnote", "endnote",
            "comment", "header", "footer"
        ]
    );
    assert_eq!(parts[5].1, "脚注3");
    assert_eq!(parts[7].1, "文末脚注1");
    assert_eq!(parts[8].1, "コメント: 山田");
}

#[test]
fn hidden_directories_are_skipped_and_max_depth_applies() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("docs");
    for name in [
        "a.docx",
        ".git/x.docx",
        ".hidden/b.docx",
        "sub/c.docx",
        "sub/deep/d.docx",
    ] {
        DocxBuilder::new().body(&p("サーバ")).write(&root, name);
    }
    let files = |extra: &[&str]| {
        let out = stdout_of(
            docgrep(dir.path())
                .arg("-l")
                .args(extra)
                .args(["サーバ", "docs"]),
        );
        out.lines()
            .map(|l| l.replace('\\', "/"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        files(&[]),
        ["docs/a.docx", "docs/sub/c.docx", "docs/sub/deep/d.docx"]
    );
    assert_eq!(files(&["--max-depth", "1"]), ["docs/a.docx"]);
    assert_eq!(
        files(&["--max-depth", "2"]),
        ["docs/a.docx", "docs/sub/c.docx"]
    );
    // A hidden directory given explicitly is searched.
    let out = stdout_of(docgrep(dir.path()).args(["-l", "サーバ", "docs/.hidden"]));
    assert_eq!(out.replace('\\', "/"), "docs/.hidden/b.docx\n");
}

#[cfg(unix)]
#[test]
fn symlinks_are_not_followed_during_walk() {
    let dir = tempfile::tempdir().unwrap();
    let other = dir.path().join("other");
    DocxBuilder::new()
        .body(&p("サーバ"))
        .write(&other, "x.docx");
    let docs = dir.path().join("docs");
    DocxBuilder::new().body(&p("サーバ")).write(&docs, "a.docx");
    std::os::unix::fs::symlink(&other, docs.join("link")).unwrap();
    std::os::unix::fs::symlink(other.join("x.docx"), docs.join("y.docx")).unwrap();
    docgrep(dir.path())
        .args(["-l", "サーバ", "docs"])
        .assert()
        .stdout("docs/a.docx\n");
    // Given explicitly, a symlinked file is searched.
    docgrep(dir.path())
        .args(["-l", "サーバ", "docs/y.docx"])
        .assert()
        .stdout("docs/y.docx\n");
}

#[test]
fn parallel_output_keeps_argument_order() {
    let dir = tempfile::tempdir().unwrap();
    let docs = dir.path().join("docs");
    for i in 0..40 {
        // Larger files first so that later ones tend to finish earlier.
        let body = p("サーバ").repeat(if i < 5 { 3000 } else { 1 });
        DocxBuilder::new()
            .body(&body)
            .write(&docs, &format!("f{i:02}.docx"));
    }
    common::write_bytes(&docs, "f20_壊れ.docx", b"x");
    let run = |j: &str| {
        let out = docgrep(dir.path())
            .args(["-c", "-j", j, "サーバ", "docs"])
            .output()
            .unwrap();
        (
            String::from_utf8(out.stdout).unwrap(),
            String::from_utf8(out.stderr).unwrap(),
            out.status.code(),
        )
    };
    let sequential = run("1");
    assert_eq!(sequential.2, Some(2));
    assert!(sequential.1.contains("f20_壊れ.docx"));
    for j in ["0", "4", "16"] {
        assert_eq!(run(j), sequential, "-j {j}");
    }
    let names: Vec<_> = sequential
        .0
        .lines()
        .map(|l| l.split(':').next().unwrap().to_string())
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
    assert_eq!(names.len(), 40);
}

// --- snapshots -------------------------------------------------------------------------

#[test]
fn snapshot_pretty_all_parts() {
    let dir = tempfile::tempdir().unwrap();
    all_parts(dir.path(), "全部.docx");
    insta::assert_snapshot!(stdout_of(docgrep(dir.path()).args([
        "--color",
        "never",
        "サーバ",
        "全部.docx"
    ])));
}

#[test]
fn snapshot_pretty_excel() {
    let dir = tempfile::tempdir().unwrap();
    workbook(dir.path(), "見積.xlsx");
    DocxBuilder::new()
        .body(&p("サーバの一覧は見積.xlsxを参照"))
        .write(dir.path(), "仕様.docx");
    insta::assert_snapshot!(stdout_of(docgrep(dir.path()).args([
        "--color",
        "never",
        "サーバ",
        "仕様.docx",
        "見積.xlsx"
    ])));
    insta::assert_snapshot!(stdout_of(docgrep(dir.path()).args([
        "--color",
        "always",
        "サーバ",
        "見積.xlsx"
    ])));
}

#[test]
fn snapshot_pretty_rendered_pages() {
    let dir = tempfile::tempdir().unwrap();
    with_pages(dir.path(), "頁.docx");
    insta::assert_snapshot!(stdout_of(docgrep(dir.path()).args([
        "--color",
        "never",
        "-C",
        "3",
        "サーバ",
        "頁.docx"
    ])));
}

#[test]
fn snapshot_pretty_headings() {
    let dir = tempfile::tempdir().unwrap();
    with_headings(dir.path(), "見出し.docx");
    insta::assert_snapshot!(stdout_of(docgrep(dir.path()).args([
        "--color",
        "never",
        "サーバ",
        "見出し.docx"
    ])));
    insta::assert_snapshot!(stdout_of(docgrep(dir.path()).args([
        "--color",
        "always",
        "サーバ",
        "見出し.docx"
    ])));
}

#[test]
fn snapshot_pretty_plain() {
    let dir = setup();
    insta::assert_snapshot!(stdout_of(docgrep(dir.path()).args([
        "--color",
        "never",
        "サーバ",
        "仕様書.docx"
    ])));
}

#[test]
fn snapshot_pretty_color() {
    let dir = setup();
    insta::assert_snapshot!(stdout_of(docgrep(dir.path()).args([
        "--color",
        "always",
        "サーバ",
        "仕様書.docx"
    ])));
}

#[test]
fn snapshot_pretty_paragraph_and_small_context() {
    let dir = setup();
    sample(dir.path(), "写し.docx");
    insta::assert_snapshot!(stdout_of(docgrep(dir.path()).args([
        "--color",
        "never",
        "-C",
        "2",
        "サーバ",
        "仕様書.docx",
        "写し.docx"
    ])));
    insta::assert_snapshot!(stdout_of(docgrep(dir.path()).args([
        "--color",
        "never",
        "-P",
        "負荷",
        "仕様書.docx"
    ])));
}

#[test]
fn snapshot_help() {
    let dir = tempfile::tempdir().unwrap();
    insta::assert_snapshot!(stdout_of(docgrep(dir.path()).arg("--help")));
}

#[test]
fn help_is_japanese() {
    let dir = setup();
    docgrep(dir.path())
        .arg("--help")
        .assert()
        .code(0)
        .stdout(predicate::str::contains("使い方:"))
        .stdout(predicate::str::contains("大文字小文字を区別しない"))
        .stdout(predicate::str::contains("docgrep -- -foo ."));
}
