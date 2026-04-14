use std::cell::RefCell;

use html5ever::tokenizer::{TagKind, Token, TokenSink, TokenSinkResult};

pub struct TagEvent {
    pub tag: String,
    pub is_open: bool,
    pub offset: usize,
}

// See: https://developer.mozilla.org/en-US/docs/Glossary/Void_element
static VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr",
];

pub fn is_void(tag: &str) -> bool {
    VOID_ELEMENTS.contains(&tag)
}

/// Token sink for html5ever's tokenizer. Collects parse errors, duplicate
/// attribute flags, and tag open/close events from a single HTML fragment.
///
/// Because html5ever processes tokens in a callback (`process_token`),
/// interior mutability is required — hence `RefCell` throughout.
pub struct HtmlTokenSink<'a> {
    /// The raw HTML fragment being tokenized.
    pub node_html: &'a str,

    /// Byte offset of this HTML fragment's start in the full markdown source.
    /// Added to fragment-local positions to get source-global offsets.
    pub node_offset: usize,

    /// Hard errors: parse errors from html5ever, non-void self-closing tags.
    /// `Some(offset)` = exact source-global offset.
    /// `None` = no offset available (html5ever parse errors don't provide spans).
    pub errors: RefCell<Vec<(Option<usize>, String)>>,

    /// Soft warnings: duplicate attributes, etc.
    /// `Some(offset)` = exact source-global offset.
    /// `None` = no offset available.
    pub warnings: RefCell<Vec<(Option<usize>, String)>>,

    /// Open/close events derived from `TagToken`s, for cross-node balance checking.
    pub tag_events: RefCell<Vec<TagEvent>>,

    /// How many times each tag name has appeared as a start/end tag so far.
    /// Used by `find_nth_tag` to resolve the correct byte offset when
    /// the same tag name appears multiple times in one fragment.
    start_counts: RefCell<std::collections::HashMap<String, usize>>,
    end_counts: RefCell<std::collections::HashMap<String, usize>>,
}

impl<'a> HtmlTokenSink<'a> {
    pub fn new(node_html: &'a str, node_offset: usize) -> Self {
        Self {
            node_html,
            node_offset,

            errors: RefCell::new(Vec::new()),
            warnings: RefCell::new(Vec::new()),
            tag_events: RefCell::new(Vec::new()),

            start_counts: RefCell::new(std::collections::HashMap::new()),
            end_counts: RefCell::new(std::collections::HashMap::new()),
        }
    }
}

impl TokenSink for HtmlTokenSink<'_> {
    type Handle = ();

    fn process_token(&self, token: Token, _line_number: u64) -> TokenSinkResult<()> {
        eprintln!("    process_token start: {token:?}");
        match token {
            Token::ParseError(msg) => {
                let msg_str = msg.to_string();
                eprintln!("      ParseError: {msg_str}");
                // html5ever provides no span/offset for parse errors
                self.errors.borrow_mut().push((None, msg_str));
            }
            Token::TagToken(ref tag) => {
                let name = String::from(&*tag.name);
                let (counts, is_start_tag) = match tag.kind {
                    TagKind::StartTag => (&self.start_counts, true),
                    TagKind::EndTag => (&self.end_counts, false),
                };

                // zero-based occurrence index of this tag name seen so far
                let n = {
                    let mut counts = counts.borrow_mut();
                    let count = counts.entry(name.clone()).or_insert(0);
                    let n = *count;
                    *count += 1;
                    n
                };
                let offset = find_nth_tag(self.node_html, &name, n, is_start_tag)
                    .map_or(self.node_offset, |p| self.node_offset + p);

                eprintln!(
                    "      TagToken: kind={} name={} self_closing={} nth={} resolved_offset={}",
                    if is_start_tag { "Start" } else { "End" },
                    name,
                    tag.self_closing,
                    n,
                    offset
                );

                if tag.had_duplicate_attributes {
                    eprintln!("      TagToken: duplicate attrs on <{name}> at offset {offset}");
                    self.warnings
                        .borrow_mut()
                        .push((Some(offset), "duplicate attribute".into()));
                }

                match tag.kind {
                    TagKind::StartTag if tag.self_closing && !is_void(&name) => {
                        eprintln!("      TagToken: StartTag non-void self-closing <{name}/>");
                        self.errors
                            .borrow_mut()
                            .push((Some(offset), format!("non-void tag: <{name}/> or <{name} />")));
                    }
                    TagKind::StartTag if !is_void(&name) => {
                        eprintln!("      TagToken: StartTag pushing OPEN event for <{name}>");
                        self.tag_events.borrow_mut().push(TagEvent {
                            tag: name,
                            is_open: true,
                            offset,
                        });
                    }
                    TagKind::StartTag => {} // void elements (e.g. <br>, <hr>)
                    TagKind::EndTag => {
                        eprintln!("      TagToken: EndTag pushing CLOSE event for </{name}>");
                        self.tag_events.borrow_mut().push(TagEvent {
                            tag: name,
                            is_open: false,
                            offset,
                        });
                    }
                }
            }
            Token::CharacterTokens(ref s) => {
                eprintln!("      CharacterTokens: {} chars", s.len());
            }
            _ => {
                eprintln!("      _: other token: {:?}", std::mem::discriminant(&token));
            }
        }

        eprintln!("    process_token end");
        TokenSinkResult::Continue
    }
}

