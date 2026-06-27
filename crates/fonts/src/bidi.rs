//! Bidirectional reordering via `unicode-bidi` (Fase 3E.3).
//!
//! Splits a logical-order string into runs in *visual* order, each tagged with
//! its direction. The caller shapes each run with the matching [`Direction`]
//! (the shaper handles glyph mirroring within a run).
//!
//! [`Direction`]: crate::Direction

use unicode_bidi::{BidiInfo, Level};

/// A maximal same-direction slice of the input, in visual order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BidiRun {
    /// The substring for this run (still in logical byte order).
    pub text: String,
    /// True if this run is right-to-left.
    pub rtl: bool,
    /// Byte offset of this run's start in the original string.
    pub start: usize,
}

/// Reorder `text` into visual-order runs.
///
/// `base_rtl` sets the paragraph base direction (e.g. Arabic/Hebrew documents).
/// Returns runs left-to-right in the order they should be laid out on the line.
pub fn reorder_runs(text: &str, base_rtl: bool) -> Vec<BidiRun> {
    if text.is_empty() {
        return Vec::new();
    }
    let base = if base_rtl { Level::rtl() } else { Level::ltr() };
    let info = BidiInfo::new(text, Some(base));

    let Some(para) = info.paragraphs.first() else {
        return vec![BidiRun {
            text: text.to_string(),
            rtl: base_rtl,
            start: 0,
        }];
    };

    let line = para.range.clone();
    let (levels, runs) = info.visual_runs(para, line);

    runs.into_iter()
        .map(|range| {
            let rtl = levels[range.start].is_rtl();
            BidiRun {
                text: text[range.clone()].to_string(),
                rtl,
                start: range.start,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_ltr_is_one_run() {
        let runs = reorder_runs("hello world", false);
        assert_eq!(runs.len(), 1);
        assert!(!runs[0].rtl);
        assert_eq!(runs[0].text, "hello world");
    }

    #[test]
    fn mixed_ltr_rtl_splits_into_runs() {
        // Latin + Hebrew + Latin. Visual order differs from logical order.
        let runs = reorder_runs("abc \u{05D0}\u{05D1}\u{05D2} xyz", false);
        assert!(runs.len() >= 2, "expected multiple runs, got {runs:?}");
        assert!(runs.iter().any(|r| r.rtl), "expected an RTL run");
        assert!(runs.iter().any(|r| !r.rtl), "expected an LTR run");
    }
}
