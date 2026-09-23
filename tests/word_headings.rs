//! Headings and chapter numbers (SPEC §6.5).

mod common;

use common::{DocxBuilder, p, r};
use docgrep::error::FileError;
use docgrep::model::Part;

/// (level, number, text) of a heading.
type H = (u8, Option<String>, String);

fn h(level: u8, number: Option<&str>, text: &str) -> Option<H> {
    Some((level, number.map(str::to_string), text.to_string()))
}

/// Unit texts with the heading in effect for each.
fn headings(b: &DocxBuilder) -> Vec<(String, Option<H>)> {
    let dir = tempfile::tempdir().unwrap();
    let path = b.write(dir.path(), "t.docx");
    let x = docgrep::word::extract(&path).unwrap();
    assert!(x.partial_error.is_none(), "{:?}", x.partial_error);
    x.units
        .into_iter()
        .map(|u| {
            let head = u.location.heading.map(|h| (h.level, h.number, h.text));
            (u.text, head)
        })
        .collect()
}

/// Just the heading of each unit.
fn heads_only(b: &DocxBuilder) -> Vec<Option<H>> {
    headings(b).into_iter().map(|(_, h)| h).collect()
}

/// Paragraph with a style.
fn sp(style: &str, text: &str) -> String {
    format!(
        r#"<w:p><w:pPr><w:pStyle w:val="{style}"/></w:pPr>{}</w:p>"#,
        r(text)
    )
}

/// Paragraph with extra pPr content.
fn pp(ppr: &str, text: &str) -> String {
    format!("<w:p><w:pPr>{ppr}</w:pPr>{}</w:p>", r(text))
}

