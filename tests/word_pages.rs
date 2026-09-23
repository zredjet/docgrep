//! Page estimation (SPEC §6.6).

mod common;

use common::{DocxBuilder, p, r};
use docgrep::model::{PageMode, Part, TextUnit};

const LR: &str = "<w:r><w:lastRenderedPageBreak/></w:r>";
const BR_PAGE: &str = r#"<w:r><w:br w:type="page"/></w:r>"#;

fn extract(b: &DocxBuilder) -> (Option<PageMode>, Vec<TextUnit>) {
    let dir = tempfile::tempdir().unwrap();
    let path = b.write(dir.path(), "t.docx");
    let x = docgrep::word::extract(&path).unwrap();
    assert!(x.partial_error.is_none(), "{:?}", x.partial_error);
    (x.page_mode, x.units)
}

/// Page of the first occurrence of `needle` in each unit that contains it.
fn page_of(units: &[TextUnit], needle: &str) -> Option<u32> {
    units.iter().find_map(|u| {
        let byte = u.text.find(needle)?;
        let pos = u.text.get(..byte)?.chars().count();
        u.page_at(pos)
    })
}

fn pages(body: &str, needles: &[&str]) -> (Option<PageMode>, Vec<Option<u32>>) {
    let (mode, units) = extract(&DocxBuilder::new().body(body));
    (mode, needles.iter().map(|n| page_of(&units, n)).collect())
}

fn with_styles(styles: &str, body: &str, needles: &[&str]) -> Vec<Option<u32>> {
    let (_, units) = extract(&DocxBuilder::new().styles(styles).body(body));
    needles.iter().map(|n| page_of(&units, n)).collect()
}

fn para(inner: &str) -> String {
    format!("<w:p>{inner}</w:p>")
}

fn table(rows: &[Vec<String>]) -> String {
    let rows: String = rows
        .iter()
        .map(|cells| {
            let cells: String = cells.iter().map(|c| format!("<w:tc>{c}</w:tc>")).collect();
            format!("<w:tr>{cells}</w:tr>")
        })
        .collect();
    format!("<w:tbl>{rows}</w:tbl>")
}

fn textbox(inner: &str) -> String {
    format!(
        r#"<w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wps:txbx><w:txbxContent>{inner}</w:txbxContent></wps:txbx></w:drawing></mc:Choice><mc:Fallback><w:pict><v:textbox><w:txbxContent>{inner}</w:txbxContent></v:textbox></w:pict></mc:Fallback></mc:AlternateContent></w:r>"#
    )
}

// --- mode --------------------------------------------------------------------------------

#[test]
fn no_breaks_is_explicit_mode_page_one() {
    let (mode, units) = extract(&DocxBuilder::new().body(&(p("一") + &p("二"))));
    assert_eq!(mode, Some(PageMode::Explicit));
    for u in &units {
        assert_eq!(u.location.page_at_start, Some(1));
        assert_eq!(u.location.page_mode, Some(PageMode::Explicit));
    }
}

#[test]
fn any_rendered_break_selects_rendered_mode() {
    let (mode, units) =
        extract(&DocxBuilder::new().body(&(p("一") + &para(&(LR.to_string() + &r("二"))))));
    assert_eq!(mode, Some(PageMode::Rendered));
    assert!(
        units
            .iter()
            .all(|u| u.location.page_mode == Some(PageMode::Rendered))
    );
}

// --- rendered ------------------------------------------------------------------------------

#[test]
fn rendered_breaks_between_and_inside_paragraphs() {
    let body = [
        p("一頁"),
        para(&(LR.to_string() + &r("二頁"))),
        para(&(r("まだ二頁") + LR + &r("三頁"))),
        p("三頁の続き"),
    ]
    .concat();
    let (_, pages) = pages(&body, &["一頁", "二頁", "まだ二頁", "三頁", "三頁の続き"]);
    assert_eq!(pages, [1, 2, 2, 3, 3].map(Some));
}

#[test]
fn page_break_position_is_inclusive_of_match_start() {
    let body = para(&(r("前") + LR + &r("後")));
    let (_, units) = extract(&DocxBuilder::new().body(&body));
    let u = &units[0];
    assert_eq!(u.page_breaks, vec![1]);
    assert_eq!(u.page_at(0), Some(1));
    assert_eq!(u.page_at(1), Some(2));
}

