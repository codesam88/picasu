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

/// Result of sanitizing an uploaded filename.
pub struct FilenameSanitize {
    /// The sanitized filename (empty if it degrades to nothing).
    pub name: String,
    /// Forbidden characters removed by tier 0/1/2/3 filtering.
    pub stripped: Vec<char>,
    /// NFC normalization altered the name.
    pub normalized: bool,
    /// A reserved Windows name was prefixed with `_`.
    pub reserved: bool,
    /// True if any sanitization step changed the name.
    pub changed: bool,
}

fn is_forbidden_filename_char(c: char) -> bool {
    // Tier 0: directory separators and null byte.
    if c == '\0' || c == '/' || c == '\\' {
        return true;
    }
    // Tier 1: Windows-forbidden characters.
    if matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
        return true;
    }
    // Tier 2: Unicode landmines.
    if ('\u{FDD0}'..='\u{FDEF}').contains(&c) {
        return true;
    }
    if (c as u32) & 0xFFFF == 0xFFFE || (c as u32) & 0xFFFF == 0xFFFF {
        return true;
    }
    if matches!(
        c,
        '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{200E}' | '\u{200F}'
    ) {
        return true;
    }
    if matches!(c, '\u{FEFF}' | '\u{2060}') {
        return true;
    }
    if ('\u{202A}'..='\u{202E}').contains(&c) || ('\u{2066}'..='\u{2069}').contains(&c) {
        return true;
    }
    // Tier 3: control characters (C0 U+0000–U+001F, DEL U+007F, C1 U+0080–U+009F).
    if c.is_control() {
        return true;
    }
    false
}

/// Windows reserved device names, matched case-insensitively against the stem
/// (part before the first `.`). Any extension is reserved on Windows.
const RESERVED_WINDOWS_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", //
    "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3",
    "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn is_reserved_windows_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    RESERVED_WINDOWS_NAMES
        .iter()
        .any(|r| stem.eq_ignore_ascii_case(r))
}

