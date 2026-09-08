---
status: open
type: feature
priority: medium
area: testing
---

Remaining filename-sanitization corner cases to test, raised in PR \#17 review (2026-09-07). Existing coverage: 16 unit
tests on `sanitize_filename` (`backend/src/process/sanitize.rs`, `mod tests`) and 5 backend YAML scenarios
(`upload_auto_rename_*`, `upload_path_traversal`). Gaps below are untested; some also expose undecided behavior.

## Corner-case gaps

1. **Control characters are not filtered at all.** `is_forbidden_filename_char` (`sanitize.rs:50`) strips tier-0
   separators/null, tier-1 Windows chars, tier-2 noncharacters/zero-width/bidi — but not C0 (`\u{0001}`–`\u{001F}`),
   DEL (`\u{007F}`), or C1 (`\u{0080}`–`\u{009F}`). A filename containing `\n` or `\u{0001}` passes through.
   `sanitize_text` filters these via `is_valid_xml_char`; decide whether `sanitize_filename` should too, then pin with a
   test. (Untested behavior, not necessarily a defect.)

2. **`resolve_filename` (`post_upload.rs:74`) has no unit tests** — only integration coverage via the YAML scenarios.
   Needs direct tests for:

   - the generated-name fallback: when the sanitized stem is empty the result must be `upload-{uuid}.{ext}`; the
     existing `upload_auto_rename_fallback` scenario only asserts `file_absent .jpeg`, never the actual final name
     (`save_file` appends the UUID only when `on_conflict` is `None`).
   - stem-derivation edges through `Path::file_stem`: `...jpg`, `..jpg`, `.jpg`, trailing-dot names like `foo.`.
   - `auto_rename=false` reject-message building (`auto_rename_rejected_error`) for each reason class: forbidden chars,
     reserved name, normalization.
   - degenerate-name `auto_rename=true` + `on_conflict=rename|replace` interaction.

3. **Fullwidth / homoglyph separators are not stripped or tested**: U+FF0F `／`, U+2215 `∕`, U+FF5C `｜`, U+FF08
   etc. pass through the tiers. Decide whether these belong in tier 2.

4. **Tier-2 boundary coverage is thin**: tests cover BMP `U+FFFE` and `U+FDD0`/`U+200B`/`U+202E` only — not the other
   planes' `FFFE/FFFF` (`(c as u32) & 0xFFFF == 0xFFFE` branch), the `FDD1`–`FDEF` interior,
   `U+200E/200F`/`U+2066`–`U+2069` bidi set, or `U+FEFF`/`U+2060`.

5. **NFC-off path has no backend scenario** (unit tests cover it; `control` verbs cannot currently encode NFD filenames
   in the upload DSL).

## Related

Tracked in `upload-conflict.md` (done) is the tier design + auto\_rename decision; this ticket tracks the remaining
test/decision space only.

## Progress (2026-09-08)

Items 1 and 2 resolved in PR \#17: control chars (C0 U+0000–U+001F, DEL U+007F, C1 U+0080–U+009F) are now stripped by
the always-on control-char tier in `sanitize_filename` (`filename_strips_c0_controls`/`_del`/`_c1_controls` +
`filename_only_controls_degrade_to_empty`); `resolve_filename` gained 11 unit tests
(`resolve_filename_tests` module) covering the `upload` fallback stem, `file_stem` edges (`.jpg`, `..jpg`, `foo.`), the
four `auto_rename=false` reject reasons, and control-char strip/reject. Remaining open: fullwidth/homoglyph separators
(item 3), extended tier-2 boundary coverage (item 4), and an NFC-off backend scenario (item 5). Merged fix data point:
`cargo test` 190 lib tests pass.
