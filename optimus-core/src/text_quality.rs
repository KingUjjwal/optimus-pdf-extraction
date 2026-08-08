//! Text-quality detection: deciding when an extracted text layer is too broken
//! to serve and a page should be flagged for OCR.
//!
//! Extraction can produce plausible-looking bytes that are actually garbage —
//! failed CID->Unicode mappings, broken ToUnicode CMaps, mojibake. These
//! detectors catch that so callers can route pages to OCR or warn the user.
//! Ported from firecrawl/pdf-inspector `src/text_quality.rs` (MIT).
//!
//! Detection classes, roughly by signal:
//! - **Replacement runs**: U+FFFD clusters.
//! - **Private-use / C1-control runs**: CID passthrough landing in PUA or the C1 block.
//! - **Dollar-as-space**: `Word$Word$Word` from broken CMaps.
//! - **Non-alphanumeric dominance**: symbol soup.
//! - **Substitution-cipher letter statistics**: pure-ASCII output whose letter
//!   distribution is a permutation of natural language.

use crate::TextSpan;
use std::collections::BTreeMap;

/// OCR reason codes, kept string-stable for frontend/event consumption.
pub const OCR_REASON_SCANNED: &str = "scanned";
pub const OCR_REASON_NO_TEXT: &str = "no_text";
pub const OCR_REASON_VECTOR_TEXT: &str = "vector_text";
pub const OCR_REASON_SUSPECTED_GARBLED_TEXT: &str = "suspected_garbled_text";

/// English letter frequencies (percent, a-z). Every Latin-script language in
/// the eval corpus scores >= 0.80 cosine similarity against it, while
/// substitution-cipher text scores ~0.53.
const ENGLISH_LETTER_FREQ: [f64; 26] = [
    8.2, 1.5, 2.8, 4.3, 12.7, 2.2, 2.0, 6.1, 7.0, 0.15, 0.8, 4.0, 2.4, 6.7, 7.5, 1.9, 0.1, 6.0,
    6.3, 9.1, 2.8, 1.0, 2.4, 0.15, 2.0, 0.07,
];

/// Letter statistics for detecting substitution-cipher garbling: broken
/// ToUnicode CMaps shift every character by a per-range constant (e.g.
/// `Certificate` extracted as `8VceZWZTReV`). Such text is 100% printable
/// ASCII with word-like token lengths, so it defeats `is_garbage_text` and
/// produces no replacement characters.
#[derive(Debug, Default)]
struct CipherGarbleStats {
    letter_counts: [u32; 26],
    ascii_letters: usize,
    ascii_vowels: usize,
    latin_ext_letters: usize,
    non_latin_letters: usize,
    letter_bigrams: usize,
    case_shift_bigrams: usize,
}

impl CipherGarbleStats {
    fn add_text(&mut self, text: &str) {
        let mut prev: Option<char> = None;
        for ch in text.chars() {
            if ch.is_ascii_alphabetic() {
                let idx = (ch.to_ascii_lowercase() as u8 - b'a') as usize;
                self.letter_counts[idx] += 1;
                self.ascii_letters += 1;
                if matches!(ch.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u') {
                    self.ascii_vowels += 1;
                }
                if let Some(p) = prev {
                    self.letter_bigrams += 1;
                    if p.is_ascii_lowercase() && ch.is_ascii_uppercase() {
                        self.case_shift_bigrams += 1;
                    }
                }
                prev = Some(ch);
            } else {
                if ch.is_alphabetic() {
                    if matches!(ch as u32, 0xC0..=0x24F | 0x1E00..=0x1EFF) {
                        self.latin_ext_letters += 1;
                    } else {
                        self.non_latin_letters += 1;
                    }
                }
                prev = None;
            }
        }
    }

    /// Cosine similarity between the observed letter histogram and English
    /// letter frequencies. A shifted alphabet permutes the histogram, which
    /// destroys the similarity regardless of the shift amount.
    fn english_cosine(&self) -> f64 {
        if self.ascii_letters == 0 {
            return 1.0;
        }
        let n = self.ascii_letters as f64;
        let mut dot = 0.0;
        let mut norm_obs = 0.0;
        for (count, freq) in self.letter_counts.iter().zip(ENGLISH_LETTER_FREQ) {
            let p = *count as f64 / n;
            dot += p * freq;
            norm_obs += p * p;
        }
        let norm_en = ENGLISH_LETTER_FREQ
            .iter()
            .map(|f| f * f)
            .sum::<f64>()
            .sqrt();
        dot / (norm_obs.sqrt() * norm_en)
    }