#[test]
fn rendered_mode_ignores_explicit_breaks() {
    // V1: Word also writes a rendered break after a hard page break.
    let body = [
        para(&(r("一") + BR_PAGE)),
        para(&(LR.to_string() + &r("二"))),
        para(&(r("まだ二") + BR_PAGE)),
        p("三になるはずだが印なし"),
    ]
    .concat();
    let (mode, pages) = pages(&body, &["一", "二", "まだ二", "三になる"]);
    assert_eq!(mode, Some(PageMode::Rendered));
    assert_eq!(pages, [1, 2, 2, 2].map(Some));
}

#[test]
fn rendered_breaks_in_deletions_and_field_codes_count() {
    let body = [
        para(&(r("一") + r#"<w:del w:id="1" w:author="a"><w:r><w:lastRenderedPageBreak/><w:delText>消</w:delText></w:r></w:del>"# + &r("二"))),
        para(
            r#"<w:r><w:fldChar w:fldCharType="begin"/><w:lastRenderedPageBreak/><w:instrText>PAGE</w:instrText><w:fldChar w:fldCharType="separate"/><w:t>三</w:t><w:fldChar w:fldCharType="end"/></w:r>"#,
        ),
    ]
    .concat();
    let (mode, pages) = pages(&body, &["一", "二", "三"]);
    assert_eq!(mode, Some(PageMode::Rendered));
    assert_eq!(pages, [1, 2, 3].map(Some));
}

#[test]
fn rendered_breaks_in_fallback_and_textboxes_do_not_count() {
    let body = [
        para(&(r("一") + &textbox(&para(&(LR.to_string() + &r("箱")))))),
        para(r#"<w:r><mc:AlternateContent><mc:Choice Requires="w14"><w:t>選</w:t></mc:Choice><mc:Fallback><w:lastRenderedPageBreak/></mc:Fallback></mc:AlternateContent></w:r>"#),
        p("後"),
    ]
    .concat();
    let (mode, pages) = pages(&body, &["一", "箱", "選", "後"]);
    assert_eq!(mode, Some(PageMode::Explicit));
    assert_eq!(pages, [1, 1, 1, 1].map(Some));
}

#[test]
fn table_row_split_across_pages_counts_once() {
    // Word repeats the rendered break in every cell of the row that moved (V5).
    let body = [
        p("前"),
        table(&[
            vec![p("A1"), p("B1")],
            vec![
                para(&(r("A2前") + LR + &r("A2後"))),
                para(&(LR.to_string() + &r("B2"))),
            ],
            vec![p("A3"), p("B3")],
        ]),
        p("後"),
    ]
    .concat();
    let (_, pages) = pages(
        &body,
        &["前", "A1", "B1", "A2前", "A2後", "B2", "A3", "B3", "後"],
    );
    assert_eq!(pages, [1, 1, 1, 1, 2, 2, 2, 2, 2].map(Some));
}

#[test]
fn row_ends_on_the_furthest_cell() {
    let body = [
        table(&[vec![
            para(&(LR.to_string() + &r("A") + LR + &r("A続き"))),
            para(&(LR.to_string() + &r("B"))),
        ]]),
        p("後"),
    ]
    .concat();
    let (_, pages) = pages(&body, &["A", "A続き", "B", "後"]);
    assert_eq!(pages, [2, 3, 2, 3].map(Some));
}

#[test]
fn nested_table_rows() {
    let inner = table(&[vec![
        para(&(LR.to_string() + &r("内1"))),
        para(&(LR.to_string() + &r("内2"))),
    ]]);
    let body = [
        table(&[vec![format!("{}{inner}{}", p("外前"), p("外後")), p("隣")]]),
        p("後"),
    ]
    .concat();
    let (_, pages) = pages(&body, &["外前", "内1", "内2", "外後", "隣", "後"]);
    assert_eq!(pages, [1, 2, 2, 2, 1, 2].map(Some));
}

#[test]
fn textbox_uses_page_at_anchor_position() {
    let body = [
        para(&(r("一") + &textbox(&p("箱1")) + LR + &r("二") + &textbox(&(p("箱2") + &p("箱3"))))),
        p("後"),
    ]
    .concat();
    let (_, units) = extract(&DocxBuilder::new().body(&body));
    let pages: Vec<_> = ["箱1", "箱2", "箱3", "後"]
        .iter()
        .map(|n| page_of(&units, n))
        .collect();
    assert_eq!(pages, [1, 2, 2, 2].map(Some));
    let tb = units.iter().find(|u| u.text == "箱2").unwrap();
    assert_eq!(tb.location.part, Part::TextBox);
    assert_eq!(tb.location.page_mode, Some(PageMode::Rendered));
    assert!(tb.page_breaks.is_empty());
}

// --- explicit ------------------------------------------------------------------------------

#[test]
fn explicit_page_break_inside_paragraph() {
    let body = [para(&(r("前") + BR_PAGE + &r("後"))), p("次")].concat();
    let (mode, pages) = pages(&body, &["前", "後", "次"]);
    assert_eq!(mode, Some(PageMode::Explicit));
    assert_eq!(pages, [1, 2, 2].map(Some));
}

#[test]
fn consecutive_hard_breaks_make_blank_pages() {
    let body = [p("一"), para(BR_PAGE), para(BR_PAGE), p("三")].concat();
    let (_, pages) = pages(&body, &["一", "三"]);
    assert_eq!(pages, [1, 3].map(Some));
}

#[test]
fn deleted_and_field_code_hard_breaks_do_not_count() {
    let body = [
        para(&(r("一") + r#"<w:del w:id="1" w:author="a"><w:r><w:br w:type="page"/></w:r></w:del>"#)),
        para(r#"<w:r><w:fldChar w:fldCharType="begin"/><w:br w:type="page"/><w:fldChar w:fldCharType="separate"/><w:t>二</w:t><w:fldChar w:fldCharType="end"/></w:r>"#),
        para(r#"<w:r><mc:AlternateContent><mc:Choice Requires="w14"><w:t>三</w:t></mc:Choice><mc:Fallback><w:br w:type="page"/></mc:Fallback></mc:AlternateContent></w:r>"#),
        p("後"),
    ]
    .concat();
    let (_, pages) = pages(&body, &["一", "二", "三", "後"]);
    assert_eq!(pages, [1, 1, 1, 1].map(Some));
}

#[test]
fn page_break_before_direct_and_from_style() {
    let styles = r#"<w:style w:type="paragraph" w:styleId="1"><w:name w:val="heading 1"/><w:pPr><w:pageBreakBefore/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="c"><w:name w:val="child"/><w:basedOn w:val="1"/></w:style><w:style w:type="paragraph" w:styleId="off"><w:name w:val="off"/><w:basedOn w:val="1"/><w:pPr><w:pageBreakBefore w:val="0"/></w:pPr></w:style>"#;
    let sp = |style: &str, text: &str| {
        format!(
            r#"<w:p><w:pPr><w:pStyle w:val="{style}"/></w:pPr>{}</w:p>"#,
            r(text)
        )
    };
    let body = [
        sp("1", "表紙"), // first paragraph: no blank page before it
        p("本文1"),
        sp("1", "章2"),
        p("本文2"),
        format!(r#"<w:p><w:pPr><w:pageBreakBefore/></w:pPr>{}</w:p>"#, r("直接")),
        sp("c", "継承"),
        sp("off", "打ち消し"),
        format!(r#"<w:p><w:pPr><w:pStyle w:val="1"/><w:pageBreakBefore w:val="false"/></w:pPr>{}</w:p>"#, r("段落で解除")),
    ]
    .concat();
    assert_eq!(
        with_styles(
            styles,
            &body,
            &[
                "表紙",
                "本文1",
                "章2",
                "直接",
                "継承",
                "打ち消し",
                "段落で解除"
            ]
        ),
        [1, 1, 2, 3, 4, 4, 4].map(Some)
    );
}

#[test]
fn page_break_before_is_ignored_in_tables() {
    let body = [
        p("前"),
        table(&[vec![format!(
            r#"<w:p><w:pPr><w:pageBreakBefore/></w:pPr>{}</w:p>"#,
            r("セル")
        )]]),
    ]
    .concat();
    let (_, pages) = pages(&body, &["前", "セル"]);
    assert_eq!(pages, [1, 1].map(Some));
}

#[test]
fn page_break_before_right_after_hard_break_is_absorbed() {
    let body = [
        p("一"),
        para(BR_PAGE),
        format!(
            r#"<w:p><w:pPr><w:pageBreakBefore/></w:pPr>{}</w:p>"#,
            r("二")
        ),
    ]
    .concat();
    let (_, pages) = pages(&body, &["一", "二"]);
    assert_eq!(pages, [1, 2].map(Some));
}

#[test]
fn empty_paragraph_after_break_holds_a_line() {
    let body = [
        p("一"),
        para(BR_PAGE),
        "<w:p/>".to_string(),
        format!(
            r#"<w:p><w:pPr><w:pageBreakBefore/></w:pPr>{}</w:p>"#,
            r("三")
        ),
    ]
    .concat();
    let (_, pages) = pages(&body, &["一", "三"]);
    assert_eq!(pages, [1, 3].map(Some));
}

fn sect(kind: Option<&str>) -> String {
    let kind = kind
        .map(|k| format!(r#"<w:type w:val="{k}"/>"#))
        .unwrap_or_default();
    format!(r#"<w:sectPr>{kind}<w:pgSz w:w="11906" w:h="16838"/></w:sectPr>"#)
}

#[test]
fn section_breaks_start_the_next_paragraph_on_a_new_page() {
    let body = [
        format!("<w:p><w:pPr>{}</w:pPr>{}</w:p>", sect(None), r("一")),
        format!(
            "<w:p><w:pPr>{}</w:pPr>{}</w:p>",
            sect(Some("nextPage")),
            r("二")
        ),
        format!(
            "<w:p><w:pPr>{}</w:pPr>{}</w:p>",
            sect(Some("continuous")),
            r("三")
        ),
        format!(
            "<w:p><w:pPr>{}</w:pPr>{}</w:p>",
            sect(Some("oddPage")),
            r("三続き")
        ),
        format!(
            "<w:p><w:pPr>{}</w:pPr>{}</w:p>",
            sect(Some("nextColumn")),
            r("四")
        ),
        format!(
            "<w:p><w:pPr>{}</w:pPr>{}</w:p>",
            sect(Some("evenPage")),
            r("四続き")
        ),
        p("五"),
        // The body-level sectPr of the last section is not a break.
        sect(None),
    ]
    .concat();
    let (_, pages) = pages(&body, &["一", "二", "三", "三続き", "四", "四続き", "五"]);
    assert_eq!(pages, [1, 2, 3, 3, 4, 4, 5].map(Some));
}

#[test]
fn section_break_followed_by_page_break_before_counts_once() {
    let body = [
        format!("<w:p><w:pPr>{}</w:pPr>{}</w:p>", sect(None), r("一")),
        format!(
            r#"<w:p><w:pPr><w:pageBreakBefore/></w:pPr>{}</w:p>"#,
            r("二")
        ),
    ]
    .concat();
    let (_, pages) = pages(&body, &["一", "二"]);
    assert_eq!(pages, [1, 2].map(Some));
}

#[test]
fn explicit_breaks_in_table_cells() {
    let body = [
        table(&[vec![para(&(r("A") + BR_PAGE + &r("A続き"))), p("B")]]),
        p("後"),
    ]
    .concat();
    let (_, pages) = pages(&body, &["A", "A続き", "B", "後"]);
    assert_eq!(pages, [1, 2, 1, 2].map(Some));
}

#[test]
fn textbox_page_in_explicit_mode() {
    let body = para(&(r("前") + BR_PAGE + &textbox(&p("箱"))));
    let (mode, pages) = pages(&body, &["前", "箱"]);
    assert_eq!(mode, Some(PageMode::Explicit));
    assert_eq!(pages, [1, 2].map(Some));
}

#[test]
fn page_break_before_in_textbox_is_ignored() {
    let body = [
        para(
            &(r("一")
                + &textbox(&format!(
                    r#"<w:p><w:pPr><w:pageBreakBefore/></w:pPr>{}</w:p>"#,
                    r("箱")
                ))),
        ),
        p("後"),
    ]
    .concat();
    let (_, pages) = pages(&body, &["一", "箱", "後"]);
    assert_eq!(pages, [1, 1, 1].map(Some));
}