enum ScanState {
    Normal,
    InTag,
    InQuote(u8),
    InComment,
}

/// Find the byte offset of the nth occurrence of `<tag` (or `</tag`) in `html`,
/// verifying a word boundary after the tag name to avoid matching `<span` in `<spaniel>`.
///
//                          ┌─────────────────────┐    <!--   ┌─────────────────────┐
//                          │       Normal        │──────────►│      InComment      │
//                          │                     │    -->    │                     │◄──┐
//                     ┌───►│  scan for <tag      │◄──────────│     skip until -->  │   │
//                     │    │  match + boundary   │◄──┐       └─────────────────┬───┘   │ not -->
//               not < │    └──┬───────┬──────────┘   │                         └───────┘
//                     └───────┘       │              │
//                                     │ <            │
//                                     ▼              │
//                          ┌─────────────────────┐   │ >
//                          │       InTag         │───┘
//                     ┌───►│                     │
//                     │    │  skip until > " '   │◄──┐
//   not (> or " or ') │    └──┬───────┬──────────┘   │
//                     └───────┘       │              │
//                                     │ " or '       │
//                                     ▼              │
//                          ┌─────────────────────┐   │ match q
//                          │    InQuote(q)       │───┘
//                     ┌───►│                     │
//                     │    │  skip until q       │
//          no match q │    └──┬──────────────────┘
//                     └───────┘
//
fn find_nth_tag(html: &str, tag: &str, n: usize, is_start_tag: bool) -> Option<usize> {
    if tag.is_empty() {
        return None;
    }

    let pattern = if is_start_tag {
        format!("<{}", tag.to_ascii_lowercase())
    } else {
        format!("</{}", tag.to_ascii_lowercase())
    };
    let pattern_bytes = pattern.as_bytes();
    let pattern_len = pattern_bytes.len();
    let bytes = html.as_bytes();
    let len = bytes.len();

    let mut count = 0;
    let mut i = 0;
    let mut state = ScanState::Normal;

    while i < len {
        match state {
            ScanState::Normal => {
                // Check for comment start before tag match
                if i + 4 <= len && &bytes[i..i + 4] == b"<!--" {
                    state = ScanState::InComment;
                    i += 4;
                    continue;
                }

                // case-insensitive tag match with word-boundary validation (<span> vs <spaniel>)
                if bytes[i] == b'<'
                    && i + pattern_len <= len
                    && bytes[i..i + pattern_len]
                        .iter()
                        .zip(pattern_bytes)
                        .all(|(a, b)| a.to_ascii_lowercase() == *b)
                {
                    let boundary = bytes.get(i + pattern_len);
                    let is_valid = match boundary {
                        None => true,
                        Some(b) => matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/'),
                    };
                    if is_valid {
                        if count == n {
                            return Some(i);
                        }
                        count += 1;
                    }
                }

                if bytes[i] == b'<' {
                    state = ScanState::InTag;
                }
                // else: implicit self-loop (state unchanged)
            }
            ScanState::InTag => match bytes[i] {
                b'"' | b'\'' => state = ScanState::InQuote(bytes[i]),
                b'>' => state = ScanState::Normal,
                b'<' => {
                    state = ScanState::Normal;
                    continue; // re-process this `<` in Normal state
                }
                _ => {} // self-loop (state unchanged)
            },
            ScanState::InQuote(q) => {
                if bytes[i] == q {
                    state = ScanState::InTag;
                }
                // else: implicit self-loop (state unchanged)
            }
            ScanState::InComment => {
                if i + 2 < len && &bytes[i..i + 3] == b"-->" {
                    state = ScanState::Normal;
                    i += 3;
                    continue;
                }
                // else: implicit self-loop (state unchanged)
            }
        }
        i += 1;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    mod find_nth_tag_tests {
        use super::*;

        mod transition_coverage {
            use super::*;

            // 1. Normal → Normal (no <)
            // 2. Normal → InComment (<!--)
            // 3. Normal → InTag (< — includes both matching and non-matching tags)
            // 4. InTag → Normal (>)
            // 5. InTag → Normal (<, continue/reprocess)
            // 6. InTag → InQuote (" or ')
            // 7. InTag → InTag (other bytes, self-loop)
            // 8. InQuote → InTag (matching quote)
            // 9. InQuote → InQuote (non-matching bytes, self-loop)
            // 10. InComment → Normal (-->)
            // 11. InComment → InComment (other bytes, self-loop)

            // 1. Normal → Normal
            #[test]
            fn normal_self_loop_no_tags() {
                assert_eq!(find_nth_tag("hello world", "div", 0, true), None);
            }

            // 2. Normal → InComment, 10. InComment → Normal
            #[test]
            fn empty_comment() {
                assert_eq!(find_nth_tag("<!----><div>", "div", 0, true), Some(7));
            }

            // 2. Normal → InComment, 11. InComment → InComment, 10. InComment → Normal
            #[test]
            fn tag_inside_comment_not_matched() {
                assert_eq!(find_nth_tag("<!-- <div> -->", "div", 0, true), None);
            }

            // 3. Normal → InTag, 7. InTag → InTag, 4. InTag → Normal (>)
            #[test]
            fn non_matching_tag_skipped() {
                assert_eq!(find_nth_tag("<span>text</span>", "div", 0, true), None);
            }

            // 3. Normal → InTag (match, start tag)
            #[test]
            fn match_start_tag() {
                assert_eq!(find_nth_tag("<div>", "div", 0, true), Some(0));
            }

            // 3. Normal → InTag (match, close tag)
            #[test]
            fn match_close_tag() {
                assert_eq!(find_nth_tag("</div>", "div", 0, false), Some(0));
            }

            // 3. Normal → InTag (match, nth counting)
            #[test]
            fn match_nth() {
                let html = "<div>a</div><div>b</div>";
                assert_eq!(find_nth_tag(html, "div", 0, true), Some(0));
                assert_eq!(find_nth_tag(html, "div", 1, true), Some(12));
                assert_eq!(find_nth_tag(html, "div", 2, true), None);
            }

            // 3. Normal → InTag (match rejected — boundary)
            #[test]
            fn boundary_rejects_longer_tag() {
                assert_eq!(find_nth_tag("<spaniel>", "span", 0, true), None);
            }

            // 3. Normal → InTag (match, boundary is None/EOF)
            #[test]
            fn tag_at_eof_no_close() {
                assert_eq!(find_nth_tag("<div", "div", 0, true), Some(0));
            }

            // 3. Normal → InTag (match, case insensitivity)
            #[test]
            fn case_insensitive_match() {
                assert_eq!(find_nth_tag("<DIV>", "div", 0, true), Some(0));
                assert_eq!(find_nth_tag("<Div>", "div", 0, true), Some(0));
            }

            // 5. InTag → Normal via < (reprocess)
            #[test]
            fn intag_to_normal_via_new_open() {
                assert_eq!(find_nth_tag("<span <div>", "div", 0, true), Some(6));
            }

            // 6. InTag → InQuote (double), 9. InQuote → InQuote, 8. InQuote → InTag
            #[test]
            fn start_tag_inside_double_quoted_attr() {
                assert_eq!(find_nth_tag(r#"<a href="<div>">real<div>"#, "div", 0, true), Some(20));
            }

            // 6. InTag → InQuote (single), 9. InQuote → InQuote, 8. InQuote → InTag
            #[test]
            fn start_tag_inside_single_quoted_attr() {
                assert_eq!(find_nth_tag("<a href='<div>'>real<div>", "div", 0, true), Some(20));
            }

            // 6. InTag → InQuote, 9. InQuote → InQuote, 8. InQuote → InTag (close tag)
            #[test]
            fn close_tag_inside_quoted_attr() {
                assert_eq!(find_nth_tag(r#"<a href="</div>"></div>"#, "div", 0, false), Some(17));
            }

            // 3. Normal → InTag (match, boundary is /)
            #[test]
            fn boundary_self_closing() {
                assert_eq!(find_nth_tag("<div/>", "div", 0, true), Some(0));
            }

            // 3. Normal → InTag (match, boundary is whitespace)
            #[test]
            fn boundary_whitespace() {
                assert_eq!(find_nth_tag("<div\n>", "div", 0, true), Some(0));
            }
        }

        mod state_specific {
            use super::*;

            // Already covered: tags inside quotes, tags inside comments,
            // adjacent comments, mixed quotes, unclosed quotes

            // Comment inside a quoted attribute — should not enter InComment
            #[test]
            fn comment_inside_quoted_attr() {
                assert_eq!(find_nth_tag(r#"<a data="<!--"><div>"#, "div", 0, true), Some(15));
            }

            // Quoted attribute inside a comment — should not enter InQuote
            #[test]
            fn quoted_attr_inside_comment() {
                assert_eq!(find_nth_tag(r#"<!-- <a href="x"> --><div>"#, "div", 0, true), Some(21));
            }

            // --> inside a quoted attribute — should not exit InQuote
            #[test]
            fn comment_close_inside_quoted_attr() {
                assert_eq!(find_nth_tag(r#"<a data="-->"><div>"#, "div", 0, true), Some(14));
            }

            // Nested-looking tags — inner tag should count
            #[test]
            fn nested_matching_tags() {
                let html = "<div><div></div></div>";
                assert_eq!(find_nth_tag(html, "div", 0, true), Some(0));
                assert_eq!(find_nth_tag(html, "div", 1, true), Some(5));
                assert_eq!(find_nth_tag(html, "div", 0, false), Some(10));
                assert_eq!(find_nth_tag(html, "div", 1, false), Some(16));
            }

            // Close tag inside open tag's attributes (malformed)
            #[test]
            fn close_tag_in_open_tag_attrs() {
                assert_eq!(find_nth_tag("<div </div>", "div", 0, false), Some(5));
            }

            // Multiple tags, only some match — verifies count skips non-matches
            #[test]
            fn mixed_tags_counting() {
                let html = "<span><div><em><div><b>";
                assert_eq!(find_nth_tag(html, "div", 0, true), Some(6));
                assert_eq!(find_nth_tag(html, "div", 1, true), Some(15));
                assert_eq!(find_nth_tag(html, "div", 2, true), None);
            }

            // Comment between two matches — comment doesn't reset count
            #[test]
            fn comment_between_matches() {
                let html = "<div><!-- <div> --><div>";
                assert_eq!(find_nth_tag(html, "div", 0, true), Some(0));
                assert_eq!(find_nth_tag(html, "div", 1, true), Some(19));
                assert_eq!(find_nth_tag(html, "div", 2, true), None);
            }

            // Quote containing comment containing tag — only outermost state wins
            #[test]
            fn tag_in_comment_in_quote() {
                assert_eq!(find_nth_tag(r#"<a x="<!-- <div> -->"><div>"#, "div", 0, true), Some(22));
            }

            // start_tag search should not match close tags
            #[test]
            fn start_search_ignores_close_tags() {
                assert_eq!(find_nth_tag("</div><div>", "div", 0, true), Some(6));
            }

            // close_tag search should not match start tags
            #[test]
            fn close_search_ignores_start_tags() {
                assert_eq!(find_nth_tag("<div></div>", "div", 0, false), Some(5));
            }
        }

        mod offset_verification {
            use super::*;

            #[test]
            fn count_offset_all_start_tags() {
                let html = "<div>a<div>b<div>c</div></div></div>";
                assert_eq!(find_nth_tag(html, "div", 0, true), Some(0));
                assert_eq!(find_nth_tag(html, "div", 1, true), Some(6));
                assert_eq!(find_nth_tag(html, "div", 2, true), Some(12));
                assert_eq!(find_nth_tag(html, "div", 3, true), None);
            }

            #[test]
            fn count_offset_all_close_tags() {
                let html = "<div>a<div>b<div>c</div></div></div>";
                assert_eq!(find_nth_tag(html, "div", 0, false), Some(18));
                assert_eq!(find_nth_tag(html, "div", 1, false), Some(24));
                assert_eq!(find_nth_tag(html, "div", 2, false), Some(30));
                assert_eq!(find_nth_tag(html, "div", 3, false), None);
            }

            #[test]
            fn count_offset_mixed_tags() {
                let html = "<p>x</p><div>y</div><p>z</p>";
                assert_eq!(find_nth_tag(html, "p", 0, true), Some(0));
                assert_eq!(find_nth_tag(html, "p", 1, true), Some(20));
                assert_eq!(find_nth_tag(html, "p", 2, true), None);
                assert_eq!(find_nth_tag(html, "p", 0, false), Some(4));
                assert_eq!(find_nth_tag(html, "p", 1, false), Some(24));
                assert_eq!(find_nth_tag(html, "p", 2, false), None);
                assert_eq!(find_nth_tag(html, "div", 0, true), Some(8));
                assert_eq!(find_nth_tag(html, "div", 1, true), None);
                assert_eq!(find_nth_tag(html, "div", 0, false), Some(14));
                assert_eq!(find_nth_tag(html, "div", 1, false), None);
            }

            // Verify offsets point to correct < and substring matches
            #[test]
            fn offset_points_to_correct_bytes() {
                let html = "hello<div>world<div>!";
                let off0 = find_nth_tag(html, "div", 0, true).unwrap();
                let off1 = find_nth_tag(html, "div", 1, true).unwrap();
                assert_eq!(&html.as_bytes()[off0..off0 + 4], b"<div");
                assert_eq!(&html.as_bytes()[off1..off1 + 4], b"<div");
                assert_ne!(off0, off1);
            }
        }

        mod edge_cases {
            use super::*;

            // Already covered: tag_at_eof_no_close, match_nth (n beyond count)

            // Empty input
            #[test]
            fn empty_input() {
                assert_eq!(find_nth_tag("", "div", 0, true), None);
            }

            // Empty tag <> — InTag exits immediately on >
            #[test]
            fn empty_angle_brackets() {
                assert_eq!(find_nth_tag("<><div>", "div", 0, true), Some(2));
            }

            // Single byte, no match
            #[test]
            fn single_byte_no_match() {
                assert_eq!(find_nth_tag("x", "div", 0, true), None);
            }

            // Single byte, just
            #[test]
            fn single_byte_open_angle() {
                assert_eq!(find_nth_tag("<", "div", 0, true), None);
            }

            // Pattern longer than input
            #[test]
            fn pattern_longer_than_input() {
                assert_eq!(find_nth_tag("<d", "div", 0, true), None);
            }

            // Exact match fills entire input (no boundary byte, EOF boundary)
            #[test]
            fn pattern_exact_end_of_input_start() {
                assert_eq!(find_nth_tag("<div", "div", 0, true), Some(0));
            }

            #[test]
            fn pattern_exact_end_of_input_close() {
                assert_eq!(find_nth_tag("</div", "div", 0, false), Some(0));
            }

            // Tag at nonzero offset, flush to end
            #[test]
            fn tag_at_end_of_input() {
                assert_eq!(find_nth_tag("hello<div", "div", 0, true), Some(5));
            }

            // n = 0 with exactly one match
            #[test]
            fn n_zero_single_match() {
                assert_eq!(find_nth_tag("<div>", "div", 0, true), Some(0));
            }

            // n = 1 with exactly one match
            #[test]
            fn n_beyond_single_match() {
                assert_eq!(find_nth_tag("<div>", "div", 1, true), None);
            }

            // Large n
            #[test]
            fn n_large() {
                assert_eq!(find_nth_tag("<div><div><div>", "div", 100, true), None);
            }

            // Single-char tag name
            #[test]
            fn single_char_tag() {
                assert_eq!(find_nth_tag("<b>bold</b>", "b", 0, true), Some(0));
                assert_eq!(find_nth_tag("<b>bold</b>", "b", 0, false), Some(7));
            }

            // Incomplete comment at EOF
            #[test]
            fn incomplete_comment_eof() {
                assert_eq!(find_nth_tag("<!--<div>", "div", 0, true), None);
            }

            // Incomplete close sequence in comment at EOF
            #[test]
            fn comment_partial_close_eof() {
                assert_eq!(find_nth_tag("<!-- --<div>", "div", 0, true), None);
            }

            // Tag immediately after comment close
            #[test]
            fn tag_immediately_after_comment() {
                assert_eq!(find_nth_tag("<!--x--><div>", "div", 0, true), Some(8));
            }

            // Quote never closed — stays in InQuote until EOF
            #[test]
            fn unclosed_quote_eof() {
                assert_eq!(find_nth_tag(r#"<a href="<div>"#, "div", 0, true), None);
            }

            // Tag never closed — stays in InTag until EOF
            #[test]
            fn unclosed_tag_eof() {
                assert_eq!(find_nth_tag("<span foo bar", "div", 0, true), None);
            }

            // Adjacent comments — does state recover correctly between them?
            #[test]
            fn adjacent_comments() {
                assert_eq!(find_nth_tag("<!--a--><!--b--><div>", "div", 0, true), Some(16));
            }

            // Comment-like but not quite: <!- is not <!--, should enter InTag
            #[test]
            fn fake_comment_prefix() {
                assert_eq!(find_nth_tag("<!-<div>", "div", 0, true), Some(3));
            }

            // <!-- check: does i+4 boundary work when <!-- is at exact end?
            #[test]
            fn comment_open_at_eof() {
                assert_eq!(find_nth_tag("<!--", "div", 0, true), None);
            }

            // Less than 4 bytes remaining when < encountered — the i+4<=len check
            #[test]
            fn angle_bracket_near_eof_not_comment() {
                assert_eq!(find_nth_tag("<!-", "div", 0, true), None);
            }

            // --> appearing outside a comment (in Normal state) — should not affect anything
            #[test]
            fn close_comment_in_normal() {
                assert_eq!(find_nth_tag("--><div>", "div", 0, true), Some(3));
            }

            // Tag match immediately after > (no gap)
            #[test]
            fn match_immediately_after_close() {
                assert_eq!(find_nth_tag("<span><div>", "div", 0, true), Some(6));
            }

            // Multiple quotes in attributes — alternating " and '
            #[test]
            fn mixed_quotes_in_attrs() {
                assert_eq!(find_nth_tag(r#"<a x="'" y='"'><div>"#, "div", 0, true), Some(15));
            }

            // Double quote inside single-quoted attr (should not exit quote)
            #[test]
            fn double_quote_inside_single_quote() {
                assert_eq!(find_nth_tag(r#"<a x='"<div>"'><div>"#, "div", 0, true), Some(15));
            }

            // Self-closing with space before / — boundary is space
            #[test]
            fn self_closing_with_space() {
                assert_eq!(find_nth_tag("<div />", "div", 0, true), Some(0));
            }

            // Only whitespace between < and tag name — should NOT match
            // because pattern is `<div`, not `< div`
            #[test]
            fn space_after_angle_bracket() {
                assert_eq!(find_nth_tag("< div>", "div", 0, true), None);
            }

            // Null bytes in input
            #[test]
            fn null_bytes_in_input() {
                assert_eq!(find_nth_tag("<div\0>", "div", 0, true), None);
            }

            // Tag inside emoji-heavy content — offsets are byte offsets, not char offsets
            #[test]
            fn tag_after_emoji() {
                let html = "😀😀<div>";
                // 😀 is 4 bytes each
                assert_eq!(find_nth_tag(html, "div", 0, true), Some(8));
            }

            // Multi-byte in attribute value
            #[test]
            fn multibyte_in_attr() {
                assert_eq!(find_nth_tag(r#"<a title="日本語"><div>"#, "div", 0, true), Some(21));
            }

            // Multi-byte between tags
            #[test]
            fn multibyte_between_tags() {
                let html = "<div>λόγος</div>";
                assert_eq!(find_nth_tag(html, "div", 0, true), Some(0));
                assert_eq!(find_nth_tag(html, "div", 0, false), Some(15));
            }

            // Known limitation: scanner doesn't understand <script> content model
            #[test]
            fn script_tag_contents_not_distinguished() {
                let html = "<script>var x = '<div>';</script><div>real</div>";
                assert_eq!(find_nth_tag(html, "div", 0, true), Some(17));
            }

            // Known limitation: CDATA not recognized
            #[test]
            fn cdata_not_recognized() {
                let html = "<![CDATA[<div>]]><div>real</div>";
                assert_eq!(find_nth_tag(html, "div", 0, true), Some(9)); // false positive
            }

            // Processing instruction — ? is just a byte in InTag
            #[test]
            fn processing_instruction() {
                assert_eq!(find_nth_tag("<?xml version=\"1.0\"?><div>", "div", 0, true), Some(21));
            }
        }
    }

    mod process_token_tests {
        use super::*;
        use html5ever::tokenizer::{BufferQueue, Tokenizer, TokenizerOpts};

        struct TokenizeResult {
            errors: Vec<(Option<usize>, String)>,
            warnings: Vec<(Option<usize>, String)>,
            tag_events: Vec<(String, bool, usize)>,
        }

        fn tokenize(html: &str, node_offset: usize, exact_errors: bool) -> TokenizeResult {
            let sink = HtmlTokenSink::new(html, node_offset);
            let tokenizer = Tokenizer::new(
                sink,
                TokenizerOpts {
                    exact_errors,
                    ..Default::default()
                },
            );
            let input = BufferQueue::default();
            input.push_back(html.into());
            let _ = tokenizer.feed(&input);
            tokenizer.end();

            let sink = &tokenizer.sink;
            TokenizeResult {
                errors: sink.errors.borrow().clone(),
                warnings: sink.warnings.borrow().clone(),
                tag_events: sink
                    .tag_events
                    .borrow()
                    .iter()
                    .map(|e| (e.tag.clone(), e.is_open, e.offset))
                    .collect(),
            }
        }

        mod start_tag_non_void {
            use super::*;

            #[test]
            fn simple_div() {
                let r = tokenize("<div>", 0, false);
                assert_eq!(r.tag_events, vec![("div".into(), true, 0)]);
                assert!(r.errors.is_empty());
            }

            #[test]
            fn with_attributes() {
                let r = tokenize(r#"<div class="x" id="y">"#, 0, false);
                assert_eq!(r.tag_events, vec![("div".into(), true, 0)]);
            }

            #[test]
            fn multiple_non_void() {
                let r = tokenize("<div><span><p>", 0, false);
                assert_eq!(
                    r.tag_events,
                    vec![
                        ("div".into(), true, 0),
                        ("span".into(), true, 5),
                        ("p".into(), true, 11),
                    ]
                );
            }
        }

        mod start_tag_void {
            use super::*;

            #[test]
            fn br_no_event() {
                let r = tokenize("<br>", 0, false);
                assert!(r.tag_events.is_empty());
                assert!(r.errors.is_empty());
            }

            #[test]
            fn hr_no_event() {
                let r = tokenize("<hr>", 0, false);
                assert!(r.tag_events.is_empty());
            }

            #[test]
            fn img_with_attrs_no_event() {
                let r = tokenize(r#"<img src="x.png" alt="y">"#, 0, false);
                assert!(r.tag_events.is_empty());
            }

            #[test]
            fn void_among_non_void() {
                let r = tokenize("<div><br><span>", 0, false);
                assert_eq!(r.tag_events, vec![("div".into(), true, 0), ("span".into(), true, 9),]);
            }
        }

        mod start_tag_void_self_closing {
            use super::*;

            #[test]
            fn br_self_closing_no_error() {
                let r = tokenize("<br/>", 0, false);
                assert!(r.tag_events.is_empty());
                // void self-closing is valid, no error
                assert!(!r.errors.iter().any(|(_, msg)| msg.contains("non-void")));
            }

            #[test]
            fn img_self_closing_no_error() {
                let r = tokenize(r#"<img src="x.png"/>"#, 0, false);
                assert!(r.tag_events.is_empty());
                assert!(!r.errors.iter().any(|(_, msg)| msg.contains("non-void")));
            }
        }

        mod start_tag_non_void_self_closing {
            use super::*;

            #[test]
            fn div_self_closing_error() {
                let r = tokenize("<div/>", 0, false);
                assert!(r.tag_events.is_empty()); // no event pushed
                assert!(
                    r.errors
                        .iter()
                        .any(|(off, msg)| { off == &Some(0) && msg.contains("non-void") })
                );
            }

            #[test]
            fn span_self_closing_error() {
                let r = tokenize("<span/>", 0, false);
                assert!(
                    r.errors
                        .iter()
                        .any(|(off, msg)| { off == &Some(0) && msg.contains("non-void") && msg.contains("span") })
                );
            }

            #[test]
            fn with_offset() {
                let r = tokenize("<span/>", 100, false);
                assert!(r.errors.iter().any(|(off, _)| off == &Some(100)));
            }
        }

        mod end_tag {
            use super::*;

            #[test]
            fn simple_close() {
                let r = tokenize("</div>", 0, false);
                assert_eq!(r.tag_events, vec![("div".into(), false, 0)]);
            }

            #[test]
            fn open_and_close() {
                let r = tokenize("<div></div>", 0, false);
                assert_eq!(r.tag_events, vec![("div".into(), true, 0), ("div".into(), false, 5),]);
            }

            #[test]
            fn close_void_element() {
                // html5ever emits end tags even for void elements
                let r = tokenize("</br>", 0, false);
                assert_eq!(r.tag_events, vec![("br".into(), false, 0)]);
            }
        }

        mod duplicate_attrs {
            use super::*;

            #[test]
            fn duplicate_attr_warning() {
                let r = tokenize(r#"<div class="a" class="b">"#, 0, false);
                assert!(
                    r.warnings
                        .iter()
                        .any(|(off, msg)| { off == &Some(0) && msg.contains("duplicate") })
                );
            }

            #[test]
            fn no_duplicate_no_warning() {
                let r = tokenize(r#"<div class="a" id="b">"#, 0, false);
                assert!(r.warnings.is_empty());
            }
        }

        mod duplicate_attrs_and_self_closing {
            use super::*;

            #[test]
            fn both_warning_and_error() {
                let r = tokenize(r#"<div class="a" class="b"/>"#, 0, false);
                assert!(r.warnings.iter().any(|(_, msg)| msg.contains("duplicate")));
                assert!(r.errors.iter().any(|(_, msg)| msg.contains("non-void")));
            }
        }

        mod nth_occurrence {
            use super::*;

            #[test]
            fn two_divs_different_offsets() {
                let r = tokenize("<div></div><div></div>", 0, false);
                assert_eq!(
                    r.tag_events,
                    vec![
                        ("div".into(), true, 0),
                        ("div".into(), false, 5),
                        ("div".into(), true, 11),
                        ("div".into(), false, 16),
                    ]
                );
            }

            #[test]
            fn interleaved_tags() {
                let r = tokenize("<div><span></span></div>", 0, false);
                assert_eq!(
                    r.tag_events,
                    vec![
                        ("div".into(), true, 0),
                        ("span".into(), true, 5),
                        ("span".into(), false, 11),
                        ("div".into(), false, 18),
                    ]
                );
            }
        }

        mod offset_calculation {
            use super::*;

            #[test]
            fn nonzero_offset_added() {
                let r = tokenize("<div></div>", 50, false);
                assert_eq!(r.tag_events, vec![("div".into(), true, 50), ("div".into(), false, 55),]);
            }

            #[test]
            fn offset_on_warning() {
                let r = tokenize(r#"<div id="a" id="b">"#, 200, false);
                assert!(r.warnings.iter().any(|(off, _)| off == &Some(200)));
            }

            #[test]
            fn offset_on_non_void_self_closing_error() {
                let r = tokenize("<p/>", 300, false);
                assert!(r.errors.iter().any(|(off, _)| off == &Some(300)));
            }
        }

        mod character_and_other_tokens {
            use super::*;

            #[test]
            fn plain_text_no_events() {
                let r = tokenize("hello world", 0, false);
                assert!(r.tag_events.is_empty());
                assert!(r.warnings.is_empty());
            }

            #[test]
            fn comment_no_events() {
                let r = tokenize("<!-- comment -->", 0, false);
                assert!(r.tag_events.is_empty());
            }

            #[test]
            fn text_between_tags() {
                let r = tokenize("<div>hello</div>", 0, false);
                assert_eq!(r.tag_events.len(), 2);
                assert!(r.warnings.is_empty());
            }
        }

        mod case_sensitivity {
            use super::*;

            #[test]
            fn uppercase_tag_resolved() {
                let r = tokenize("<DIV></DIV>", 0, false);
                // html5ever lowercases tag names
                assert_eq!(r.tag_events, vec![("div".into(), true, 0), ("div".into(), false, 5),]);
            }

            #[test]
            fn mixed_case() {
                let r = tokenize("<Div></dIV>", 0, false);
                assert_eq!(r.tag_events, vec![("div".into(), true, 0), ("div".into(), false, 5),]);
            }
        }

        mod break_process_token {
            use super::*;

            // ── Wrong offset: tag name appears in unquoted attribute ─────
            // find_nth_tag handles quoted attrs but unquoted attr values
            // are just bytes in InTag state — should be fine. But what if
            // the > inside unquoted attr terminates the tag early?

            #[test]
            fn tag_name_in_unquoted_attr() {
                // <div data=div> — "div" in attr value is inside InTag,
                // not matched because scanner only matches after < in Normal
                let r = tokenize(r"<div data=div><div>", 0, false);
                assert_eq!(r.tag_events, vec![("div".into(), true, 0), ("div".into(), true, 14),]);
            }

            // ── Wrong offset: html5ever auto-closes tags ────────────────
            // <p><p> — html5ever implicitly closes first <p> before second.
            // It synthesizes a </p> that doesn't exist in source.
            // find_nth_tag searches for </p> with n=0, finds nothing → fallback.

            #[test]
            fn implicit_close_p_in_p() {
                let r = tokenize("<p>first<p>second", 0, false);
                // html5ever may emit: <p>, </p>(implicit), <p>
                // The implicit </p> can't be found → offset = node_offset (0)
                // This means the close event has a WRONG offset (0 instead of nonexistent)
                // Verify the events exist and check what offset we get
                let close_events: Vec<_> = r
                    .tag_events
                    .iter()
                    .filter(|(tag, is_open, _)| tag == "p" && !is_open)
                    .collect();
                if !close_events.is_empty() {
                    // implicit close tag → fallback offset = node_offset
                    // This is silently wrong — no error reported
                    assert_eq!(close_events[0].2, 0, "implicit close falls back to node_offset");
                }
            }

            // ── Wrong offset: html5ever synthesizes <html><head><body> ───
            // These tags don't exist in source at all.

            #[test]
            fn synthesized_tags_not_in_source() {
                // NOTE: html5ever tokenizer (not tree builder) should NOT
                // synthesize tags. But if it does, offsets are wrong.
                // This test documents the assumption.
                let r = tokenize("<div>", 0, false);
                let tags: Vec<_> = r.tag_events.iter().map(|(tag, _, _)| tag.as_str()).collect();
                // Tokenizer should only emit what's in source
                assert_eq!(tags, vec!["div"]);
            }

            // ── Count desync: end tag before any start tag ──────────────
            // end_counts starts at 0, finds nth=0 close tag correctly.
            // But what if source has more end tags than start tags?

            #[test]
            fn more_end_tags_than_start_tags() {
                let r = tokenize("</div></div></div>", 0, false);
                assert_eq!(
                    r.tag_events,
                    vec![
                        ("div".into(), false, 0),
                        ("div".into(), false, 6),
                        ("div".into(), false, 12),
                    ]
                );
            }

            // ── Count desync: interleaved same-name open/close ──────────
            // start_counts and end_counts are independent.
            // <div></div><div></div> should give start n=0,1 and end n=0,1

            #[test]
            fn interleaved_same_name_counts() {
                let r = tokenize("<div></div><div></div>", 0, false);
                assert_eq!(
                    r.tag_events,
                    vec![
                        ("div".into(), true, 0),
                        ("div".into(), false, 5),
                        ("div".into(), true, 11),
                        ("div".into(), false, 16),
                    ]
                );
            }

            // ── Offset bomb: tag name appears in text content before tag ─
            // "div<div>" — find_nth_tag should NOT match bare "div" text
            // because it requires leading <. Safe. But what about:
            // "<div>text with <br> and <div>"
            // nth=1 start for div should be 24, not confused by <br>

            #[test]
            fn tag_name_in_text_content() {
                let r = tokenize("div <div>", 0, false);
                assert_eq!(r.tag_events, vec![("div".into(), true, 4)]);
            }

            #[test]
            fn empty_input() {
                let r = tokenize("", 0, false);
                assert!(r.tag_events.is_empty());
                assert!(r.errors.is_empty());
                assert!(r.warnings.is_empty());
            }

            #[test]
            fn whitespace_only() {
                let r = tokenize("   \n\t  ", 0, false);
                assert!(r.tag_events.is_empty());
            }

            // ── Wrong offset: attribute contains < (unquoted) ───────────
            // Unquoted < in attribute is malformed HTML. html5ever tokenizer
            // may interpret this differently than find_nth_tag's scanner.

            #[test]
            fn unquoted_lt_in_attribute() {
                // html5ever and find_nth_tag may disagree on tag boundaries
                let html = "<div data=<span>>";
                let r = tokenize(html, 0, false);
                // Whatever events come out, offsets should be valid
                // (within bounds of html)
                for (_, _, off) in &r.tag_events {
                    assert!(*off < html.len(), "offset {off} out of bounds");
                }
            }
        }
    }
}
