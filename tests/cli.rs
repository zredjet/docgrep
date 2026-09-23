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
    let expected = ["docs/a.docx", "docs/b.docx", "docs/sub/c.DOCX"]
        .map(|s| Path::new(s).display().to_string() + "\n")
        .concat();
    docgrep(dir.path())
        .args(["-l", "サーバ", "docs"])
        .assert()
        .code(0)
        .stdout(expected)
        .stderr("");
    // Default path is the current directory, shown without "./".
    let expected = ["a.docx", "b.docx", "sub/c.DOCX"]
        .map(|s| Path::new(s).display().to_string() + "\n")
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
    assert!(first["page"].is_null());
    assert!(first["page_mode"].is_null());
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

// --- snapshots -------------------------------------------------------------------------

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