    /// Cosine similarity comparing the *shape* of the frequency profile
    /// (both sorted descending), ignoring which letter sits where. A
    /// substitution cipher is a bijection, so it preserves this shape exactly.
    fn english_shape_cosine(&self) -> f64 {
        if self.ascii_letters == 0 {
            return 1.0;
        }
        let n = self.ascii_letters as f64;
        let mut obs: [f64; 26] = std::array::from_fn(|i| self.letter_counts[i] as f64 / n);
        obs.sort_unstable_by(|a, b| b.total_cmp(a));
        let mut en = ENGLISH_LETTER_FREQ;
        en.sort_unstable_by(|a, b| b.total_cmp(a));

        let dot: f64 = obs.iter().zip(en).map(|(o, e)| o * e).sum();
        let norm_obs = obs.iter().map(|o| o * o).sum::<f64>().sqrt();
        let norm_en = en.iter().map(|e| e * e).sum::<f64>().sqrt();
        dot / (norm_obs * norm_en)
    }

    fn looks_garbled(&self) -> bool {
        if self.ascii_letters < 200
            || self.non_latin_letters > self.ascii_letters + self.latin_ext_letters
        {
            return false;
        }
        let vowel_ratio = self.ascii_vowels as f64 / self.ascii_letters as f64;
        if vowel_ratio > 0.30 {
            return false;
        }
        let case_shifts = self.letter_bigrams >= 100
            && self.case_shift_bigrams as f64 >= self.letter_bigrams as f64 * 0.10;
        let permuted_language = self.english_cosine() < 0.60 && self.english_shape_cosine() >= 0.90;
        case_shifts || permuted_language
    }
}

fn has_dollar_as_space_pattern(markdown: &str) -> bool {
    let total_dollars = markdown.matches('$').count();
    if total_dollars > 10 {
        let bytes = markdown.as_bytes();
        let mut letter_dollar_letter = 0usize;
        for i in 1..bytes.len().saturating_sub(1) {
            if bytes[i] == b'$'
                && bytes[i - 1].is_ascii_alphabetic()
                && bytes[i + 1].is_ascii_alphabetic()
            {
                letter_dollar_letter += 1;
            }
        }
        if letter_dollar_letter > 20 || letter_dollar_letter * 2 > total_dollars {
            return true;
        }
    }
    false
}

/// Check if extracted text is predominantly garbage (non-alphanumeric).
/// Real text in any language has >50% alphanumeric characters.
pub fn is_garbage_text(markdown: &str) -> bool {
    let mut alphanum = 0usize;
    let mut non_alphanum = 0usize;

    let chars: Vec<char> = markdown.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        let mut run_end = i + 1;
        while run_end < chars.len() && chars[run_end] == ch {
            run_end += 1;
        }

        let is_decorative_leader = matches!(ch, '.' | '_' | '·') && run_end - i >= 3;
        if !is_decorative_leader {
            for &run_ch in &chars[i..run_end] {
                if run_ch.is_whitespace() {
                    continue;
                }
                if matches!(run_ch, '#' | '*' | '|' | '-' | '\n') {
                    continue;
                }
                if run_ch.is_alphanumeric() {
                    alphanum += 1;
                } else {
                    non_alphanum += 1;
                }
            }
        }
        i = run_end;
    }

    let total = alphanum + non_alphanum;
    total >= 50 && alphanum * 2 < total
}

/// Detect broken font encodings: U+FFFD replacement chars, dollar-as-space,
/// or substitution-cipher letter statistics.
pub fn detect_encoding_issues(markdown: &str) -> bool {
    if markdown.contains('\u{FFFD}') {
        return true;
    }
    if has_dollar_as_space_pattern(markdown) {
        return true;
    }
    let mut stats = CipherGarbleStats::default();
    stats.add_text(markdown);
    stats.looks_garbled()
}

/// Detect garbage from failed CID-to-Unicode mapping on Identity-H fonts:
/// C1 control chars or CID-as-Latin-1 mojibake. Falls back to
/// `is_garbage_text` for non-alphanumeric-heavy patterns.
pub fn is_cid_garbage(text: &str) -> bool {
    if is_garbage_text(text) {
        return true;
    }
    let mut total = 0usize;
    let mut c1_control = 0usize;
    let mut high_latin = 0usize;
    for ch in text.chars() {
        if ch.is_whitespace() {
            continue;
        }
        total += 1;
        if ch == '·' {
            continue;
        }
        if ('\u{0080}'..='\u{009F}').contains(&ch) {
            c1_control += 1;
        }
        if ('\u{00A0}'..='\u{00FF}').contains(&ch) {
            high_latin += 1;
        }
    }
    if total < 5 {
        return false;
    }
    if c1_control >= 2 && c1_control * 20 >= total {
        return true;
    }
    let ascii_letters = text.chars().filter(|c| c.is_ascii_alphabetic()).count();
    total >= 20 && high_latin * 5 >= total * 2 && ascii_letters * 3 < total
}

