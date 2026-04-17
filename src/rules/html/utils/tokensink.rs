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
#[path = "tokensink/find_nth_tag_tests.rs"]
mod find_nth_tag_tests;

#[cfg(test)]
#[path = "tokensink/process_token_tests.rs"]
mod process_token_tests;
