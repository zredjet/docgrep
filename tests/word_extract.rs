//! Extraction rules of SPEC §6.1–6.3, checked through the library API.

mod common;

use common::{DocxBuilder, p, r};
use docgrep::error::FileError;
use docgrep::model::{Extracted, Part};

fn extract(builder: &DocxBuilder) -> Extracted {
    let dir = tempfile::tempdir().unwrap();
    let path = builder.write(dir.path(), "t.docx");
    docgrep::word::extract(&path).unwrap()
}

fn texts_of(builder: &DocxBuilder) -> Vec<String> {
    let x = extract(builder);
    assert!(x.partial_error.is_none(), "{:?}", x.partial_error);
    x.units.into_iter().map(|u| u.text).collect()
}

fn texts(body: &str) -> Vec<String> {
    texts_of(&DocxBuilder::new().body(body))
}

/// Text of a single-paragraph body.
fn para(inner: &str) -> String {
    let t = texts(&format!("<w:p>{inner}</w:p>"));
    assert_eq!(t.len(), 1, "{t:?}");
    t.into_iter().next().unwrap()
}

fn parts(body: &str) -> Vec<(String, Part)> {
    extract(&DocxBuilder::new().body(body))
        .units
        .into_iter()
        .map(|u| (u.text, u.location.part))
        .collect()
}

// --- basic runs -----------------------------------------------------------------

#[test]
fn paragraphs_become_units_in_order() {
    assert_eq!(texts(&(p("一") + &p("二"))), vec!["一", "二"]);
}

#[test]
fn split_runs_are_joined() {
    assert_eq!(para(&(r("サー") + &r("バ"))), "サーバ");
}

#[test]
fn empty_paragraph_is_a_unit() {
    assert_eq!(texts("<w:p/><w:p></w:p>"), vec!["", ""]);
}

#[test]
fn whitespace_is_preserved() {
    assert_eq!(para(&r("  a \u{3000} b  ")), "  a \u{3000} b  ");
}

#[test]
fn entities_and_char_refs_are_unescaped() {
    assert_eq!(
        para("<w:r><w:t>a&amp;b&lt;c&gt;&quot;&apos;&#x30B5;&#12540;</w:t></w:r>"),
        "a&b<c>\"'サー"
    );
}

#[test]
fn cdata_is_text() {
    assert_eq!(para("<w:r><w:t><![CDATA[<x>&]]></w:t></w:r>"), "<x>&");
}

// --- special run content --------------------------------------------------------

#[test]
fn tab_and_ptab_become_tab() {
    assert_eq!(
        para(
            r#"<w:r><w:t>a</w:t><w:tab/><w:t>b</w:t><w:ptab w:relativeTo="margin" w:alignment="right" w:leader="none"/><w:t>c</w:t></w:r>"#
        ),
        "a\tb\tc"
    );
}

#[test]
fn breaks_become_newline() {
    assert_eq!(
        para(
            r#"<w:r><w:t>a</w:t><w:br/><w:t>b</w:t><w:br w:type="textWrapping"/><w:t>c</w:t><w:br w:type="column"/><w:t>d</w:t><w:br w:type="page"/><w:t>e</w:t><w:cr/><w:t>f</w:t></w:r>"#
        ),
        "a\nb\nc\nd\ne\nf"
    );
}

#[test]
fn hyphens() {
    assert_eq!(
        para("<w:r><w:t>a</w:t><w:noBreakHyphen/><w:t>b</w:t><w:softHyphen/><w:t>c</w:t></w:r>"),
        "a-bc"
    );
}

#[test]
fn sym_adds_character_as_is() {
    assert_eq!(
        para(
            r#"<w:r><w:t>a</w:t><w:sym w:font="Wingdings" w:char="F0E0"/><w:sym w:font="MS Mincho" w:char="2460"/></w:r>"#
        ),
        "a\u{F0E0}①"
    );
}