fn replacement_text_stats(text: &str) -> (usize, usize) {
    let mut replacement = 0usize;
    let mut current_run = 0usize;
    let mut longest_run = 0usize;

    for ch in text.chars() {
        if ch == '\u{FFFD}' {
            replacement += 1;
            current_run += 1;
            longest_run = longest_run.max(current_run);
        } else {
            current_run = 0;
        }
    }

    (replacement, longest_run)
}

fn has_replacement_text_run(text: &str) -> bool {
    let (replacement, longest_run) = replacement_text_stats(text);
    longest_run >= 2 || replacement >= 3
}

fn has_private_use_text_run(text: &str) -> bool {
    let mut total = 0usize;
    let mut private_use = 0usize;
    let mut current_run = 0usize;
    let mut longest_run = 0usize;

    for ch in text.chars() {
        if ch.is_whitespace() {
            current_run = 0;
            continue;
        }
        total += 1;
        if is_private_use_char(ch) {
            private_use += 1;
            current_run += 1;
            longest_run = longest_run.max(current_run);
        } else {
            current_run = 0;
        }
    }

    if private_use == 0 {
        return false;
    }

    longest_run >= 3 || (total >= 5 && private_use >= 2 && private_use * 2 >= total)
}

fn has_cid_control_token(text: &str) -> bool {
    text.split_whitespace().any(token_has_cid_control)
}

fn token_has_cid_control(token: &str) -> bool {
    let mut total = 0usize;
    let mut c1_control = 0usize;

    for ch in token.chars() {
        total += 1;
        if ('\u{0080}'..='\u{009F}').contains(&ch) {
            c1_control += 1;
        }
    }

    total >= 5 && c1_control >= 2 && c1_control * 20 >= total
}

fn is_private_use_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextSpanIssueKind {
    Replacement,
    Strong,
}

fn text_span_issue_kind(text: &str) -> Option<TextSpanIssueKind> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    if has_dollar_as_space_pattern(text)
        || has_private_use_text_run(text)
        || is_cid_garbage(text)
        || has_cid_control_token(text)
    {
        return Some(TextSpanIssueKind::Strong);
    }

    if has_replacement_text_run(text) {
        return Some(TextSpanIssueKind::Replacement);
    }

    None
}

/// True when a single span carries a *strong* decoding issue (PUA, C1
/// controls, dollar-as-space, CID garbage) that makes it unsafe to serve.
/// Used for span-level filtering.
pub fn span_has_strong_issue(text: &str) -> bool {
    matches!(text_span_issue_kind(text), Some(TextSpanIssueKind::Strong))
}

#[derive(Debug, Default)]
struct PageTextQualityEvidence {
    chars: usize,
    replacement_chars: usize,
    replacement_spans: usize,
    longest_replacement_run: usize,
    cipher_garble: CipherGarbleStats,
}

fn page_replacement_evidence_needs_ocr(evidence: &PageTextQualityEvidence) -> bool {
    if evidence.replacement_chars == 0 || evidence.chars == 0 {
        return false;
    }
    if evidence.chars <= 80 && evidence.longest_replacement_run >= 2 {
        return true;
    }
    let replacement_density_bps = evidence.replacement_chars * 10_000 / evidence.chars;
    let enough_bad_text = evidence.replacement_chars >= 12 && replacement_density_bps >= 500;
    let repeated_bad_spans = evidence.replacement_spans >= 3 && replacement_density_bps >= 250;
    let long_bad_run = evidence.longest_replacement_run >= 8 && replacement_density_bps >= 250;

    enough_bad_text || repeated_bad_spans || long_bad_run
}

/// Result of analyzing extracted spans for decoding/encoding quality.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TextQualityReport {
    pub has_encoding_issues: bool,
    /// 1-indexed page numbers whose text layer is too broken to trust.
    pub pages_needing_ocr: Vec<u32>,
    /// 1-indexed page -> reason codes (see `OCR_REASON_*`).
    pub reasons_by_page: BTreeMap<u32, Vec<String>>,
}

impl TextQualityReport {
    /// True when the whole text layer looks healthy enough to run through
    /// schema inference and LLM codegen.
    pub fn is_usable(&self) -> bool {
        !self.has_encoding_issues
    }
}

