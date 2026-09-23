//! Heading detection and chapter numbers along the body stream (SPEC §6.5).

use super::numbering::{Counters, LEVELS, Numbering};
use super::styles::Styles;
use crate::model::Heading;

/// Paragraph properties read from a paragraph's own `w:pPr`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParaProps {
    pub style: Option<String>,
    pub outline_lvl: Option<u8>,
    pub num_id: Option<u32>,
    pub ilvl: Option<u8>,
    pub page_break_before: Option<bool>,
    /// The paragraph ends a section that starts the next one on a new page.
    pub section_break: bool,
}

/// Styles and list definitions of a document.
#[derive(Debug, Clone, Default)]
pub struct DocContext {
    pub styles: Styles,
    pub numbering: Numbering,
}

/// Walks body-stream paragraphs in order, advancing list counters and remembering the
/// latest heading.
#[derive(Debug, Default)]
pub struct HeadingTracker {
    counters: Counters,
    current: Option<Heading>,
}

impl HeadingTracker {
    /// Processes one body-stream paragraph (tables included, text boxes excluded) and
    /// returns the heading in effect for it: the paragraph itself if it is a heading,
    /// otherwise the latest heading before it.
    pub fn paragraph(
        &mut self,
        ctx: &DocContext,
        props: &ParaProps,
        text: &str,
    ) -> Option<Heading> {
        let styles = &ctx.styles;
        let style_id = styles.effective_id(props.style.as_deref());
        let number = self.advance_numbering(ctx, props, style_id);

        let level = props
            .outline_lvl
            .or_else(|| styles.outline_level(style_id))
            .filter(|l| usize::from(*l) < LEVELS);
        if let Some(level) = level
            && !styles.is_toc(style_id)
        {
            let text = text.trim();
            // Empty headings advance counters but do not become the current heading.
            if !text.is_empty() {
                self.current = Some(Heading {
                    level: level + 1,
                    number,
                    text: text.to_string(),
                });
            }
        }
        self.current.clone()
    }

    /// Resolves numId/ilvl (paragraph first, then style chain, each independently) and
    /// advances the counters. Returns the formatted number, if any.
    fn advance_numbering(
        &mut self,
        ctx: &DocContext,
        props: &ParaProps,
        style_id: Option<&str>,
    ) -> Option<String> {
        let styles = &ctx.styles;
        let num_id = props.num_id.or_else(|| styles.num_id(style_id))?;
        if num_id == 0 {
            return None;
        }
        let ilvl = match props.ilvl.or_else(|| styles.ilvl(style_id)) {
            Some(i) => usize::from(i),
            None => ctx
                .numbering
                .resolve_abstract(num_id, styles)
                .zip(style_id)
                .and_then(|(abs, sid)| ctx.numbering.level_for_style(abs, sid))
                .unwrap_or(0),
        };
        if ilvl >= LEVELS {
            return None;
        }
        self.counters.advance(&ctx.numbering, styles, num_id, ilvl)
    }
}