#[test]
fn invalid_sym_is_ignored() {
    assert_eq!(para(r#"<w:r><w:sym w:char="zz"/><w:t>a</w:t></w:r>"#), "a");
}

#[test]
fn last_rendered_page_break_adds_no_text() {
    assert_eq!(
        para("<w:r><w:t>サー</w:t><w:lastRenderedPageBreak/><w:t>バ</w:t></w:r>"),
        "サーバ"
    );
}

#[test]
fn hidden_text_is_included() {
    assert_eq!(
        para(r#"<w:r><w:rPr><w:vanish/></w:rPr><w:t>隠し</w:t></w:r>"#),
        "隠し"
    );
}

#[test]
fn note_and_comment_references_add_no_text() {
    assert_eq!(
        para(
            r#"<w:r><w:t>本文</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r><w:r><w:endnoteReference w:id="2"/></w:r><w:r><w:commentReference w:id="0"/></w:r>"#
        ),
        "本文"
    );
}

// --- revisions ------------------------------------------------------------------

#[test]
fn deleted_text_is_excluded_and_inserted_included() {
    assert_eq!(
        para(
            r#"<w:r><w:t>サー</w:t></w:r><w:del w:id="1" w:author="a"><w:r><w:delText>ビス</w:delText></w:r></w:del><w:ins w:id="2" w:author="a"><w:r><w:t>バ</w:t></w:r></w:ins>"#
        ),
        "サーバ"
    );
}

#[test]
fn moves_keep_destination_only() {
    assert_eq!(
        para(
            r#"<w:moveFrom w:id="1" w:author="a"><w:r><w:t>旧</w:t></w:r></w:moveFrom><w:moveTo w:id="2" w:author="a"><w:r><w:t>新</w:t></w:r></w:moveTo>"#
        ),
        "新"
    );
}

#[test]
fn plain_t_inside_del_is_excluded() {
    assert_eq!(
        para(
            r#"<w:del w:id="1" w:author="a"><w:r><w:t>x</w:t></w:r></w:del><w:r><w:t>y</w:t></w:r>"#
        ),
        "y"
    );
}

#[test]
fn paragraph_mark_revision_in_ppr_does_not_start_deletion() {
    assert_eq!(
        texts(
            r#"<w:p><w:pPr><w:rPr><w:del w:id="1" w:author="a"/></w:rPr></w:pPr><w:r><w:t>残る</w:t></w:r></w:p><w:p><w:r><w:t>次</w:t></w:r></w:p>"#
        ),
        vec!["残る", "次"]
    );
    // Non-empty form of the same marker.
    assert_eq!(
        para(
            r#"<w:pPr><w:rPr><w:del w:id="1" w:author="a"></w:del></w:rPr></w:pPr><w:r><w:t>残る</w:t></w:r>"#
        ),
        "残る"
    );
}

#[test]
fn tab_stops_in_ppr_are_not_tabs() {
    assert_eq!(
        para(
            r#"<w:pPr><w:tabs><w:tab w:val="left" w:pos="840"/></w:tabs></w:pPr><w:r><w:t>a</w:t></w:r>"#
        ),
        "a"
    );
}

// --- fields -----------------------------------------------------------------------

const FIELD: &str = r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> HYPERLINK "http://example.com" </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>表示</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>"#;

#[test]
fn field_code_excluded_result_included() {
    assert_eq!(para(&(r("前") + FIELD + &r("後"))), "前表示後");
}

#[test]
fn text_between_begin_and_separate_is_excluded() {
    assert_eq!(
        para(
            r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:t>コード</w:t></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>結果</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>"#
        ),
        "結果"
    );
}

#[test]
fn field_without_separate_is_fully_excluded() {
    assert_eq!(
        para(
            r#"<w:r><w:t>a</w:t><w:fldChar w:fldCharType="begin"/><w:t>x</w:t><w:fldChar w:fldCharType="end"/><w:t>b</w:t></w:r>"#
        ),
        "ab"
    );
}

#[test]
fn nested_fields() {
    // Outer field whose code contains an inner field; only the outer result shows.
    let inner = r#"<w:fldChar w:fldCharType="begin"/><w:instrText>PAGE</w:instrText><w:fldChar w:fldCharType="separate"/><w:t>内側結果</w:t><w:fldChar w:fldCharType="end"/>"#;
    let body = format!(
        r#"<w:r><w:fldChar w:fldCharType="begin"/><w:instrText>IF </w:instrText>{inner}<w:fldChar w:fldCharType="separate"/><w:t>外側結果</w:t><w:fldChar w:fldCharType="end"/></w:r>"#
    );
    assert_eq!(para(&body), "外側結果");

    // Inner field inside the outer result: its result is shown.
    let body = format!(
        r#"<w:r><w:fldChar w:fldCharType="begin"/><w:instrText>X</w:instrText><w:fldChar w:fldCharType="separate"/><w:t>外</w:t>{inner}<w:fldChar w:fldCharType="end"/></w:r>"#
    );
    assert_eq!(para(&body), "外内側結果");
}

#[test]
fn field_state_carries_across_paragraphs() {
    let body = r#"
      <w:p><w:r><w:fldChar w:fldCharType="begin"/><w:instrText>TOC \o "1-3"</w:instrText><w:fldChar w:fldCharType="separate"/><w:t>目次1</w:t></w:r></w:p>
      <w:p><w:r><w:t>目次2</w:t></w:r></w:p>
      <w:p><w:r><w:t>目次3</w:t><w:fldChar w:fldCharType="end"/></w:r><w:r><w:t>後</w:t></w:r></w:p>
      <w:p><w:r><w:fldChar w:fldCharType="begin"/><w:instrText>X</w:instrText></w:r></w:p>
      <w:p><w:r><w:t>コードの続き</w:t><w:fldChar w:fldCharType="separate"/><w:t>結果</w:t><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    assert_eq!(texts(body), vec!["目次1", "目次2", "目次3後", "", "結果"]);
}

#[test]
fn fld_char_inside_deletion_is_ignored() {
    let body = r#"<w:r><w:t>a</w:t></w:r><w:del w:id="1" w:author="x"><w:r><w:fldChar w:fldCharType="begin"/></w:r></w:del><w:r><w:t>b</w:t></w:r>"#;
    assert_eq!(para(body), "ab");
}

#[test]
fn fld_simple_children_are_included() {
    assert_eq!(
        para(r#"<w:fldSimple w:instr=" PAGE "><w:r><w:t>12</w:t></w:r></w:fldSimple>"#),
        "12"
    );
}

#[test]
fn del_instr_text_is_excluded() {
    assert_eq!(
        para(r#"<w:r><w:delInstrText>PAGE</w:delInstrText><w:t>a</w:t></w:r>"#),
        "a"
    );
}

// --- containers -------------------------------------------------------------------

#[test]
fn transparent_containers_are_descended() {
    let body = r#"<w:hyperlink r:id="rId9"><w:r><w:t>リンク</w:t></w:r></w:hyperlink><w:smartTag w:uri="u" w:element="e"><w:smartTagPr/><w:r><w:t>スマート</w:t></w:r></w:smartTag><w:customXml w:element="c"><w:customXmlPr/><w:r><w:t>カスタム</w:t></w:r></w:customXml><w:sdt><w:sdtPr><w:alias w:val="タイトル"/><w:placeholder><w:docPart w:val="x"/></w:placeholder></w:sdtPr><w:sdtContent><w:r><w:t>コントロール</w:t></w:r></w:sdtContent></w:sdt>"#;
    assert_eq!(para(body), "リンクスマートカスタムコントロール");
}

#[test]
fn block_level_sdt_paragraphs() {
    let body = format!(
        "<w:sdt><w:sdtPr/><w:sdtContent>{}{}</w:sdtContent></w:sdt>",
        p("一"),
        p("二")
    );
    assert_eq!(texts(&body), vec!["一", "二"]);
}

#[test]
fn ruby_text_is_excluded() {
    let body = r#"<w:r><w:ruby><w:rubyPr><w:rubyAlign w:val="distributeSpace"/></w:rubyPr><w:rt><w:r><w:t>さーば</w:t></w:r></w:rt><w:rubyBase><w:r><w:t>鯖</w:t></w:r></w:rubyBase></w:ruby></w:r>"#;
    assert_eq!(para(body), "鯖");
}

#[test]
fn math_text_is_included() {
    let body = r#"<m:oMath><m:r><w:rPr><w:rFonts w:ascii="Cambria Math"/></w:rPr><m:t>x=1</m:t></m:r></m:oMath>"#;
    assert_eq!(para(body), "x=1");
}

// --- text boxes and alternate content -----------------------------------------------

fn dml_textbox(inner: &str) -> String {
    format!(
        r#"<w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wp:anchor><a:graphic><a:graphicData><wps:wsp><wps:txbx><w:txbxContent>{inner}</w:txbxContent></wps:txbx></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></mc:Choice><mc:Fallback><w:pict><v:shape><v:textbox><w:txbxContent>{inner}</w:txbxContent></v:textbox></v:shape></w:pict></mc:Fallback></mc:AlternateContent></w:r>"#
    )
}

#[test]
fn textbox_is_separate_and_not_duplicated() {
    let body = format!(
        "<w:p>{}{}{}</w:p>{}",
        r("前"),
        dml_textbox(&(p("箱1") + &p("箱2"))),
        r("後"),
        p("次")
    );
    assert_eq!(
        parts(&body),
        vec![
            ("前後".to_string(), Part::Body),
            ("箱1".to_string(), Part::TextBox),
            ("箱2".to_string(), Part::TextBox),
            ("次".to_string(), Part::Body),
        ]
    );
}

#[test]
fn vml_textbox_outside_alternate_content() {
    let body = format!(
        r#"<w:p>{}<w:r><w:pict><v:shape><v:textbox><w:txbxContent>{}</w:txbxContent></v:textbox></v:shape></w:pict></w:r></w:p>"#,
        r("外"),
        p("箱")
    );
    assert_eq!(
        parts(&body),
        vec![
            ("外".to_string(), Part::Body),
            ("箱".to_string(), Part::TextBox),
        ]
    );
}

#[test]
fn fallback_only_text_is_ignored() {
    let body = r#"<w:p><w:r><mc:AlternateContent><mc:Choice Requires="w14"><w:t>選択</w:t></mc:Choice><mc:Fallback><w:t>代替</w:t></mc:Fallback></mc:AlternateContent></w:r></w:p>"#;
    assert_eq!(texts(body), vec!["選択"]);
}

#[test]
fn only_first_choice_is_processed() {
    let body = r#"<w:p><w:r><mc:AlternateContent><mc:Choice Requires="w14"><w:t>一</w:t></mc:Choice><mc:Choice Requires="w15"><w:t>二</w:t></mc:Choice></mc:AlternateContent></w:r></w:p>"#;
    assert_eq!(texts(body), vec!["一"]);
}

#[test]
fn textbox_does_not_inherit_outer_field_state() {
    // Anchor sits inside a field code; the text box is its own story.
    let body = format!(
        r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r>{}<w:r><w:fldChar w:fldCharType="separate"/><w:t>結果</w:t><w:fldChar w:fldCharType="end"/></w:r></w:p>"#,
        dml_textbox(&p("箱"))
    );
    assert_eq!(texts(&body), vec!["結果", "箱"]);
}

#[test]
fn nested_textbox_order() {
    let inner_box = dml_textbox(&p("内箱"));
    let outer_box = dml_textbox(&format!("<w:p>{}{}</w:p>", r("外箱"), inner_box));
    let body = format!("<w:p>{}{}</w:p>", r("本文"), outer_box);
    assert_eq!(texts(&body), vec!["本文", "外箱", "内箱"]);
}

// --- tables ---------------------------------------------------------------------------

fn tc(inner: &str) -> String {
    format!("<w:tc><w:tcPr><w:tcW w:w=\"0\" w:type=\"auto\"/></w:tcPr>{inner}</w:tc>")
}

fn tbl(rows: &[Vec<String>]) -> String {
    let rows: String = rows
        .iter()
        .map(|cells| format!("<w:tr><w:trPr/>{}</w:tr>", cells.concat()))
        .collect();
    format!("<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w=\"100\"/></w:tblGrid>{rows}</w:tbl>")
}

fn table_part(index: u32, row: u32, col: u32) -> Part {
    Part::Table { index, row, col }
}

#[test]
fn table_cells_are_labelled() {
    let body = p("前")
        + &tbl(&[
            vec![tc(&p("A1")), tc(&p("B1"))],
            vec![tc(&(p("A2a") + &p("A2b"))), tc(&p("B2"))],
        ])
        + &p("後")
        + &tbl(&[vec![tc(&p("X"))]]);
    assert_eq!(
        parts(&body),
        vec![
            ("前".into(), Part::Body),
            ("A1".into(), table_part(1, 1, 1)),
            ("B1".into(), table_part(1, 1, 2)),
            ("A2a".into(), table_part(1, 2, 1)),
            ("A2b".into(), table_part(1, 2, 1)),
            ("B2".into(), table_part(1, 2, 2)),
            ("後".into(), Part::Body),
            ("X".into(), table_part(2, 1, 1)),
        ]
    );
}

#[test]
fn nested_tables_are_numbered_in_order_of_appearance() {
    let nested = tbl(&[vec![tc(&p("内")), tc(&p("内2"))]]);
    let body = tbl(&[vec![tc(&(p("外") + &nested + &p("外後"))), tc(&p("外2"))]])
        + &tbl(&[vec![tc(&p("次"))]]);
    assert_eq!(
        parts(&body),
        vec![
            ("外".into(), table_part(1, 1, 1)),
            ("内".into(), table_part(2, 1, 1)),
            ("内2".into(), table_part(2, 1, 2)),
            ("外後".into(), table_part(1, 1, 1)),
            ("外2".into(), table_part(1, 1, 2)),
            ("次".into(), table_part(3, 1, 1)),
        ]
    );
}

#[test]
fn textbox_in_table_and_table_in_textbox() {
    let in_box = tbl(&[vec![tc(&p("箱の表"))]]);
    let body = tbl(&[vec![tc(&format!(
        "<w:p>{}{}</w:p>",
        r("セル"),
        dml_textbox(&(in_box + &p("箱")))
    ))]])
        + &tbl(&[vec![tc(&p("次の表"))]]);
    assert_eq!(
        parts(&body),
        vec![
            ("セル".into(), table_part(1, 1, 1)),
            ("箱の表".into(), Part::TextBox),
            ("箱".into(), Part::TextBox),
            // The table inside the text box does not consume a number.
            ("次の表".into(), table_part(2, 1, 1)),
        ]
    );
}

#[test]
fn sdt_wrapped_rows_and_cells_are_counted() {
    let body = format!(
        "<w:tbl><w:tblPr/><w:sdt><w:sdtContent><w:tr>{}<w:sdt><w:sdtContent>{}</w:sdtContent></w:sdt></w:tr></w:sdtContent></w:sdt></w:tbl>",
        tc(&p("A")),
        tc(&p("B"))
    );
    assert_eq!(
        parts(&body),
        vec![
            ("A".into(), table_part(1, 1, 1)),
            ("B".into(), table_part(1, 1, 2)),
        ]
    );
}

// --- namespaces ---------------------------------------------------------------------

#[test]
fn non_w_prefix_is_supported() {
    let doc = format!(
        r#"<?xml version="1.0"?><ns0:document xmlns:ns0="{}"><ns0:body><ns0:p><ns0:r><ns0:t>サー</ns0:t></ns0:r><ns0:r><ns0:tab/><ns0:t>バ</ns0:t></ns0:r></ns0:p></ns0:body></ns0:document>"#,
        common::W_NS
    );
    assert_eq!(
        texts_of(&DocxBuilder::new().document_xml(&doc)),
        vec!["サー\tバ"]
    );
}

#[test]
fn default_namespace_is_supported() {
    let doc = format!(
        r#"<document xmlns="{}"><body><p><r><t>既定</t></r></p></body></document>"#,
        common::W_NS
    );
    assert_eq!(
        texts_of(&DocxBuilder::new().document_xml(&doc)),
        vec!["既定"]
    );
}

#[test]
fn foreign_namespace_with_w_prefix_is_not_wordml() {
    let doc = format!(
        r#"<w:document xmlns:w="{}" xmlns:x="urn:other"><w:body><w:p><x:t>無視</x:t><w:r><w:t>有効</w:t></w:r></w:p></w:body></w:document>"#,
        common::W_NS
    );
    assert_eq!(
        texts_of(&DocxBuilder::new().document_xml(&doc)),
        vec!["有効"]
    );
}

#[test]
fn strict_documents_are_read() {
    let b = DocxBuilder::new()
        .strict()
        .body(r#"<w:p><w:r><w:t>厳格</w:t><w:br w:type="page"/><w:t>形式</w:t></w:r></w:p>"#);
    assert_eq!(texts_of(&b), vec!["厳格\n形式"]);
}

// --- package ----------------------------------------------------------------------------

#[test]
fn main_part_is_found_through_relationships() {
    let b = DocxBuilder::new()
        .main_part("content/main.xml")
        .body(&p("別の場所"));
    assert_eq!(texts_of(&b), vec!["別の場所"]);
}

#[test]
fn main_part_lookup_is_case_insensitive() {
    let doc = format!(
        r#"<w:document xmlns:w="{}"><w:body>{}</w:body></w:document>"#,
        common::W_NS,
        p("大文字")
    );
    let b = DocxBuilder::new()
        .without("word/document.xml")
        .raw("word/Document.xml", doc);
    assert_eq!(texts_of(&b), vec!["大文字"]);
}

#[test]
fn absolute_and_parent_targets_and_external_rels() {
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId0" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="http://example.com/x.xml" TargetMode="External"/><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="/a/../doc/main.xml"/></Relationships>"#;
    let doc = format!(
        r#"<w:document xmlns:w="{}"><w:body>{}</w:body></w:document>"#,
        common::W_NS,
        p("解決")
    );
    let b = DocxBuilder::new()
        .raw("_rels/.rels", rels)
        .raw("doc/main.xml", doc);
    assert_eq!(texts_of(&b), vec!["解決"]);
}

#[test]
fn utf16_part_is_read() {
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-16"?><w:document xmlns:w="{}"><w:body>{}</w:body></w:document>"#,
        common::W_NS,
        p("十六")
    );
    let mut bytes = vec![0xFF, 0xFE];
    for u in doc.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    let b = DocxBuilder::new().raw("word/document.xml", bytes);
    assert_eq!(texts_of(&b), vec!["十六"]);
}

#[test]
fn missing_main_relationship_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = DocxBuilder::new()
        .without("_rels/.rels")
        .write(dir.path(), "t.docx");
    assert!(matches!(
        docgrep::word::extract(&path),
        Err(FileError::MissingMainPart)
    ));
}

#[test]
fn cfb_file_is_reported_as_encrypted_warning() {
    let dir = tempfile::tempdir().unwrap();
    let path = common::write_cfb(dir.path(), "enc.docx");
    let err = docgrep::word::extract(&path).unwrap_err();
    assert!(matches!(err, FileError::EncryptedOrLegacy));
    assert!(err.is_warning());
}

#[test]
fn broken_zip_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = common::write_bytes(dir.path(), "bad.docx", b"PK\x03\x04 not really a zip");
    let err = docgrep::word::extract(&path).unwrap_err();
    assert!(!err.is_warning(), "{err:?}");
}

#[test]
fn empty_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = common::write_bytes(dir.path(), "empty.docx", b"");
    assert!(!docgrep::word::extract(&path).unwrap_err().is_warning());
}

#[test]
fn truncated_xml_keeps_completed_paragraphs() {
    let doc = format!(
        r#"<w:document xmlns:w="{}"><w:body>{}{}<w:p><w:r><w:t>途中"#,
        common::W_NS,
        p("一"),
        p("二")
    );
    let x = extract(&DocxBuilder::new().document_xml(&doc));
    assert!(matches!(x.partial_error, Some(FileError::Xml { .. })));
    let texts: Vec<_> = x.units.iter().map(|u| u.text.as_str()).collect();
    assert_eq!(texts, vec!["一", "二"]);
    // Mismatched tags.
    let doc = format!(
        r#"<w:document xmlns:w="{}"><w:body>{}<w:p></w:r></w:p>{}</w:body></w:document>"#,
        common::W_NS,
        p("一"),
        p("後")
    );
    let x = extract(&DocxBuilder::new().document_xml(&doc));
    assert!(matches!(x.partial_error, Some(FileError::Xml { .. })));
    let texts: Vec<_> = x.units.iter().map(|u| u.text.as_str()).collect();
    assert_eq!(texts, vec!["一"]);
}
