//! Font-size statistics: heading-tier discovery and rarity-based structure
//! detection. Ported from firecrawl/pdf-inspector `src/markdown/analysis.rs`.
//!
//! Headings and labels are rendered in font sizes that appear on far fewer
//! spans than body text. Rarity = 1 - (frequency ratio) turns that into a
//! 0.0-1.0 score used by schema inference to weight label candidates.

use crate::TextSpan;
use std::collections::HashMap;

/// Document-wide font-size distribution.
#[derive(Debug, Clone, Default)]
pub struct FontStats {
    pub most_common_size: f32,
    /// (font_size * 10) -> span count.
    pub size_counts: HashMap<i32, usize>,
    pub total_spans: usize,
}

/// Count font sizes across spans. Sizes below 9pt (footnotes, folios) are
/// excluded so captions don't skew the base.
pub fn calculate_font_stats(spans: &[TextSpan]) -> FontStats {
    let mut size_counts: HashMap<i32, usize> = HashMap::new();
    for s in spans {
        if s.font_size >= 9.0 {
            let key = (s.font_size * 10.0) as i32;
            *size_counts.entry(key).or_insert(0) += 1;
        }
    }
    let total_spans: usize = size_counts.values().sum();
    // Break ties by preferring the smaller font size for deterministic output.
    let most_common_size = size_counts
        .iter()
        .max_by(|(a, ca), (b, cb)| ca.cmp(cb).then_with(|| b.cmp(a)))
        .map(|(k, _)| *k as f32 / 10.0)
        .unwrap_or(12.0);
    FontStats {
        most_common_size,
        size_counts,
        total_spans,
    }
}

/// How rare a font size is (0.0 = most common, 1.0 = unique). Heading and
/// label fonts appear on few lines, so their rarity is high.
pub fn font_size_rarity(font_size: f32, stats: &FontStats) -> f32 {
    if stats.total_spans == 0 {
        return 0.0;
    }
    let key = (font_size * 10.0) as i32;
    let count = stats.size_counts.get(&key).copied().unwrap_or(0);
    1.0 - (count as f32 / stats.total_spans as f32)
}

/// Discover distinct heading font-size tiers in the document, largest first.
/// Sizes within 0.5pt cluster into one tier; capped at 4 tiers. Digit-only
/// lines (page numbers, issue numbers) never define a tier. When nothing
/// clears the 1.2x ratio gate, bold lines modestly above body size become
/// the tiers so same-size bold headings still get structure.
pub fn compute_heading_tiers(spans: &[TextSpan], base_size: f32) -> Vec<f32> {
    let mut sizes: Vec<f32> = spans
        .iter()
        .filter(|s| s.font_size / base_size >= 1.2)
        .filter(|s| {
            let t = s.text.trim();
            !t.is_empty() && t.chars().any(|c| c.is_alphabetic())
        })
        .map(|s| s.font_size)
        .collect();
    sizes.sort_by(|a, b| b.total_cmp(a));

    let mut tiers: Vec<f32> = Vec::new();
    for size in sizes {
        if !tiers.iter().any(|&t| (t - size).abs() < 0.5) {
            tiers.push(size);
        }
    }

    if tiers.is_empty() {
        let mut bold_sizes: Vec<f32> = spans
            .iter()
            .filter(|s| {
                let t = s.text.trim();
                !t.is_empty() && t.chars().any(|c| c.is_alphabetic())
            })
            .filter(|s| s.is_bold && s.font_size / base_size >= 1.05)
            .map(|s| s.font_size)
            .collect();
        bold_sizes.sort_by(|a, b| b.total_cmp(a));
        for size in bold_sizes {
            if !tiers.iter().any(|&t| (t - size).abs() < 0.5) {
                tiers.push(size);
            }
        }
    }

    tiers.truncate(4);
    tiers
}

/// True when a span is likely structural (a label or heading) rather than
/// body prose. Uses font metadata when present, falling back to a trailing
/// colon as the label signal.
pub fn is_likely_label(span: &TextSpan, stats: &FontStats) -> bool {
    let text = span.text.trim();
    if text.is_empty() {
        return false;
    }
    // Bold always reads as structural (section headers, field labels).
    if span.is_bold {
        return true;
    }
    // Rare size -> likely a label/heading.
    if span.font_size > 0.0 && font_size_rarity(span.font_size, stats) >= 0.4 {
        return true;
    }
    // No font metadata (or common body size): a short span ending in ':'
    // is a field label; anything else is ambiguous prose.
    text.ends_with(':') && text.chars().count() <= 40 && text.chars().any(|c| c.is_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str, font_size: f32, bold: bool) -> TextSpan {
        TextSpan {
            text: text.into(),
            x0: 0.0,
            y0: 0.0,
            x1: 10.0,
            y1: 10.0,
            page: Some(1),
            font_size,
            is_bold: bold,
            is_italic: false,
        }
    }

    #[test]
    fn rarity_prefers_rare_sizes() {
        let spans = vec![span("body", 10.0, false); 80]
            .into_iter()
            .chain(vec![span("HEADING", 18.0, false); 3])
            .collect::<Vec<_>>();
        let stats = calculate_font_stats(&spans);
        let body_rarity = font_size_rarity(10.0, &stats);
        let heading_rarity = font_size_rarity(18.0, &stats);
        assert!(heading_rarity > body_rarity);
        assert!((body_rarity - (1.0 - 80.0 / 83.0)).abs() < 0.001);
    }

    #[test]
    fn heading_tiers_cluster_within_half_pt() {
        let spans = vec![
            span("body text", 10.0, false),
            span("Chapter One", 18.2, false),
            span("Section 1", 18.0, false),
            span("Subsection", 14.1, false),
        ];
        let tiers = compute_heading_tiers(&spans, 10.0);
        assert_eq!(tiers, vec![18.2, 14.1]);
        assert_eq!(tiers.len(), 2);
    }

    #[test]
    fn digit_only_lines_do_not_form_tiers() {
        let spans = vec![
            span("76", 14.0, true),
            span("Replace", 11.0, true),
            span("body text at eleven points", 11.0, false),
        ];
        let tiers = compute_heading_tiers(&spans, 11.0);
        assert!(tiers.is_empty(), "page number claimed a tier: {tiers:?}");
    }

    #[test]
    fn bold_fallback_when_nothing_clears_ratio_gate() {
        let spans = vec![
            span("4. Entropy", 11.0, true),
            span("body text about entropy", 10.0, false),
            span("5. The dynamics", 11.0, true),
        ];
        let tiers = compute_heading_tiers(&spans, 10.0);
        assert_eq!(tiers, vec![11.0]);
    }

    #[test]
    fn bold_span_is_label_regardless_of_rarity() {
        let spans = vec![
            span("Invoice Number:", 10.0, true),
            span("body", 10.0, false),
        ];
        let stats = calculate_font_stats(&spans);
        assert!(is_likely_label(&spans[0], &stats));
        assert!(!is_likely_label(&spans[1], &stats));
    }

    #[test]
    fn long_body_prose_with_colon_is_not_label() {
        let spans = vec![
            span("body text", 10.0, false),
            span(
                "This is a long sentence that mentions: the following points",
                10.0,
                false,
            ),
        ];
        let stats = calculate_font_stats(&spans);
        assert!(!is_likely_label(&spans[1], &stats));
    }
}
