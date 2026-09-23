//! Exact matching of a pattern against unit text. Returns char offsets.

use fancy_regex::{Regex, RegexBuilder};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PatternError {
    #[error("検索パターンが空です")]
    Empty,
    #[error("正規表現が不正です: {0}")]
    Invalid(String),
}

#[derive(Debug)]
pub struct Matcher {
    re: Regex,
}

impl Matcher {
    /// Builds a matcher. Literal patterns are escaped and run through the same engine.
    /// No normalization of any kind is applied to the pattern.
    pub fn new(pattern: &str, regex: bool, ignore_case: bool) -> Result<Self, PatternError> {
        if pattern.is_empty() {
            return Err(PatternError::Empty);
        }
        let source = if regex {
            pattern.to_string()
        } else {
            fancy_regex::escape(pattern).into_owned()
        };
        let re = RegexBuilder::new(&source)
            .case_insensitive(ignore_case)
            .build()
            .map_err(|e| PatternError::Invalid(e.to_string()))?;
        Ok(Matcher { re })
    }

    /// Returns non-overlapping matches as `(start, end)` char offsets, left to right.
    /// Zero-length matches are dropped.
    pub fn find(&self, text: &str) -> Result<Vec<(usize, usize)>, fancy_regex::Error> {
        let mut out = Vec::new();
        let mut cursor = CharCursor::new(text);
        for m in self.re.find_iter(text) {
            let m = m?;
            if m.start() == m.end() {
                continue;
            }
            let start = cursor.char_index(m.start());
            let end = cursor.char_index(m.end());
            out.push((start, end));
        }
        Ok(out)
    }
}

/// Converts increasing byte offsets into char offsets in a single pass.
struct CharCursor<'a> {
    text: &'a str,
    byte: usize,
    chars: usize,
}

impl<'a> CharCursor<'a> {
    fn new(text: &'a str) -> Self {
        CharCursor {
            text,
            byte: 0,
            chars: 0,
        }
    }

    fn char_index(&mut self, byte: usize) -> usize {
        if byte < self.byte {
            // Not expected for find_iter output; recount from the start.
            self.byte = 0;
            self.chars = 0;
        }
        let counted = self
            .text
            .get(self.byte..byte)
            .map(|s| s.chars().count())
            .unwrap_or(0);
        self.chars += counted;
        self.byte = byte;
        self.chars
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(pattern: &str, regex: bool, ignore_case: bool, text: &str) -> Vec<(usize, usize)> {
        Matcher::new(pattern, regex, ignore_case)
            .unwrap()
            .find(text)
            .unwrap()
    }

    #[test]
    fn server_matches_server_with_long_vowel_as_substring() {
        assert_eq!(
            find("サーバ", false, false, "サーバとサーバーとWebサーバ群"),
            vec![(0, 3), (4, 7), (12, 15)]
        );
    }

    #[test]
    fn negative_lookahead_excludes_long_vowel() {
        assert_eq!(
            find("サーバ(?!ー)", true, false, "サーバとサーバー"),
            vec![(0, 3)]
        );
    }

    #[test]
    fn full_width_and_half_width_are_distinct() {
        assert!(find("ABC", false, false, "ＡＢＣ").is_empty());
        assert!(find("ｻｰﾊﾞ", false, false, "サーバ").is_empty());
        assert!(find("1", false, false, "１").is_empty());
    }

    #[test]
    fn hiragana_and_katakana_are_distinct() {
        assert!(find("さーば", false, false, "サーバ").is_empty());
    }

    #[test]
    fn ignore_case_folds_full_width_letters_only_among_themselves() {
        assert_eq!(find("Ａ", false, true, "ａ"), vec![(0, 1)]);
        assert!(find("a", false, true, "Ａ").is_empty());
        assert!(find("Ａ", false, true, "a").is_empty());
        assert_eq!(find("abc", false, true, "xABCx"), vec![(1, 4)]);
    }

    #[test]
    fn ignore_case_works_with_lookaround() {
        assert_eq!(find("a(?=b)", true, true, "AB"), vec![(0, 1)]);
    }

    #[test]
    fn case_is_significant_by_default() {
        assert!(find("abc", false, false, "ABC").is_empty());
    }

    #[test]
    fn nfc_and_nfd_do_not_match() {
        let nfc = "\u{30AC}"; // ガ
        let nfd = "\u{30AB}\u{3099}"; // カ + combining dakuten
        assert!(find(nfc, false, false, nfd).is_empty());
        assert!(find(nfd, false, false, nfc).is_empty());
    }

    #[test]
    fn literal_mode_escapes_metacharacters() {
        assert!(find("a.b", false, false, "axb").is_empty());
        assert_eq!(find("a.b", false, false, "a.b"), vec![(0, 3)]);
        assert_eq!(find("(1+1)*2", false, false, "x(1+1)*2"), vec![(1, 8)]);
    }

    #[test]
    fn regex_newline_matches_line_break() {
        assert_eq!(find("名\\n役", true, false, "サーバ名\n役割"), vec![(3, 6)]);
    }

    #[test]
    fn offsets_are_chars_even_for_astral_characters() {
        assert_eq!(find("x", false, false, "😀𠮷x"), vec![(2, 3)]);
    }

    #[test]
    fn matches_are_non_overlapping() {
        assert_eq!(find("aa", false, false, "aaaa"), vec![(0, 2), (2, 4)]);
        assert_eq!(find("aa", false, false, "aaa"), vec![(0, 2)]);
    }

    #[test]
    fn empty_pattern_is_rejected() {
        assert!(matches!(
            Matcher::new("", false, false),
            Err(PatternError::Empty)
        ));
        assert!(matches!(
            Matcher::new("", true, false),
            Err(PatternError::Empty)
        ));
    }

    #[test]
    fn invalid_regex_is_rejected() {
        assert!(matches!(
            Matcher::new("(abc", true, false),
            Err(PatternError::Invalid(_))
        ));
    }

    #[test]
    fn zero_length_matches_are_dropped() {
        assert!(find("^", true, false, "abc").is_empty());
        assert_eq!(find("x*", true, false, "axxb"), vec![(1, 3)]);
    }

    #[test]
    fn whitespace_is_not_collapsed() {
        assert!(find("a b", false, false, "a  b").is_empty());
        assert!(find("a b", false, false, "a\u{3000}b").is_empty());
    }
}
