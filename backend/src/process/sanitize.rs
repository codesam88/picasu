use unicode_normalization::UnicodeNormalization as _;

/// Returns true if `c` is a valid XML 1.0 character and not a C1 control.
///
/// XML 1.0 §2.2: Char ::= #x9 | #xA | #xD | [#x20–#xD7FF] | [#xE000–#xFFFD] | [#x10000–#x10FFFF]
/// C1 controls (U+0080–U+009F) are technically within the XML 1.0 range but are
/// non-printable legacy control characters that no XMP tool produces intentionally.
pub fn is_valid_xml_char(c: char) -> bool {
    matches!(c,
        '\u{0009}' | '\u{000A}' | '\u{000D}'
        | '\u{0020}'..='\u{007E}'           // printable ASCII
        | '\u{00A0}'..='\u{D7FF}'           // skips C1 controls (U+0080–U+009F) and DEL (U+007F)
        | '\u{E000}'..='\u{FFFD}'
        | '\u{10000}'..='\u{10FFFF}'
    )
}

/// Sanitize a tag string for storage and XMP output.
/// - NFC-normalises for interoperability with Lightroom/digiKam/exiv2
/// - Keeps only valid XML 1.0 characters (excluding C1 controls)
/// - Strips newlines and carriage returns (tags must be single-line)
pub fn sanitize_tag(s: &str) -> String {
    s.nfc()
        .filter(|&c| is_valid_xml_char(c) && c != '\n' && c != '\r')
        .collect()
}

/// Sanitize a free-text field (e.g. description) for storage and XMP output.
/// - NFC-normalises for interoperability
/// - Keeps only valid XML 1.0 characters (excluding C1 controls)
/// - Preserves newlines and tabs as intentional formatting
pub fn sanitize_text(s: &str) -> String {
    s.nfc().filter(|&c| is_valid_xml_char(c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_strips_null_byte() {
        assert_eq!(sanitize_tag("foo\x00bar"), "foobar");
    }

    #[test]
    fn tag_strips_c0_controls() {
        assert_eq!(sanitize_tag("foo\x01\x08\x0Cbar"), "foobar");
    }

    #[test]
    fn tag_strips_del() {
        assert_eq!(sanitize_tag("foo\x7Fbar"), "foobar");
    }

    #[test]
    fn tag_strips_c1_controls() {
        // U+0080 (PAD) and U+009F (APC) are C1 controls
        assert_eq!(sanitize_tag("foo\u{0080}bar\u{009F}"), "foobar");
    }

    #[test]
    fn tag_strips_newline() {
        assert_eq!(sanitize_tag("foo\nbar"), "foobar");
    }

    #[test]
    fn tag_keeps_tab() {
        assert_eq!(sanitize_tag("foo\tbar"), "foo\tbar");
    }

    #[test]
    fn tag_keeps_unicode_letters() {
        assert_eq!(sanitize_tag("München"), "München");
        assert_eq!(sanitize_tag("日本語"), "日本語");
    }

    #[test]
    fn tag_nfc_normalises_nfd_input() {
        // "ü" as NFD: U+0075 + U+0308 → NFC: U+00FC
        let nfd = "u\u{0308}ber"; // ü decomposed
        let result = sanitize_tag(nfd);
        assert_eq!(result, "über"); // U+00FC composed
        assert_eq!(result.chars().next().unwrap(), '\u{00FC}');
    }

    #[test]
    fn text_keeps_newline_and_tab() {
        assert_eq!(sanitize_text("line1\nline2\ttab"), "line1\nline2\ttab");
    }

    #[test]
    fn text_strips_null_and_c0() {
        assert_eq!(sanitize_text("a\x00b\x1bc"), "abc");
    }

    #[test]
    fn text_strips_c1_controls() {
        assert_eq!(sanitize_text("a\u{0085}b"), "ab"); // U+0085 NEL
    }

    #[test]
    fn text_nfc_normalises_nfd_input() {
        let nfd = "cafe\u{0301}"; // "café" with combining acute
        assert_eq!(sanitize_text(nfd), "café"); // U+00E9 composed
    }

    #[test]
    fn text_keeps_unicode() {
        assert_eq!(sanitize_text("café\n日本語"), "café\n日本語");
    }

    #[test]
    fn empty_input_is_fine() {
        assert_eq!(sanitize_tag(""), "");
        assert_eq!(sanitize_text(""), "");
    }
}
