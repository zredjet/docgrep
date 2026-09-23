//! Footnotes, endnotes, comments, headers, footers and TOC labels (SPEC §6.4).

mod common;

use common::{DocxBuilder, p, r};
use docgrep::error::FileError;
use docgrep::model::{Extracted, PageMode, Part};

fn extract(b: &DocxBuilder) -> Extracted {
    let dir = tempfile::tempdir().unwrap();
    let path = b.write(dir.path(), "t.docx");
    docgrep::word::extract(&path).unwrap()
}

/// (text, part, page, heading text) of every unit.
fn units(b: &DocxBuilder) -> Vec<(String, Part, Option<u32>, Option<String>)> {
    let x = extract(b);
    assert!(x.partial_error.is_none(), "{:?}", x.partial_error);
    x.units
        .into_iter()
        .map(|u| {
            let page = u.page_at(0);
            let heading = u.location.heading.map(|h| h.text);
            (u.text, u.location.part, page, heading)
        })
        .collect()
}

fn fnote(id: &str) -> Part {
    Part::Footnote { id: id.into() }
}

fn enote(id: &str) -> Part {
    Part::Endnote { id: id.into() }
}

fn comment(id: &str, author: &str) -> Part {
    Part::Comment {
        id: id.into(),
        author: author.into(),
    }
}

const SEPARATORS: &str = r#"<w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r><w:r><w:t>区切り</w:t></w:r></w:p></w:footnote><w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote><w:footnote w:type="continuationNotice" w:id="9"><w:p><w:r><w:t>続き</w:t></w:r></w:p></w:footnote>"#;