/// Accumulates per-page evidence and flags pages whose text layer is garbled.
/// Localized bad spans on an otherwise clean page are caught without a single
/// span having to condemn the page.
pub fn analyze_text_quality(spans: &[TextSpan]) -> TextQualityReport {
    let mut reasons_by_page = BTreeMap::<u32, Vec<String>>::new();
    let mut evidence_by_page = BTreeMap::<u32, PageTextQualityEvidence>::new();

    for span in spans {
        let page = span.page.unwrap_or(0);
        let evidence = evidence_by_page.entry(page).or_default();
        evidence.chars += span.text.chars().filter(|ch| !ch.is_whitespace()).count();
        evidence.cipher_garble.add_text(&span.text);

        match text_span_issue_kind(&span.text) {
            Some(TextSpanIssueKind::Strong) => {
                reasons_by_page
                    .entry(page)
                    .or_default()
                    .push(OCR_REASON_SUSPECTED_GARBLED_TEXT.to_string());
            }
            Some(TextSpanIssueKind::Replacement) => {
                let stats = replacement_text_stats(&span.text);
                evidence.replacement_chars += stats.0;
                evidence.replacement_spans += 1;
                evidence.longest_replacement_run = evidence.longest_replacement_run.max(stats.1);
            }
            None => {}
        }
    }

    for (page, evidence) in evidence_by_page {
        if reasons_by_page.contains_key(&page) {
            continue;
        }
        if page_replacement_evidence_needs_ocr(&evidence) || evidence.cipher_garble.looks_garbled()
        {
            reasons_by_page
                .entry(page)
                .or_default()
                .push(OCR_REASON_SUSPECTED_GARBLED_TEXT.to_string());
        }
    }

    let mut pages_needing_ocr: Vec<u32> = reasons_by_page.keys().copied().collect();
    pages_needing_ocr.sort_unstable();
    TextQualityReport {
        has_encoding_issues: !pages_needing_ocr.is_empty(),
        pages_needing_ocr,
        reasons_by_page,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str, page: u32) -> TextSpan {
        TextSpan {
            text: text.into(),
            x0: 0.0,
            y0: 0.0,
            x1: 10.0,
            y1: 10.0,
            page: Some(page),
        }
    }

    #[test]
    fn garbage_symbol_soup_detected() {
        let soup = "----1-.-.-.___  --.-. .._ I_---.".repeat(6);
        assert!(is_garbage_text(&soup));
    }

    #[test]
    fn real_text_passes() {
        assert!(!is_garbage_text(
            "Invoice Number: INV-2026-001 dated 23 May 2026"
        ));
        assert!(!is_garbage_text("Acme Corp, 123 Main Street"));
    }

    #[test]
    fn replacement_chars_detected() {
        assert!(detect_encoding_issues("Total: \u{FFFD}\u{FFFD}\u{FFFD}.50"));
    }

    #[test]
    fn dollar_as_space_detected() {
        let bad = "word$".repeat(20) + "word";
        assert!(detect_encoding_issues(&bad));
    }

    #[test]
    fn substitution_cipher_detected() {
        // Shifted-alphabet garble like broken ToUnicode output, repeated to
        // satisfy the >=200-letter statistical sample guard.
        let garble = "8VceZWZTReVkZWPRjReV aWV ".repeat(20);
        assert!(detect_encoding_issues(&garble));
    }

    #[test]
    fn cid_c1_control_detected() {
        assert!(is_cid_garbage(
            "A\u{0082}\u{0093}B\u{0082}\u{0093}C\u{0082}\u{0093}D\u{0082}\u{0093}"
        ));
    }

    #[test]
    fn private_use_run_detected() {
        assert!(span_has_strong_issue("\u{E000}\u{E001}\u{E002}\u{E003}"));
    }

    #[test]
    fn clean_document_usable() {
        let spans = vec![
            span("Invoice Number: INV-001", 1),
            span("Date: 2026-05-23", 1),
            span("Total: $500.50", 1),
        ];
        let report = analyze_text_quality(&spans);
        assert!(report.is_usable());
        assert!(report.pages_needing_ocr.is_empty());
    }

    #[test]
    fn garbled_page_flagged_but_clean_page_spared() {
        let spans = vec![
            span("INVOICE", 1),
            span("Invoice Number: INV-001", 1),
            span("This is a perfectly clean body text line", 2),
            span("The quick brown fox jumps over the lazy dog", 2),
            span(
                "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}",
                2,
            ),
            span("More clean text on page two", 2),
        ];
        let report = analyze_text_quality(&spans);
        assert!(report.has_encoding_issues);
        assert!(report.reasons_by_page.contains_key(&2));
    }

    #[test]
    fn short_math_does_not_condemn_page() {
        let spans = vec![
            span("x = 2", 1),
            span("The equation above solves for the unknown variable x", 1),
            span("E = mc2 (12)", 1),
        ];
        let report = analyze_text_quality(&spans);
        assert!(report.is_usable());
    }
}