fn num_pr(num_id: u32, ilvl: Option<u8>) -> String {
    let ilvl = ilvl
        .map(|i| format!(r#"<w:ilvl w:val="{i}"/>"#))
        .unwrap_or_default();
    format!(r#"<w:numPr>{ilvl}<w:numId w:val="{num_id}"/></w:numPr>"#)
}

/// Paragraph style. `outline` is the 0-based outline level.
fn style(id: &str, name: &str, outline: Option<u8>, num: Option<(u32, Option<u8>)>) -> String {
    let outline = outline
        .map(|l| format!(r#"<w:outlineLvl w:val="{l}"/>"#))
        .unwrap_or_default();
    let num = num.map(|(n, i)| num_pr(n, i)).unwrap_or_default();
    format!(
        r#"<w:style w:type="paragraph" w:styleId="{id}"><w:name w:val="{name}"/><w:basedOn w:val="a"/><w:pPr>{num}{outline}</w:pPr></w:style>"#
    )
}

/// Japanese-Word-like styles: ids are "1", "2", "3", names are English built-ins.
fn jp_styles(extra: &str) -> String {
    format!(
        r#"<w:style w:type="paragraph" w:default="1" w:styleId="a"><w:name w:val="Normal"/></w:style>{}{}{}{extra}"#,
        style("1", "heading 1", Some(0), Some((1, Some(0)))),
        style("2", "heading 2", Some(1), Some((1, Some(1)))),
        style("3", "heading 3", Some(2), Some((1, Some(2)))),
    )
}

fn lvl(ilvl: u8, fmt: &str, text: &str, extra: &str) -> String {
    format!(
        r#"<w:lvl w:ilvl="{ilvl}"><w:start w:val="1"/><w:numFmt w:val="{fmt}"/><w:lvlText w:val="{text}"/>{extra}</w:lvl>"#
    )
}

fn abstract_num(id: u32, levels: &str) -> String {
    format!(r#"<w:abstractNum w:abstractNumId="{id}">{levels}</w:abstractNum>"#)
}

fn num(num_id: u32, abs: u32, overrides: &str) -> String {
    format!(r#"<w:num w:numId="{num_id}"><w:abstractNumId w:val="{abs}"/>{overrides}</w:num>"#)
}

/// Outline numbering 1 / 1.1 / 1.1.1 on abstractNum 0, numId 1.
fn outline_levels() -> String {
    lvl(0, "decimal", "%1", "")
        + &lvl(1, "decimal", "%1.%2", "")
        + &lvl(2, "decimal", "%1.%2.%3", "")
}

fn std_numbering(extra: &str) -> String {
    abstract_num(0, &outline_levels()) + &num(1, 0, "") + extra
}

fn doc(body: &str) -> DocxBuilder {
    DocxBuilder::new()
        .styles(&jp_styles(""))
        .numbering(&std_numbering(""))
        .body(body)
}

// --- detection and numbering ---------------------------------------------------------

#[test]
fn multi_level_numbers_and_reset_on_upper_level() {
    let body = [
        p("前文"),
        sp("1", "概要"),
        p("本文1"),
        sp("2", "目的"),
        sp("3", "詳細"),
        p("本文2"),
        sp("2", "範囲"),
        sp("1", "構成"),
        sp("2", "全体"),
    ]
    .concat();
    assert_eq!(
        heads_only(&doc(&body)),
        vec![
            None,
            h(1, Some("1"), "概要"),
            h(1, Some("1"), "概要"),
            h(2, Some("1.1"), "目的"),
            h(3, Some("1.1.1"), "詳細"),
            h(3, Some("1.1.1"), "詳細"),
            h(2, Some("1.2"), "範囲"),
            h(1, Some("2"), "構成"),
            h(2, Some("2.1"), "全体"),
        ]
    );
}

#[test]
fn heading_text_is_trimmed_and_number_is_not_in_text() {
    let units = headings(&doc(&sp("1", " 　概要 ")));
    assert_eq!(units[0].0, " 　概要 ");
    assert_eq!(units[0].1, h(1, Some("1"), "概要"));
}

#[test]
fn style_id_is_not_used_for_detection() {
    let styles = jp_styles(&style("Heading1", "Body Text", None, None));
    let b = DocxBuilder::new()
        .styles(&styles)
        .body(&sp("Heading1", "見出しではない"));
    assert_eq!(heads_only(&b), vec![None]);
}

#[test]
fn heading_by_style_name_only() {
    let styles =
        r#"<w:style w:type="paragraph" w:styleId="x"><w:name w:val="Heading 2"/></w:style>"#;
    let b = DocxBuilder::new().styles(styles).body(&sp("x", "名前だけ"));
    assert_eq!(heads_only(&b), vec![h(2, None, "名前だけ")]);
}

#[test]
fn outline_level_inherited_through_based_on() {
    let styles = jp_styles(
        r#"<w:style w:type="paragraph" w:styleId="c"><w:name w:val="章見出し"/><w:basedOn w:val="1"/></w:style>"#,
    );
    let b = DocxBuilder::new()
        .styles(&styles)
        .numbering(&std_numbering(""))
        .body(&sp("c", "独自スタイル"));
    assert_eq!(heads_only(&b), vec![h(1, Some("1"), "独自スタイル")]);
}

#[test]
fn based_on_cycle_terminates() {
    let styles = r#"<w:style w:type="paragraph" w:styleId="x"><w:name w:val="X"/><w:basedOn w:val="y"/></w:style><w:style w:type="paragraph" w:styleId="y"><w:name w:val="Y"/><w:basedOn w:val="x"/></w:style>"#;
    let b = DocxBuilder::new().styles(styles).body(&sp("x", "循環"));
    assert_eq!(heads_only(&b), vec![None]);
}

#[test]
fn direct_outline_level() {
    let b = doc(&(pp(r#"<w:outlineLvl w:val="1"/>"#, "直接") + &p("後")));
    assert_eq!(heads_only(&b), vec![h(2, None, "直接"), h(2, None, "直接")]);
}

#[test]
fn direct_outline_level_9_makes_heading_style_body_text() {
    let body = sp("1", "章")
        + &pp(
            r#"<w:pStyle w:val="1"/><w:outlineLvl w:val="9"/>"#,
            "本文扱い",
        );
    // The second paragraph is still numbered (it has the style's numPr) but is not a heading.
    assert_eq!(
        heads_only(&doc(&body)),
        vec![h(1, Some("1"), "章"), h(1, Some("1"), "章")]
    );
}

#[test]
fn default_paragraph_style_applies_without_pstyle() {
    let styles = r#"<w:style w:type="paragraph" w:default="1" w:styleId="d"><w:name w:val="Normal"/><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:style>"#;
    let b = DocxBuilder::new()
        .styles(styles)
        .body(&(p("既定") + &sp("unknown", "未定義")));
    assert_eq!(
        heads_only(&b),
        vec![h(1, None, "既定"), h(1, None, "未定義")]
    );
}

#[test]
fn toc_styles_are_not_headings() {
    let styles = jp_styles(&style("toc1", "TOC 1", Some(0), None));
    let b = DocxBuilder::new()
        .styles(&styles)
        .numbering(&std_numbering(""))
        .body(&(sp("toc1", "1 概要\t3") + &sp("1", "概要")));
    assert_eq!(heads_only(&b), vec![None, h(1, Some("1"), "概要")]);
}

#[test]
fn empty_heading_advances_counter_but_is_not_current() {
    let body = [sp("1", "一"), sp("1", "　"), p("本文"), sp("1", "三")].concat();
    assert_eq!(
        heads_only(&doc(&body)),
        vec![
            h(1, Some("1"), "一"),
            h(1, Some("1"), "一"),
            h(1, Some("1"), "一"),
            h(1, Some("3"), "三"),
        ]
    );
}

#[test]
fn table_paragraphs_can_be_headings() {
    let body = format!(
        "{}<w:tbl><w:tr><w:tc>{}{}</w:tc></w:tr></w:tbl>{}",
        sp("1", "章"),
        sp("2", "表内の節"),
        p("セル"),
        p("後")
    );
    assert_eq!(
        heads_only(&doc(&body)),
        vec![
            h(1, Some("1"), "章"),
            h(2, Some("1.1"), "表内の節"),
            h(2, Some("1.1"), "表内の節"),
            h(2, Some("1.1"), "表内の節"),
        ]
    );
}

#[test]
fn textbox_takes_anchor_heading_and_is_never_a_heading() {
    let textbox = format!(
        r#"<w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wps:txbx><w:txbxContent>{}</w:txbxContent></wps:txbx></w:drawing></mc:Choice><mc:Fallback/></mc:AlternateContent></w:r>"#,
        sp("1", "箱の中の見出し風")
    );
    let body = [
        sp("1", "章"),
        format!("<w:p>{}{textbox}</w:p>", r("アンカー")),
        sp("1", "次の章"),
    ]
    .concat();
    let units = headings(&doc(&body));
    assert_eq!(
        units,
        vec![
            ("章".into(), h(1, Some("1"), "章")),
            ("アンカー".into(), h(1, Some("1"), "章")),
            ("箱の中の見出し風".into(), h(1, Some("1"), "章")),
            // The text box heading did not advance the counter.
            ("次の章".into(), h(1, Some("2"), "次の章")),
        ]
    );
}

#[test]
fn anchor_heading_paragraph_gives_its_heading_to_text_box() {
    let textbox = format!(
        r#"<w:r><w:pict><v:textbox><w:txbxContent>{}</w:txbxContent></v:textbox></w:pict></w:r>"#,
        p("箱")
    );
    let body = format!(
        r#"<w:p><w:pPr><w:pStyle w:val="1"/></w:pPr>{}{textbox}</w:p>"#,
        r("章")
    );
    assert_eq!(
        heads_only(&doc(&body)),
        vec![h(1, Some("1"), "章"), h(1, Some("1"), "章")]
    );
}

#[test]
fn ppr_change_does_not_override_current_properties() {
    let body = pp(
        r#"<w:pStyle w:val="1"/><w:pPrChange w:id="1" w:author="a"><w:pPr><w:pStyle w:val="3"/><w:outlineLvl w:val="5"/></w:pPr></w:pPrChange>"#,
        "変更後",
    );
    assert_eq!(heads_only(&doc(&body)), vec![h(1, Some("1"), "変更後")]);
}

// --- numPr resolution --------------------------------------------------------------------

#[test]
fn direct_num_pr_overrides_style() {
    let body = sp("1", "章")
        + &pp(
            &format!(r#"<w:pStyle w:val="1"/>{}"#, num_pr(1, Some(1))),
            "節扱い",
        );
    assert_eq!(
        heads_only(&doc(&body)),
        vec![h(1, Some("1"), "章"), h(1, Some("1.1"), "節扱い")]
    );
}

#[test]
fn num_id_zero_removes_style_numbering() {
    let body = sp("1", "章")
        + &pp(
            &format!(r#"<w:pStyle w:val="1"/>{}"#, num_pr(0, None)),
            "番号なし",
        )
        + &sp("1", "次");
    assert_eq!(
        heads_only(&doc(&body)),
        vec![
            h(1, Some("1"), "章"),
            h(1, None, "番号なし"),
            h(1, Some("2"), "次"),
        ]
    );
}

#[test]
fn num_id_and_ilvl_fall_back_independently() {
    // ilvl from the paragraph, numId from the style.
    let body = sp("1", "章")
        + &pp(
            r#"<w:pStyle w:val="1"/><w:numPr><w:ilvl w:val="1"/></w:numPr>"#,
            "節",
        );
    assert_eq!(
        heads_only(&doc(&body)),
        vec![h(1, Some("1"), "章"), h(1, Some("1.1"), "節")]
    );
}

#[test]
fn ilvl_from_level_pstyle() {
    let styles = format!(
        r#"<w:style w:type="paragraph" w:default="1" w:styleId="a"><w:name w:val="Normal"/></w:style>{}{}"#,
        style("1", "heading 1", Some(0), Some((1, None))),
        style("2", "heading 2", Some(1), Some((1, None))),
    );
    let levels = lvl(0, "decimal", "%1", r#"<w:pStyle w:val="1"/>"#)
        + &lvl(1, "decimal", "%1-%2", r#"<w:pStyle w:val="2"/>"#);
    let b = DocxBuilder::new()
        .styles(&styles)
        .numbering(&(abstract_num(0, &levels) + &num(1, 0, "")))
        .body(&(sp("1", "章") + &sp("2", "節")));
    assert_eq!(
        heads_only(&b),
        vec![h(1, Some("1"), "章"), h(2, Some("1-1"), "節")]
    );
}

#[test]
fn numbered_body_paragraphs_advance_counters() {
    let body = [
        sp("1", "章"),
        pp(&num_pr(1, Some(0)), "番号付き本文"),
        sp("1", "次の章"),
    ]
    .concat();
    assert_eq!(
        heads_only(&doc(&body)),
        vec![
            h(1, Some("1"), "章"),
            h(1, Some("1"), "章"),
            h(1, Some("3"), "次の章"),
        ]
    );
}

#[test]
fn deeper_level_first_shows_upper_start_value() {
    assert_eq!(
        heads_only(&doc(&(sp("2", "いきなり節") + &sp("1", "章")))),
        vec![h(2, Some("1.1"), "いきなり節"), h(1, Some("1"), "章")]
    );
}

#[test]
fn missing_numbering_part_gives_headings_without_numbers() {
    let b = DocxBuilder::new()
        .styles(&jp_styles(""))
        .body(&sp("1", "章"));
    assert_eq!(heads_only(&b), vec![h(1, None, "章")]);
}

#[test]
fn unknown_num_id_gives_no_number() {
    let body = pp(
        &format!(r#"<w:pStyle w:val="1"/>{}"#, num_pr(99, Some(0))),
        "章",
    );
    assert_eq!(heads_only(&doc(&body)), vec![h(1, None, "章")]);
}

// --- counters -------------------------------------------------------------------------------

#[test]
fn nums_sharing_an_abstract_continue_numbering() {
    let numbering = std_numbering(&num(3, 0, ""));
    let body = sp("1", "一")
        + &pp(
            &format!(r#"<w:pStyle w:val="1"/>{}"#, num_pr(3, Some(0))),
            "二",
        );
    let b = DocxBuilder::new()
        .styles(&jp_styles(""))
        .numbering(&numbering)
        .body(&body);
    assert_eq!(
        heads_only(&b),
        vec![h(1, Some("1"), "一"), h(1, Some("2"), "二")]
    );
}

#[test]
fn start_override_restarts_on_first_use_of_num() {
    let numbering = std_numbering(&num(
        2,
        0,
        r#"<w:lvlOverride w:ilvl="0"><w:startOverride w:val="5"/></w:lvlOverride>"#,
    ));
    let restart = |t: &str| {
        pp(
            &format!(r#"<w:pStyle w:val="1"/>{}"#, num_pr(2, Some(0))),
            t,
        )
    };
    let body = [
        sp("1", "一"),
        sp("1", "二"),
        restart("五"),
        restart("六"),
        sp("1", "七"),
    ]
    .concat();
    let b = DocxBuilder::new()
        .styles(&jp_styles(""))
        .numbering(&numbering)
        .body(&body);
    assert_eq!(
        heads_only(&b),
        vec![
            h(1, Some("1"), "一"),
            h(1, Some("2"), "二"),
            h(1, Some("5"), "五"),
            h(1, Some("6"), "六"),
            h(1, Some("7"), "七"),
        ]
    );
}

#[test]
fn start_value_is_used_for_the_first_paragraph() {
    let levels = r#"<w:lvl w:ilvl="0"><w:start w:val="3"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1"/></w:lvl>"#;
    let b = DocxBuilder::new()
        .styles(&jp_styles(""))
        .numbering(&(abstract_num(0, levels) + &num(1, 0, "")))
        .body(&(sp("1", "三") + &sp("1", "四")));
    assert_eq!(
        heads_only(&b),
        vec![h(1, Some("3"), "三"), h(1, Some("4"), "四")]
    );
}

#[test]
fn missing_start_means_zero() {
    let levels = r#"<w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/><w:lvlText w:val="%1"/></w:lvl>"#;
    let b = DocxBuilder::new()
        .styles(&jp_styles(""))
        .numbering(&(abstract_num(0, levels) + &num(1, 0, "")))
        .body(&sp("1", "零"));
    assert_eq!(heads_only(&b), vec![h(1, Some("0"), "零")]);
}

#[test]
fn lvl_restart_zero_never_resets() {
    let levels =
        lvl(0, "decimal", "%1", "") + &lvl(1, "decimal", "%1.%2", r#"<w:lvlRestart w:val="0"/>"#);
    let b = DocxBuilder::new()
        .styles(&jp_styles(""))
        .numbering(&(abstract_num(0, &levels) + &num(1, 0, "")))
        .body(&[sp("1", "a"), sp("2", "b"), sp("1", "c"), sp("2", "d")].concat());
    assert_eq!(
        heads_only(&b),
        vec![
            h(1, Some("1"), "a"),
            h(2, Some("1.1"), "b"),
            h(1, Some("2"), "c"),
            h(2, Some("2.2"), "d"),
        ]
    );
}

#[test]
fn lvl_restart_k_resets_only_after_levels_up_to_k() {
    // Level 3 restarts only after a level-1 paragraph, not after level 2.
    let levels = lvl(0, "decimal", "%1", "")
        + &lvl(1, "decimal", "%1.%2", "")
        + &lvl(2, "decimal", "%1.%2.%3", r#"<w:lvlRestart w:val="1"/>"#);
    let b = DocxBuilder::new()
        .styles(&jp_styles(""))
        .numbering(&(abstract_num(0, &levels) + &num(1, 0, "")))
        .body(
            &[
                sp("1", "a"),
                sp("2", "b"),
                sp("3", "c"),
                sp("2", "d"),
                sp("3", "e"),
                sp("1", "f"),
                sp("2", "g"),
                sp("3", "h"),
            ]
            .concat(),
        );
    let numbers: Vec<_> = heads_only(&b)
        .into_iter()
        .map(|h| h.and_then(|h| h.1).unwrap_or_default())
        .collect();
    assert_eq!(
        numbers,
        vec!["1", "1.1", "1.1.1", "1.2", "1.2.2", "2", "2.1", "2.1.1"]
    );
}

#[test]
fn is_lgl_forces_decimal() {
    let levels = lvl(0, "upperRoman", "第%1章", "")
        + &lvl(1, "decimal", "%1.%2", r#"<w:isLgl/>"#)
        + &lvl(2, "decimal", "%1-%2-%3", r#"<w:isLgl w:val="0"/>"#);
    let b = DocxBuilder::new()
        .styles(&jp_styles(""))
        .numbering(&(abstract_num(0, &levels) + &num(1, 0, "")))
        .body(&[sp("1", "a"), sp("2", "b"), sp("3", "c")].concat());
    let numbers: Vec<_> = heads_only(&b)
        .into_iter()
        .map(|h| h.and_then(|h| h.1).unwrap_or_default())
        .collect();
    assert_eq!(numbers, vec!["第I章", "1.1", "I-1-1"]);
}

#[test]
fn lvl_override_replaces_level_definition() {
    let numbering = std_numbering(&num(
        7,
        0,
        r#"<w:lvlOverride w:ilvl="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="(%1)"/></w:lvl></w:lvlOverride>"#,
    ));
    let body = pp(
        &format!(r#"<w:pStyle w:val="1"/>{}"#, num_pr(7, Some(0))),
        "上書き",
    );
    let b = DocxBuilder::new()
        .styles(&jp_styles(""))
        .numbering(&numbering)
        .body(&body);
    assert_eq!(heads_only(&b), vec![h(1, Some("(a)"), "上書き")]);
}

#[test]
fn num_style_link_is_followed() {
    let styles = jp_styles(&format!(
        r#"<w:style w:type="numbering" w:styleId="ListStyle"><w:name w:val="章番号リスト"/><w:pPr>{}</w:pPr></w:style>"#,
        num_pr(5, None)
    ));
    let real = lvl(0, "decimal", "第%1章", "");
    let numbering = abstract_num(10, r#"<w:numStyleLink w:val="ListStyle"/>"#)
        + &abstract_num(11, &real)
        + &num(5, 11, "")
        + &num(1, 10, "");
    let b = DocxBuilder::new()
        .styles(&styles)
        .numbering(&numbering)
        .body(&(sp("1", "一") + &sp("1", "二")));
    assert_eq!(
        heads_only(&b),
        vec![h(1, Some("第1章"), "一"), h(1, Some("第2章"), "二")]
    );
}

// --- formats ----------------------------------------------------------------------------------

fn single_level(fmt_xml: &str, text: &str) -> DocxBuilder {
    let levels = format!(
        r#"<w:lvl w:ilvl="0"><w:start w:val="1"/>{fmt_xml}<w:lvlText w:val="{text}"/></w:lvl>"#
    );
    DocxBuilder::new()
        .styles(&jp_styles(""))
        .numbering(&(abstract_num(0, &levels) + &num(1, 0, "")))
        .body(&(sp("1", "a") + &sp("1", "b")))
}

fn numbers(b: &DocxBuilder) -> Vec<Option<String>> {
    heads_only(b)
        .into_iter()
        .map(|h| h.and_then(|h| h.1))
        .collect()
}

#[test]
fn japanese_formats_in_level_text() {
    assert_eq!(
        numbers(&single_level(
            r#"<w:numFmt w:val="japaneseCounting"/>"#,
            "第%1章"
        )),
        vec![Some("第一章".into()), Some("第二章".into())]
    );
    assert_eq!(
        numbers(&single_level(
            r#"<w:numFmt w:val="decimalFullWidth"/>"#,
            "%1．"
        )),
        vec![Some("１．".into()), Some("２．".into())]
    );
    assert_eq!(
        numbers(&single_level(
            r#"<w:numFmt w:val="decimalEnclosedCircle"/>"#,
            "%1"
        )),
        vec![Some("①".into()), Some("②".into())]
    );
}

#[test]
fn bullet_and_none_levels_have_no_number() {
    assert_eq!(
        numbers(&single_level(r#"<w:numFmt w:val="bullet"/>"#, "・")),
        vec![None, None]
    );
    assert_eq!(
        numbers(&single_level(r#"<w:numFmt w:val="none"/>"#, "")),
        vec![None, None]
    );
}

#[test]
fn custom_format_in_alternate_content() {
    let ac = |choice: &str, fallback: &str| {
        format!(
            r#"<mc:AlternateContent><mc:Choice Requires="w14"><w:numFmt w:val="custom" w:format="{choice}"/></mc:Choice><mc:Fallback><w:numFmt w:val="{fallback}"/></mc:Fallback></mc:AlternateContent>"#
        )
    };
    assert_eq!(
        numbers(&single_level(&ac("001, 002, 003, ...", "decimal"), "%1")),
        vec![Some("001".into()), Some("002".into())]
    );
    assert_eq!(
        numbers(&single_level(
            &ac("一, 二, 三, ...", "decimalFullWidth"),
            "%1"
        )),
        vec![Some("１".into()), Some("２".into())]
    );
}

// --- namespaces and errors ----------------------------------------------------------------------

#[test]
fn strict_styles_and_numbering() {
    let b = DocxBuilder::new()
        .strict()
        .styles(&jp_styles(""))
        .numbering(&std_numbering(""))
        .body(&(sp("1", "章") + &sp("2", "節")));
    assert_eq!(
        heads_only(&b),
        vec![h(1, Some("1"), "章"), h(2, Some("1.1"), "節")]
    );
}

#[test]
fn other_prefix_in_styles_part() {
    let styles = format!(
        r#"<s:styles xmlns:s="{}"><s:style s:type="paragraph" s:styleId="1"><s:name s:val="heading 1"/></s:style></s:styles>"#,
        common::W_NS
    );
    let b = DocxBuilder::new()
        .styles("")
        .raw("word/styles.xml", styles)
        .body(&sp("1", "章"));
    assert_eq!(heads_only(&b), vec![h(1, None, "章")]);
}

#[test]
fn broken_styles_part_is_reported_but_body_is_searched() {
    let dir = tempfile::tempdir().unwrap();
    let path = DocxBuilder::new()
        .styles("")
        .raw("word/styles.xml", "<w:styles xmlns:w=\"x\"><broken")
        .body(&p("本文"))
        .write(dir.path(), "t.docx");
    let x = docgrep::word::extract(&path).unwrap();
    assert!(matches!(
        x.partial_error,
        Some(FileError::Xml { ref part, .. }) if part == "word/styles.xml"
    ));
    assert_eq!(x.units.len(), 1);
    assert_eq!(x.units[0].location.part, Part::Body);
}

#[test]
fn empty_ppr_does_not_leak_into_later_skipped_elements() {
    // After an empty <w:pPr/>, a skipped subtree (mc:Fallback) must not be read as pPr.
    let body = r#"<w:p><w:pPr/><w:r><mc:AlternateContent><mc:Choice Requires="w14"><w:t>本文</w:t></mc:Choice><mc:Fallback><w:pStyle w:val="1"/><w:outlineLvl w:val="0"/></mc:Fallback></mc:AlternateContent></w:r></w:p>"#;
    assert_eq!(heads_only(&doc(body)), vec![None]);
}