/// Sanitize an uploaded filename for safe storage.
///
/// Tier 0/1/2/3 filtering (separators, Windows-forbidden characters, Unicode
/// landmines, control characters) is always applied. When `normalize_nfc` is set, the name is
/// NFC-normalised so macOS NFD names collapse onto their composed form.
/// Windows reserved names get a `_` prefix so they remain recognizable.
///
/// If the result degrades to empty, `.` or `..`, `name` is returned as empty.
pub fn sanitize_filename(name: &str, normalize_nfc: bool) -> FilenameSanitize {
    let original = name;
    let mut stripped: Vec<char> = Vec::new();
    let mut cleaned: String = name
        .chars()
        .filter(|&c| {
            if is_forbidden_filename_char(c) {
                stripped.push(c);
                false
            } else {
                true
            }
        })
        .collect();

    cleaned = cleaned.trim().to_string();

    let normalized = normalize_nfc && cleaned.nfc().collect::<String>() != cleaned;
    if normalize_nfc {
        cleaned = cleaned.nfc().collect();
    }

    let reserved = is_reserved_windows_name(&cleaned);
    if reserved {
        cleaned.insert(0, '_');
    }

    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        cleaned.clear();
    }

    FilenameSanitize {
        changed: cleaned != original,
        name: cleaned,
        stripped,
        normalized,
        reserved,
    }
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

    // ── filename sanitization ──

    fn s(name: &str, nfc: bool) -> FilenameSanitize {
        sanitize_filename(name, nfc)
    }

    #[test]
    fn filename_passes_through_clean_name() {
        let r = s("photo.jpg", true);
        assert_eq!(r.name, "photo.jpg");
        assert!(!r.changed);
        assert!(r.stripped.is_empty());
        assert!(!r.normalized);
        assert!(!r.reserved);
    }

    #[test]
    fn filename_keeps_brackets_and_unicode() {
        let r = s("photo (1) [copy].jpg", true);
        assert_eq!(r.name, "photo (1) [copy].jpg");
        assert!(!r.changed);
    }

    #[test]
    fn filename_strips_path_separators_tier0() {
        let r = s("../../etc/passwd.jpg", true);
        assert!(!r.name.contains('/'));
        assert!(!r.name.contains('\\'));
        assert!(r.stripped.contains(&'/'));
        assert!(r.changed);
    }

    #[test]
    fn filename_strips_null_byte() {
        let r = s("a\x00b.jpg", true);
        assert_eq!(r.name, "ab.jpg");
        assert!(r.stripped.contains(&'\0'));
    }

    #[test]
    fn filename_strips_windows_forbidden_tier1() {
        let r = s("a<b>:c\"d|e?f*g.jpg", true);
        assert_eq!(r.name, "abcdefg.jpg");
        for ch in ['<', '>', ':', '"', '|', '?', '*'] {
            assert!(r.stripped.contains(&ch), "should have stripped {ch:?}");
        }
    }

    #[test]
    fn filename_prefixes_reserved_windows_name() {
        let r = s("con.jpg", true);
        assert_eq!(r.name, "_con.jpg");
        assert!(r.reserved);
        assert!(r.changed);
    }

    #[test]
    fn filename_prefixes_reserved_name_without_extension() {
        let r = s("AUX", true);
        assert_eq!(r.name, "_AUX");
        assert!(r.reserved);
    }

    #[test]
    fn filename_reserved_name_case_insensitive() {
        let r = s("CoM3.txt", true);
        assert_eq!(r.name, "_CoM3.txt");
        assert!(r.reserved);
    }

    #[test]
    fn filename_not_reserved_for_regular_name() {
        let r = s("console.log", true);
        assert_eq!(r.name, "console.log");
        assert!(!r.reserved);
        assert!(!r.changed);
    }

    #[test]
    fn filename_strips_unicode_noncharacters_tier2() {
        let r = s("a\u{FDD0}b\u{FFFE}.jpg", true);
        assert_eq!(r.name, "ab.jpg");
        assert!(r.stripped.contains(&'\u{FDD0}'));
        assert!(r.stripped.contains(&'\u{FFFE}'));
    }

    #[test]
    fn filename_strips_zero_width_and_bidi_overrides() {
        let r = s("a\u{200B}b\u{202E}c.jpg", true);
        assert_eq!(r.name, "abc.jpg");
        assert!(r.stripped.contains(&'\u{200B}'));
        assert!(r.stripped.contains(&'\u{202E}'));
    }

    #[test]
    fn filename_nfc_normalizes_when_enabled() {
        let nfd = "cafe\u{0301}.jpg"; // café decomposed
        let r = s(nfd, true);
        assert_eq!(r.name, "café.jpg"); // U+00E9 composed
        assert!(r.normalized);
        assert!(r.changed);
    }

    #[test]
    fn filename_skips_nfc_when_disabled() {
        let nfd = "cafe\u{0301}.jpg";
        let r = s(nfd, false);
        assert_eq!(r.name, nfd);
        assert!(!r.normalized);
        assert!(!r.changed);
    }

    #[test]
    fn filename_degenerate_dot_and_dotdot_become_empty() {
        assert_eq!(s(".", true).name, "");
        assert_eq!(s("..", true).name, "");
        assert_eq!(s("///", true).name, "");
        assert_eq!(s("...", true).name, "...");
    }

    #[test]
    fn filename_trims_whitespace() {
        let r = s("  photo.jpg  ", true);
        assert_eq!(r.name, "photo.jpg");
        assert!(r.changed);
    }

    #[test]
    fn filename_all_stripped_becomes_empty() {
        let r = s("///", true);
        assert_eq!(r.name, "");
        assert!(r.changed);
    }

    #[test]
    fn filename_strips_c0_controls() {
        let r = s("a\x01\x08\x0Cb.jpg", true);
        assert_eq!(r.name, "ab.jpg");
        for ch in ['\x01', '\x08', '\x0C'] {
            assert!(r.stripped.contains(&ch), "should have stripped {ch:?}");
        }
        assert!(r.changed);
    }

    #[test]
    fn filename_strips_del() {
        let r = s("a\x7Fb.jpg", true);
        assert_eq!(r.name, "ab.jpg");
        assert!(r.stripped.contains(&'\x7F'));
        assert!(r.changed);
    }

    #[test]
    fn filename_strips_c1_controls() {
        // U+0080 (PAD) and U+009F (APC) are C1 controls
        let r = s("a\u{0080}b\u{009F}.jpg", true);
        assert_eq!(r.name, "ab.jpg");
        assert!(r.stripped.contains(&'\u{0080}'));
        assert!(r.stripped.contains(&'\u{009F}'));
        assert!(r.changed);
    }

    #[test]
    fn filename_only_controls_degrade_to_empty() {
        let r = s("/\x01/", true);
        assert_eq!(r.name, "");
        assert!(r.changed);
        assert!(r.stripped.contains(&'/'));
        assert!(r.stripped.contains(&'\x01'));
    }
}