fn note(tag: &str, id: &str, text: &str) -> String {
    format!(r#"<w:{tag} w:id="{id}">{}</w:{tag}>"#, p(text))
}

fn ref_run(tag: &str, id: &str) -> String {
    format!(r#"<w:r><w:{tag} w:id="{id}"/></w:r>"#)
}

fn heading_styles() -> &'static str {
    r#"<w:style w:type="paragraph" w:styleId="1"><w:name w:val="heading 1"/><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="toc1"><w:name w:val="toc 1"/></w:style>"#
}

fn sp(style: &str, text: &str) -> String {
    format!(
        r#"<w:p><w:pPr><w:pStyle w:val="{style}"/></w:pPr>{}</w:p>"#,
        r(text)
    )
}

#[test]
fn notes_take_page_and_heading_from_their_reference() {
    let body = [
        sp("1", "概要"),
        format!(
            "<w:p>{}{}</w:p>",
            r("本文"),
            ref_run("footnoteReference", "1")
        ),
        format!(
            r#"<w:p><w:r><w:br w:type="page"/></w:r>{}{}</w:p>"#,
            r("二頁目"),
            ref_run("endnoteReference", "3")
        ),
    ]
    .concat();
    let b = DocxBuilder::new()
        .styles(heading_styles())
        .body(&body)
        .footnotes(
            &(SEPARATORS.to_string()
                + &note("footnote", "1", "脚注本文")
                + &note("footnote", "2", "参照なし")),
        )
        .endnotes(&note("endnote", "3", "文末脚注本文"));
    let got = units(&b);
    assert_eq!(
        got[3..],
        [
            ("脚注本文".into(), fnote("1"), Some(1), Some("概要".into())),
            ("参照なし".into(), fnote("2"), None, None),
            (
                "文末脚注本文".into(),
                enote("3"),
                Some(2),
                Some("概要".into())
            ),
        ]
    );
    let x = extract(&b);
    let note = x.units.iter().find(|u| u.text == "脚注本文").unwrap();
    assert_eq!(note.location.page_mode, Some(PageMode::Explicit));
    assert!(note.page_breaks.is_empty());
}

#[test]
fn comments_are_labelled_with_author() {
    let body = format!(
        r#"<w:p><w:commentRangeStart w:id="0"/>{}<w:commentRangeEnd w:id="0"/>{}</w:p>"#,
        r("指摘箇所"),
        ref_run("commentReference", "0")
    );
    let comments = r#"<w:comment w:id="0" w:author="山田" w:initials="Y"><w:p><w:r><w:t>要確認</w:t></w:r></w:p></w:comment><w:comment w:id="1"><w:p><w:r><w:t>作成者なし</w:t></w:r></w:p></w:comment>"#;
    let got = units(&DocxBuilder::new().body(&body).comments(comments));
    assert_eq!(
        got[1..],
        [
            ("要確認".into(), comment("0", "山田"), Some(1), None),
            ("作成者なし".into(), comment("1", ""), None, None),
        ]
    );
    assert_eq!(comment("0", "山田").label().unwrap(), "コメント: 山田");
    assert_eq!(comment("1", "").label().unwrap(), "コメント");
}

#[test]
fn headers_and_footers_have_no_page_or_heading() {
    let b = DocxBuilder::new()
        .styles(heading_styles())
        .body(&sp("1", "章"))
        .header(&p("社外秘"))
        .footer(&p("フッター文"));
    let got = units(&b);
    assert_eq!(
        got[1..],
        [
            ("社外秘".into(), Part::Header, None, None),
            ("フッター文".into(), Part::Footer, None, None),
        ]
    );
}

#[test]
fn output_order_is_body_notes_comments_headers_footers() {
    let b = DocxBuilder::new()
        .body(&format!(
            "<w:p>{}{}{}</w:p>",
            r("本文"),
            ref_run("footnoteReference", "1"),
            ref_run("commentReference", "0")
        ))
        .footer(&p("F"))
        .header(&p("H"))
        .comments(
            r#"<w:comment w:id="0" w:author="a"><w:p><w:r><w:t>C</w:t></w:r></w:p></w:comment>"#,
        )
        .endnotes(&note("endnote", "1", "E"))
        .footnotes(&note("footnote", "1", "N"));
    let texts: Vec<_> = units(&b).into_iter().map(|u| u.0).collect();
    assert_eq!(texts, ["本文", "N", "E", "C", "H", "F"]);
}

#[test]
fn header_parts_in_natural_order_and_shared_parts_once() {
    let mut b = DocxBuilder::new().body(&p("本文"));
    for i in 1..=10 {
        b = b.header(&p(&format!("H{i}")));
    }
    // Two sections using header1.xml: one extra relationship to the same target.
    let rel = r#"http://schemas.openxmlformats.org/officeDocument/2006/relationships/header"#;
    let mut rels = String::from(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for i in (1..=10).rev() {
        rels.push_str(&format!(
            r#"<Relationship Id="rId{i}" Type="{rel}" Target="header{i}.xml"/>"#
        ));
    }
    rels.push_str(&format!(
        r#"<Relationship Id="rId99" Type="{rel}" Target="header1.xml"/>"#
    ));
    rels.push_str("</Relationships>");
    let b = b.raw("word/_rels/document.xml.rels", rels);
    let texts: Vec<_> = units(&b).into_iter().skip(1).map(|u| u.0).collect();
    let expected: Vec<String> = (1..=10).map(|i| format!("H{i}")).collect();
    assert_eq!(texts, expected);
}

#[test]
fn tables_and_textboxes_in_other_parts_take_the_part_label() {
    let tbl = format!(
        "<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>",
        p("ヘッダー表")
    );
    let textbox = format!(
        r#"<w:p><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wps:txbx><w:txbxContent>{}</w:txbxContent></wps:txbx></w:drawing></mc:Choice><mc:Fallback/></mc:AlternateContent></w:r></w:p>"#,
        p("ロゴ文字")
    );
    let footnote = format!(
        r#"<w:footnote w:id="1"><w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl></w:footnote>"#,
        p("脚注の表")
    );
    let referencing = format!(
        "<w:p>{}{}</w:p>",
        r("本文"),
        ref_run("footnoteReference", "1")
    );
    let body = format!(
        "{referencing}<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>",
        p("本文の表")
    );
    let got = units(
        &DocxBuilder::new()
            .body(&body)
            .header(&(tbl + &textbox))
            .footnotes(&footnote),
    );
    let parts: Vec<_> = got.into_iter().map(|u| (u.0, u.1)).collect();
    assert_eq!(
        parts,
        [
            ("本文".into(), Part::Body),
            // Tables in other parts do not consume table numbers.
            (
                "本文の表".into(),
                Part::Table {
                    index: 1,
                    row: 1,
                    col: 1
                }
            ),
            ("脚注の表".into(), fnote("1")),
            ("ヘッダー表".into(), Part::Header),
            ("".into(), Part::Header),
            ("ロゴ文字".into(), Part::Header),
        ]
    );
}

#[test]
fn toc_paragraphs_are_labelled() {
    let body = [
        sp("toc1", "1 概要\t1"),
        format!(
            "<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>",
            sp("toc1", "表の中")
        ),
        sp("1", "概要"),
    ]
    .concat();
    let got = units(&DocxBuilder::new().styles(heading_styles()).body(&body));
    assert_eq!(got[0].1, Part::Toc);
    assert_eq!(got[0].2, Some(1));
    assert!(matches!(got[1].1, Part::Table { .. }));
    assert_eq!(got[2].1, Part::Body);
}

#[test]
fn field_state_is_reset_for_each_part() {
    // The body ends inside a field code; the footnote must still be read.
    let body = format!(
        r#"<w:p>{}<w:r><w:fldChar w:fldCharType="begin"/><w:instrText>X</w:instrText></w:r></w:p>"#,
        ref_run("footnoteReference", "1")
    );
    let got = units(
        &DocxBuilder::new()
            .body(&body)
            .footnotes(&note("footnote", "1", "脚注")),
    );
    assert_eq!(got.last().unwrap().0, "脚注");
}

#[test]
fn references_in_text_boxes_use_the_anchor() {
    let textbox = format!(
        r#"<w:r><w:pict><v:textbox><w:txbxContent><w:p>{}{}</w:p></w:txbxContent></v:textbox></w:pict></w:r>"#,
        r("箱"),
        ref_run("footnoteReference", "1")
    );
    let body = [
        sp("1", "章"),
        format!(
            "<w:p>{}<w:r><w:lastRenderedPageBreak/></w:r>{}{textbox}</w:p>",
            r("前"),
            r("後")
        ),
    ]
    .concat();
    let got = units(
        &DocxBuilder::new()
            .styles(heading_styles())
            .body(&body)
            .footnotes(&note("footnote", "1", "箱の脚注")),
    );
    let last = got.last().unwrap();
    assert_eq!(last.0, "箱の脚注");
    assert_eq!(last.2, Some(2));
    assert_eq!(last.3.as_deref(), Some("章"));
}

#[test]
fn rendered_page_of_reference() {
    let body = format!(
        "<w:p>{}<w:r><w:lastRenderedPageBreak/></w:r>{}{}</w:p>",
        r("一頁"),
        r("二頁"),
        ref_run("footnoteReference", "1")
    );
    let x = extract(
        &DocxBuilder::new()
            .body(&body)
            .footnotes(&note("footnote", "1", "脚注")),
    );
    let n = x.units.last().unwrap();
    assert_eq!(n.location.page_at_start, Some(2));
    assert_eq!(n.location.page_mode, Some(PageMode::Rendered));
}

#[test]
fn strict_notes_and_headers() {
    let b = DocxBuilder::new()
        .strict()
        .body(&format!(
            "<w:p>{}{}</w:p>",
            r("本文"),
            ref_run("footnoteReference", "1")
        ))
        .footnotes(&note("footnote", "1", "厳格脚注"))
        .header(&p("厳格ヘッダー"));
    let parts: Vec<_> = units(&b).into_iter().map(|u| (u.0, u.1)).collect();
    assert_eq!(
        parts[1..],
        [
            ("厳格脚注".into(), fnote("1")),
            ("厳格ヘッダー".into(), Part::Header),
        ]
    );
}

#[test]
fn broken_secondary_part_keeps_the_rest() {
    let b = DocxBuilder::new()
        .body(&p("本文"))
        .footnotes("")
        .raw("word/footnotes.xml", "<w:footnotes xmlns:w=\"x\"><broken")
        .header(&p("ヘッダー"));
    let x = extract(&b);
    assert!(matches!(
        x.partial_error,
        Some(FileError::Xml { ref part, .. }) if part == "word/footnotes.xml"
    ));
    let texts: Vec<_> = x.units.iter().map(|u| u.text.as_str()).collect();
    assert_eq!(texts, ["本文", "ヘッダー"]);
}

#[test]
fn missing_secondary_part_is_ignored() {
    let b = DocxBuilder::new()
        .body(&p("本文"))
        .header(&p("消えた"))
        .without("word/header1.xml");
    let texts: Vec<_> = units(&b).into_iter().map(|u| u.0).collect();
    assert_eq!(texts, ["本文"]);
}
