use std::collections::HashMap;

use html5ever::tokenizer::{BufferQueue, Tokenizer, TokenizerOpts};
use markdown_it::common::sourcemap::SourcePos;
use markdown_it::common::sourcemap::SourceWithLineStarts;
use markdown_it::parser::core::CoreRule;
use markdown_it::plugins::html::html_block::HtmlBlock;
use markdown_it::plugins::html::html_inline::HtmlInline;
use markdown_it::{MarkdownIt, Node};

use super::utils::tokensink;

pub fn add(md: &mut MarkdownIt) {
    md.add_rule::<HtmlValidatorRule>();
}

/// Whether the byte offset points to the exact problem location
/// or only to the start of the containing HTML node.
#[derive(Debug, Clone, Copy)]
pub enum ErrorOffset {
    /// Precise byte offset (e.g. resolved from tag matching via `find_nth_tag`).
    Exact(usize),
    /// Coarse offset — only the start of the HTML node that contains the error.
    /// Column information is not meaningful.
    NodeStart(usize),
}

impl ErrorOffset {
    fn value(&self) -> usize {
        match self {
            ErrorOffset::Exact(o) | ErrorOffset::NodeStart(o) => *o,
        }
    }

    fn from_option(offset: Option<usize>, fallback: usize) -> Self {
        match offset {
            Some(o) => ErrorOffset::Exact(o),
            None => ErrorOffset::NodeStart(fallback),
        }
    }
}

#[derive(Debug)]
pub struct HtmlError {
    pub offset: ErrorOffset,
    pub message: String,
    pub severity: Severity,
    pub related: Vec<RelatedInfo>,
}

#[derive(Debug)]
pub struct RelatedInfo {
    pub offset: ErrorOffset,
    pub message: String,
}

#[derive(Debug)]
pub enum Severity {
    Error,
    Warning,
}

pub struct HtmlValidatorRule;

impl CoreRule for HtmlValidatorRule {
    fn run(root: &mut Node, _md: &MarkdownIt) {
        crate::debug_write("01-ast.txt", &format!("{root:#?}"));

        let source = root
            .cast::<markdown_it::parser::core::Root>()
            .expect("root node")
            .content
            .clone();
        let source_map = SourceWithLineStarts::new(&source);

        eprintln!("\n\x1b[36m=== HtmlValidatorRule::run START ===\x1b[0m");
        eprintln!("  source length: {} bytes", source.len());

        let errors = validate(root);

        eprintln!("\n\x1b[36m--- final errors ({}) ---\x1b[0m", errors.len());
        for error in &errors {
            let o = error.offset.value();
            let sp = SourcePos::new(o, o + 1);
            let ((line, col), _) = sp.get_positions(&source_map);
            let location = match error.offset {
                ErrorOffset::Exact(_) => format!("{line}:{col}"),
                ErrorOffset::NodeStart(_) => format!("{line}:?"),
            };
            match error.severity {
                Severity::Error => {
                    eprintln!("\x1b[31mERROR   -\x1b[0m  {location} {}", error.message);
                }
                Severity::Warning => {
                    eprintln!("\x1b[33mWARNING -\x1b[0m  {location} {}", error.message);
                }
            }
            for rel in &error.related {
                let ro = rel.offset.value();
                let rsp = SourcePos::new(ro, ro + 1);
                let ((rline, rcol), _) = rsp.get_positions(&source_map);
                let rlocation = match rel.offset {
                    ErrorOffset::Exact(_) => format!("{rline}:{rcol}"),
                    ErrorOffset::NodeStart(_) => format!("{rline}:?"),
                };
                eprintln!("              └─ {rlocation} {}", rel.message);
            }
        }
        eprintln!("\x1b[36m=== HtmlValidatorRule::run END ===\x1b[0m\n");

        crate::debug_write("02-ast-html-validator.txt", &format!("{root:#?}"));
    }
}

pub fn validate(root: &mut Node) -> Vec<HtmlError> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut tag_events = Vec::new();

    // single walk: tokenizer parse errors, duplicate attrs, tag events, and HTML attr extraction
    root.walk(|node, _| {
        // Clone content to release borrow on node before writing to node.ext
        let Some((node_html, node_offset)) = extract_html_content_owned(node) else {
            return;
        };

        eprintln!("\n\x1b[36m--- walk: found HTML node at node_offset {node_offset} ---\x1b[0m");
        eprintln!("  node_html: {node_html:?}");

        let result = tokenize(node_html, node_offset);

        errors.extend(result.errors);
        warnings.extend(result.warnings);
        tag_events.extend(result.tag_events);
    });

    eprintln!(
        "\n\x1b[36m--- after walk: {} errors, {} warnings, {} tag_events ---\x1b[0m",
        errors.len(),
        warnings.len(),
        tag_events.len()
    );
    for tag_event in &tag_events {
        eprintln!(
            "  tag_event: {} <{}> at offset {}",
            if tag_event.is_open { "OPEN" } else { "CLOSE" },
            tag_event.tag,
            tag_event.offset
        );
    }

    // cross-node: tag balance (uses already-collected events)
    tag_balance(&tag_events, &mut errors);

    errors.extend(warnings);

    errors.sort_by_key(|e| match e.offset {
        ErrorOffset::Exact(o) => (0, o),
        ErrorOffset::NodeStart(o) => (1, o),
    });

    errors
}

/// Like `extract_html_content` but clones the content string, releasing the
/// borrow on `node` so the caller can subsequently write to `node.ext`.
fn extract_html_content_owned(node: &Node) -> Option<(&str, usize)> {
    let byte_offset_start = node.srcmap.as_ref().map_or(0, |sp| sp.get_byte_offsets().0);

    #[allow(clippy::collapsible_if)]
    if let Some(hb) = node.cast::<HtmlBlock>() {
        if !hb.content.trim().is_empty() {
            eprintln!(
                "  extract_html_content_owned: HtmlBlock at offset {byte_offset_start}, content len={}",
                hb.content.len()
            );
            return Some((&hb.content, byte_offset_start));
        }
    }

    #[allow(clippy::collapsible_if)]
    if let Some(hi) = node.cast::<HtmlInline>() {
        if !hi.content.trim().is_empty() {
            eprintln!(
                "  extract_html_content_owned: HtmlInline at offset {byte_offset_start}, content len={}",
                hi.content.len()
            );
            return Some((&hi.content, byte_offset_start));
        }
    }

    None
}

struct TokenizeResult {
    errors: Vec<HtmlError>,
    warnings: Vec<HtmlError>,
    tag_events: Vec<tokensink::TagEvent>,
}

fn tokenize(node_html: &str, node_offset: usize) -> TokenizeResult {
    eprintln!("  tokenize: node_offset={node_offset} node_html={node_html:?}");

    let tokenizer = Tokenizer::new(
        tokensink::HtmlTokenSink::new(node_html, node_offset),
        TokenizerOpts {
            exact_errors: false,
            ..Default::default()
        },
    );

    let input = BufferQueue::default();
    input.push_back(node_html.into());
    let _ = tokenizer.feed(&input);
    tokenizer.end();

    let sink = tokenizer.sink;

    eprintln!(
        "  tokenize result:\n\
         \x20   node_html={:?}\n\
         \x20   node_offset={}\n\
         \x20   errors={}\n\
         \x20   warnings={}\n\
         \x20   tag_events={}",
        sink.node_html,
        sink.node_offset,
        sink.errors.borrow().len(),
        sink.warnings.borrow().len(),
        sink.tag_events.borrow().len(),
    );

    let errors = sink
        .errors
        .into_inner()
        .into_iter()
        .map(|(offset, msg)| HtmlError {
            offset: ErrorOffset::from_option(offset, node_offset),
            message: msg,
            severity: Severity::Error,
            related: vec![],
        })
        .collect();

    let warnings = sink
        .warnings
        .into_inner()
        .into_iter()
        .map(|(offset, msg)| HtmlError {
            offset: ErrorOffset::from_option(offset, node_offset),
            message: msg,
            severity: Severity::Warning,
            related: vec![],
        })
        .collect();

    TokenizeResult {
        errors,
        warnings,
        tag_events: sink.tag_events.into_inner(),
    }
}

/// Check tag balance across all collected tag events.
///
/// Find the nearest plausible match and assume the intervening tags were never closed
///
/// This enforces strict open/close matching — HTML's implicit close
/// rules (e.g. `<p>` closing a previous `<p>`) are not implemented.
/// This is intentional: markdown-embedded HTML should use explicit
/// closing tags for clarity.
///
///
/// events : <tag, kind, offset>[]
/// errors = []
/// stack = []
/// evicted : multiset = {}
///
/// for event in events:
///     if event.kind == open:                                // opening event.tag
///         stack.push(event)
///
///     else:                                                 // closing event.tag
///         match = stack.find_last(e => e.tag == event.tag)  // search for matching opening from the top of the stack
///
///         if !match:                                        // current closing has no matching opening
///             if evicted.count(event.tag) > 0:
///                 evicted.remove_one(event.tag)
///             else:
///                 errors.append("closing </{event.tag}> at {event.offset} has no opener")
///
///         else:                                             // current closing has a matching opening
///             while stack.top != match:
///                 evicted.add(stack.top.tag)
///                 errors.append("misnested <{stack.top.tag}> at {stack.top.offset}, forced by </{event.tag}> at {event.offset}")
///                 stack.pop
///             stack.pop                                     // consume match
///
/// for event in stack:
///     errors.append("unclosed <{event.tag}> at {event.offset}")
///
fn tag_balance(tag_events: &[tokensink::TagEvent], errors: &mut Vec<HtmlError>) {
    debug_assert!(
        tag_events.iter().all(|e| !tokensink::is_void(&e.tag)),
        "void element leaked into tag_events"
    );

    let mut stack: Vec<&tokensink::TagEvent> = Vec::new();
    let mut evicted: HashMap<&str, usize> = HashMap::new();

    for event in tag_events {
        if event.is_open {
            stack.push(event);
        } else {
            let match_pos = stack.iter().rposition(|e| e.tag == event.tag);

            match match_pos {
                None => {
                    let count = evicted.get_mut(event.tag.as_str());
                    if let Some(c) = count.filter(|c| **c > 0) {
                        *c -= 1;
                    } else {
                        errors.push(HtmlError {
                            offset: ErrorOffset::Exact(event.offset),
                            message: format!("closing </{}> has no matching opener", event.tag),
                            severity: Severity::Error,
                            related: vec![],
                        });
                    }
                }
                Some(pos) => {
                    while stack.len() > pos + 1 {
                        let top = stack.pop().unwrap();
                        *evicted.entry(&top.tag).or_insert(0) += 1;
                        errors.push(HtmlError {
                            offset: ErrorOffset::Exact(top.offset),
                            message: format!("misnested <{}>", top.tag),
                            severity: Severity::Error,
                            related: vec![RelatedInfo {
                                offset: ErrorOffset::Exact(event.offset),
                                message: format!("forced by </{}>", event.tag),
                            }],
                        });
                    }
                    stack.pop();
                }
            }
        }
    }

    for event in &stack {
        errors.push(HtmlError {
            offset: ErrorOffset::Exact(event.offset),
            message: format!("unclosed <{}>", event.tag),
            severity: Severity::Error,
            related: vec![],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod tag_balance_tests {
        use super::*;
        use crate::rules::html::utils::tokensink::TagEvent;

        /// Helper: build an open tag event.
        fn open(tag: &str, offset: usize) -> TagEvent {
            TagEvent {
                tag: tag.to_string(),
                is_open: true,
                offset,
            }
        }

        /// Helper: build a close tag event.
        fn close(tag: &str, offset: usize) -> TagEvent {
            TagEvent {
                tag: tag.to_string(),
                is_open: false,
                offset,
            }
        }

        /// Run `tag_balance` and return errors.
        fn run(events: &[TagEvent]) -> Vec<HtmlError> {
            let mut errors = Vec::new();
            tag_balance(events, &mut errors);
            errors
        }

        /// Assert error messages match exactly.
        fn assert_messages(errors: &[HtmlError], expected: &[&str]) {
            let msgs: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
            assert_eq!(msgs, expected, "error messages mismatch");
        }

        /// Assert error offsets match exactly.
        fn assert_offsets(errors: &[HtmlError], expected: &[usize]) {
            let offsets: Vec<usize> = errors.iter().map(|e| e.offset.value()).collect();
            assert_eq!(offsets, expected, "error offsets mismatch");
        }

        mod valid_nesting {
            use super::*;

            #[test]
            fn empty_input() {
                let errors = run(&[]);
                assert!(errors.is_empty());
            }

            #[test]
            fn single_pair() {
                let errors = run(&[open("div", 0), close("div", 10)]);
                assert!(errors.is_empty());
            }

            #[test]
            fn nested_pairs() {
                let errors = run(&[open("div", 0), open("span", 5), close("span", 10), close("div", 15)]);
                assert!(errors.is_empty());
            }

            #[test]
            fn sequential_siblings() {
                let errors = run(&[open("p", 0), close("p", 5), open("p", 10), close("p", 15)]);
                assert!(errors.is_empty());
            }

            #[test]
            fn deep_nesting() {
                let errors = run(&[
                    open("div", 0),
                    open("ul", 5),
                    open("li", 10),
                    open("span", 15),
                    close("span", 20),
                    close("li", 25),
                    close("ul", 30),
                    close("div", 35),
                ]);
                assert!(errors.is_empty());
            }
        }

        mod unclosed_tags {
            use super::*;

            #[test]
            fn single_unclosed() {
                let errors = run(&[open("div", 0)]);
                assert_messages(&errors, &["unclosed <div>"]);
                assert_offsets(&errors, &[0]);
            }

            #[test]
            fn multiple_unclosed_same_tag() {
                let errors = run(&[open("div", 0), open("div", 10)]);
                assert_eq!(errors.len(), 2);
                assert!(errors.iter().all(|e| e.message == "unclosed <div>"));
            }

            #[test]
            fn multiple_unclosed_different_tags() {
                let errors = run(&[open("div", 0), open("span", 10)]);
                assert_eq!(errors.len(), 2);
                assert_messages(&errors, &["unclosed <div>", "unclosed <span>"]);
            }

            #[test]
            fn unclosed_at_depth() {
                // <div><span></div> — span unclosed, but that's misnesting.
                // Here: <div><span> — both unclosed, no closers at all.
                let errors = run(&[open("div", 0), open("span", 5)]);
                assert_eq!(errors.len(), 2);
            }
        }

        mod orphan_closers {
            use super::*;

            #[test]
            fn single_orphan() {
                let errors = run(&[close("div", 0)]);
                assert_messages(&errors, &["closing </div> has no matching opener"]);
                assert_offsets(&errors, &[0]);
            }

            #[test]
            fn multiple_orphans_same_tag() {
                let errors = run(&[close("div", 0), close("div", 10)]);
                assert_eq!(errors.len(), 2);
                assert!(
                    errors
                        .iter()
                        .all(|e| e.message == "closing </div> has no matching opener")
                );
            }

            #[test]
            fn multiple_orphans_different_tags() {
                let errors = run(&[close("div", 0), close("span", 10)]);
                assert_eq!(errors.len(), 2);
            }

            #[test]
            fn orphan_before_valid_pair() {
                let errors = run(&[close("span", 0), open("div", 5), close("div", 10)]);
                assert_eq!(errors.len(), 1);
                assert_messages(&errors, &["closing </span> has no matching opener"]);
            }
        }

        mod misnesting {
            use super::*;

            #[test]
            fn classic_ab_overlap() {
                // <a><b></a></b>
                let errors = run(&[open("a", 0), open("b", 5), close("a", 10), close("b", 15)]);
                // <b> at 5 is misnested (forced by </a> at 10)
                // </b> at 15 consumes eviction credit — no error
                assert_eq!(errors.len(), 1);
                assert_messages(&errors, &["misnested <b>"]);
                assert_offsets(&errors, &[5]);
            }

            #[test]
            fn triple_overlap() {
                // <a><b><c></a></c></b>
                let errors = run(&[
                    open("a", 0),
                    open("b", 5),
                    open("c", 10),
                    close("a", 15),
                    close("c", 20),
                    close("b", 25),
                ]);
                // </a> at 15 evicts <c> at 10 and <b> at 5
                // </c> at 20 and </b> at 25 consume eviction credits
                assert_eq!(errors.len(), 2);
                assert_messages(&errors, &["misnested <c>", "misnested <b>"]);
            }

            #[test]
            fn evicted_closer_consumed_silently() {
                // <a><b></a></b> — </b> should not produce an error
                let errors = run(&[open("a", 0), open("b", 5), close("a", 10), close("b", 15)]);
                assert_eq!(errors.len(), 1); // only the misnesting error
                assert!(
                    errors
                        .iter()
                        .all(|e| e.message != "closing </b> has no matching opener")
                );
            }

            #[test]
            fn evicted_closer_missing() {
                // <a><b></a> — <b> evicted, no </b> ever appears
                // eviction credit for "b" remains unused; <b> already reported as misnested
                let errors = run(&[open("a", 0), open("b", 5), close("a", 10)]);
                assert_eq!(errors.len(), 1);
                assert_messages(&errors, &["misnested <b>"]);
            }

            #[test]
            fn multiple_evictions_from_single_closer() {
                // <a><b><c></a>
                // </a> at 15 evicts both <c> and <b>
                let errors = run(&[open("a", 0), open("b", 5), open("c", 10), close("a", 15)]);
                assert_eq!(errors.len(), 2);
                assert_messages(&errors, &["misnested <c>", "misnested <b>"]);
            }

            #[test]
            fn misnesting_at_depth() {
                // <div><a><b></a></b></div>
                let errors = run(&[
                    open("div", 0),
                    open("a", 5),
                    open("b", 10),
                    close("a", 15),
                    close("b", 20),
                    close("div", 25),
                ]);
                assert_eq!(errors.len(), 1);
                assert_messages(&errors, &["misnested <b>"]);
            }
        }

        mod eviction_credits {
            use super::*;

            #[test]
            fn more_closers_than_credits() {
                // <a><b></a></b></b>
                // eviction gives 1 credit for "b", first </b> consumes it, second </b> is orphan
                let errors = run(&[
                    open("a", 0),
                    open("b", 5),
                    close("a", 10),
                    close("b", 15),
                    close("b", 20),
                ]);
                let orphan = errors
                    .iter()
                    .find(|e| e.message == "closing </b> has no matching opener");
                assert!(orphan.is_some());
                assert_eq!(orphan.unwrap().offset.value(), 20);
            }

            #[test]
            fn exact_credit_count() {
                // <a><b><b></a></b></b>
                // two <b> evicted, two </b> consume credits — no orphan errors
                let errors = run(&[
                    open("a", 0),
                    open("b", 5),
                    open("b", 10),
                    close("a", 15),
                    close("b", 20),
                    close("b", 25),
                ]);
                assert!(errors.iter().all(|e| !e.message.contains("has no matching opener")));
                assert_eq!(errors.len(), 2); // two misnesting errors
            }

            #[test]
            fn credits_for_different_tags_dont_interfere() {
                // <a><b><c></a></b></c>
                // eviction: 1 credit for "b", 1 credit for "c"
                // </b> consumes b credit, </c> consumes c credit
                let errors = run(&[
                    open("a", 0),
                    open("b", 5),
                    open("c", 10),
                    close("a", 15),
                    close("b", 20),
                    close("c", 25),
                ]);
                assert!(errors.iter().all(|e| !e.message.contains("has no matching opener")));
                // two misnesting errors only
                assert_eq!(errors.len(), 2);
            }
        }

        mod mixed_errors {
            use super::*;

            #[test]
            fn misnesting_orphan_and_unclosed() {
                // <div><a><b></a></span>
                // - <b> misnested (forced by </a>)
                // - </span> orphan
                // - <div> unclosed
                let errors = run(&[
                    open("div", 0),
                    open("a", 5),
                    open("b", 10),
                    close("a", 15),
                    close("span", 20),
                ]);

                let misnested: Vec<_> = errors.iter().filter(|e| e.message.contains("misnested")).collect();
                let orphans: Vec<_> = errors
                    .iter()
                    .filter(|e| e.message.contains("no matching opener"))
                    .collect();
                let unclosed: Vec<_> = errors.iter().filter(|e| e.message.contains("unclosed")).collect();

                assert_eq!(misnested.len(), 1);
                assert_eq!(orphans.len(), 1);
                assert_eq!(unclosed.len(), 1);
                assert!(unclosed[0].message.contains("div"));
            }
        }

        mod repeated_tags {
            use super::*;

            #[test]
            fn same_tag_nested_valid() {
                // <a><a></a></a>
                let errors = run(&[open("a", 0), open("a", 5), close("a", 10), close("a", 15)]);
                assert!(errors.is_empty());
            }

            #[test]
            fn same_tag_sequential_valid() {
                // <a></a><a></a>
                let errors = run(&[open("a", 0), close("a", 5), open("a", 10), close("a", 15)]);
                assert!(errors.is_empty());
            }

            #[test]
            fn same_tag_nested_unclosed() {
                // <a><a></a> — inner pair matches, outer unclosed
                let errors = run(&[open("a", 0), open("a", 5), close("a", 10)]);
                assert_eq!(errors.len(), 1);
                assert_messages(&errors, &["unclosed <a>"]);
                assert_offsets(&errors, &[0]);
            }
        }

        mod error_metadata {
            use super::*;

            #[test]
            fn misnesting_related_info() {
                // <a><b></a></b>
                let errors = run(&[open("a", 0), open("b", 100), close("a", 200), close("b", 300)]);
                assert_eq!(errors.len(), 1);
                let err = &errors[0];
                assert_eq!(err.offset.value(), 100); // misnested <b> offset
                assert_eq!(err.related.len(), 1);
                assert_eq!(err.related[0].offset.value(), 200); // forced by </a> offset
                assert!(err.related[0].message.contains("forced by </a>"));
            }

            #[test]
            fn all_offsets_are_exact() {
                let errors = run(&[open("div", 42)]);
                assert_eq!(errors.len(), 1);
                assert!(matches!(errors[0].offset, ErrorOffset::Exact(42)));
            }

            #[test]
            fn severity_is_always_error() {
                // misnesting
                let e1 = run(&[open("a", 0), open("b", 5), close("a", 10)]);
                // orphan
                let e2 = run(&[close("a", 0)]);
                // unclosed
                let e3 = run(&[open("a", 0)]);

                for errors in [&e1, &e2, &e3] {
                    for err in errors {
                        assert!(matches!(err.severity, Severity::Error));
                    }
                }
            }

            #[test]
            fn orphan_has_no_related_info() {
                let errors = run(&[close("div", 0)]);
                assert!(errors[0].related.is_empty());
            }

            #[test]
            fn unclosed_has_no_related_info() {
                let errors = run(&[open("div", 0)]);
                assert!(errors[0].related.is_empty());
            }
        }

        mod edge_cases {
            use super::*;

            #[test]
            fn single_open() {
                let errors = run(&[open("x", 0)]);
                assert_eq!(errors.len(), 1);
                assert_messages(&errors, &["unclosed <x>"]);
            }

            #[test]
            fn single_close() {
                let errors = run(&[close("x", 0)]);
                assert_eq!(errors.len(), 1);
                assert_messages(&errors, &["closing </x> has no matching opener"]);
            }

            #[test]
            fn alternating_different_tags() {
                // <a></b><b></a> — orphan </b>, then <b> misnested by </a>
                let errors = run(&[open("a", 0), close("b", 5), open("b", 10), close("a", 15)]);
                // </b> at 5: no opener (no eviction credit for b yet) → orphan
                // </a> at 15: matches <a> at 0, evicts <b> at 10 → misnested
                // eviction credit for b=1, but already consumed the orphan error
                assert_eq!(errors.len(), 2);
                let orphan = errors.iter().find(|e| e.message.contains("no matching opener"));
                assert!(orphan.is_some());
                let misnested = errors.iter().find(|e| e.message.contains("misnested"));
                assert!(misnested.is_some());
            }

            #[test]
            fn long_sequence_valid() {
                let mut events = Vec::new();
                for i in 0..100 {
                    events.push(open("div", i * 10));
                }
                for i in (0..100).rev() {
                    events.push(close("div", 1000 + i * 10));
                }
                let errors = run(&events);
                assert!(errors.is_empty());
            }

            #[test]
            fn long_sequence_all_unclosed() {
                let events: Vec<_> = (0..50).map(|i| open("p", i * 10)).collect();
                let errors = run(&events);
                assert_eq!(errors.len(), 50);
                assert!(errors.iter().all(|e| e.message.contains("unclosed")));
            }
        }

        #[cfg(test)]
        mod adversarial {
            use super::*;
            use crate::rules::html::utils::tokensink::TagEvent;

            fn open(tag: &str, offset: usize) -> TagEvent {
                TagEvent {
                    tag: tag.to_string(),
                    is_open: true,
                    offset,
                }
            }

            fn close(tag: &str, offset: usize) -> TagEvent {
                TagEvent {
                    tag: tag.to_string(),
                    is_open: false,
                    offset,
                }
            }

            fn run(events: &[TagEvent]) -> Vec<HtmlError> {
                let mut errors = Vec::new();
                tag_balance(events, &mut errors);
                errors
            }

            // ── Eviction credit over-consumption ────────────────────────────
            // Can we trick the multiset into going negative or masking real orphans?

            mod eviction_edge_cases {
                use super::*;

                #[test]
                fn eviction_credit_not_shared_across_tags() {
                    // <a><b></a></c>
                    // <b> evicted (credit for "b"), </c> has no credit — must be orphan
                    let errors = run(&[open("a", 0), open("b", 5), close("a", 10), close("c", 15)]);
                    let orphan = errors.iter().find(|e| e.message.contains("</c>"));
                    assert!(
                        orphan.is_some(),
                        "wrong tag must not consume another tag's eviction credit"
                    );
                }

                #[test]
                fn eviction_credit_zero_stays_in_map() {
                    // <a><b></a></b></b>
                    // After first </b> consumes credit, count=0. Second </b> must error.
                    // Bug vector: HashMap entry exists with value 0, filter(|c| **c > 0) must catch it.
                    let errors = run(&[
                        open("a", 0),
                        open("b", 5),
                        close("a", 10),
                        close("b", 15),
                        close("b", 20),
                    ]);
                    let orphans: Vec<_> = errors
                        .iter()
                        .filter(|e| e.message == "closing </b> has no matching opener")
                        .collect();
                    assert_eq!(orphans.len(), 1);
                    assert_eq!(orphans[0].offset.value(), 20);
                }

                #[test]
                fn multiple_eviction_rounds_accumulate() {
                    // <x><b></x> <x><b></x> </b></b>
                    // Two separate evictions of "b" → 2 credits. Both </b> should be silent.
                    let errors = run(&[
                        open("x", 0),
                        open("b", 5),
                        close("x", 10),
                        open("x", 15),
                        open("b", 20),
                        close("x", 25),
                        close("b", 30),
                        close("b", 35),
                    ]);
                    assert!(
                        errors.iter().all(|e| !e.message.contains("no matching opener")),
                        "both </b> should consume eviction credits"
                    );
                }
            }

            // ── Stack search direction ──────────────────────────────────────
            // rposition finds the *last* match. What if the wrong one is matched?

            mod stack_search {
                use super::*;

                #[test]
                fn closer_matches_innermost_not_outermost() {
                    // <a> <a> </a>
                    // Must match inner <a> at 5, leaving outer <a> at 0 unclosed.
                    let errors = run(&[open("a", 0), open("a", 5), close("a", 10)]);
                    assert_eq!(errors.len(), 1);
                    assert_eq!(errors[0].message, "unclosed <a>");
                    assert_eq!(errors[0].offset.value(), 0, "outer <a> must be the unclosed one");
                }

                #[test]
                fn three_deep_same_tag_close_one() {
                    // <a><a><a></a> — innermost matched, two unclosed
                    let errors = run(&[open("a", 0), open("a", 5), open("a", 10), close("a", 15)]);
                    assert_eq!(errors.len(), 2);
                    // Unclosed errors should be for offsets 0 and 5 (stack order)
                    let offsets: Vec<usize> = errors.iter().map(|e| e.offset.value()).collect();
                    assert_eq!(offsets, vec![0, 5]);
                }

                #[test]
                fn closer_skips_intervening_different_tag() {
                    // <a><b><a></a> — </a> must match inner <a> at 10, not outer <a> at 0
                    // No eviction of <b> because <a> at 10 is on top-ish
                    // Wait: stack is [<a>0, <b>5, <a>10]. rposition for "a" finds index 2 (<a>10).
                    // pos=2, stack.len()=3, pos+1=3 → no eviction. Pop <a>10. Stack: [<a>0, <b>5].
                    let errors = run(&[open("a", 0), open("b", 5), open("a", 10), close("a", 15)]);
                    // <a>0 and <b>5 remain unclosed
                    assert_eq!(errors.len(), 2);
                    assert!(errors.iter().all(|e| e.message.contains("unclosed")));
                }
            }

            // ── Eviction then re-open same tag ──────────────────────────────
            // Evicted tag reopened — credit should still exist independently.

            mod reopen_after_eviction {
                use super::*;

                #[test]
                fn reopen_evicted_tag_then_close_twice() {
                    // <a><b></a> <b></b> </b>
                    // <b>5 evicted (credit=1). <b>15 opened fresh. </b>20 closes <b>15. </b>25 consumes credit.
                    let errors = run(&[
                        open("a", 0),
                        open("b", 5),
                        close("a", 10),
                        open("b", 15),
                        close("b", 20),
                        close("b", 25),
                    ]);
                    assert!(
                        errors.iter().all(|e| !e.message.contains("no matching opener")),
                        "</b> at 25 should consume eviction credit, not be orphan"
                    );
                }

                #[test]
                fn reopen_evicted_tag_close_only_once() {
                    // <a><b></a> <b></b>
                    // <b>5 evicted (credit=1). <b>15 opened. </b>20 closes <b>15 (stack match).
                    // Credit=1 remains unused. No errors except the misnesting.
                    let errors = run(&[
                        open("a", 0),
                        open("b", 5),
                        close("a", 10),
                        open("b", 15),
                        close("b", 20),
                    ]);
                    assert_eq!(errors.len(), 1);
                    assert!(errors[0].message.contains("misnested <b>"));
                }
            }

            // ── Cascading evictions ─────────────────────────────────────────
            // Eviction triggers further eviction credits. Order matters.

            mod cascading {
                use super::*;

                #[test]
                fn eviction_then_immediate_eviction() {
                    // <a><b><c></a></b></c>
                    // </a> evicts <c> and <b>. </b> and </c> consume credits. No orphans.
                    // Then: what if we add another closer?
                    // <a><b><c></a></b></c></c>
                    let errors = run(&[
                        open("a", 0),
                        open("b", 5),
                        open("c", 10),
                        close("a", 15),
                        close("b", 20),
                        close("c", 25),
                        close("c", 30),
                    ]);
                    let orphans: Vec<_> = errors
                        .iter()
                        .filter(|e| e.message.contains("no matching opener"))
                        .collect();
                    assert_eq!(orphans.len(), 1, "extra </c> must be orphan");
                    assert_eq!(orphans[0].offset.value(), 30);
                }

                #[test]
                fn nested_eviction_chains() {
                    // <a><b><c><d></b></c></d></a>
                    // </b> at 20: matches <b> at 5, evicts <d>10 <c>15 ... wait let me re-index.
                    // <a>0 <b>5 <c>10 <d>15 </b>20 </c>25 </d>30 </a>35
                    // </b>20: rposition finds <b>5 at stack index 1. Evicts <d>15, <c>10. Stack: [<a>0]. Pop <b>5.
                    //   credits: c=1, d=1
                    // </c>25: no stack match. credit c=1→0. Silent.
                    // </d>30: no stack match. credit d=1→0. Silent.
                    // </a>35: matches <a>0. Stack empty.
                    let errors = run(&[
                        open("a", 0),
                        open("b", 5),
                        open("c", 10),
                        open("d", 15),
                        close("b", 20),
                        close("c", 25),
                        close("d", 30),
                        close("a", 35),
                    ]);
                    // Only 2 misnesting errors (<d> and <c>), no orphans, no unclosed
                    assert_eq!(errors.len(), 2);
                    assert!(errors.iter().all(|e| e.message.contains("misnested")));
                }
            }

            // ── Pathological inputs ─────────────────────────────────────────

            mod pathological {
                use super::*;

                #[test]
                fn all_opens_then_all_closes_wrong_order() {
                    // <a><b><c></c></b></a> is valid.
                    // <a><b><c></a></b></c> — each closer evicts everything above its match.
                    let errors = run(&[
                        open("a", 0),
                        open("b", 5),
                        open("c", 10),
                        close("a", 15),
                        close("b", 20),
                        close("c", 25),
                    ]);
                    // </a>15 evicts <c>10, <b>5. credits: b=1, c=1.
                    // </b>20 consumes credit. </c>25 consumes credit.
                    assert_eq!(errors.len(), 2);
                    assert!(errors.iter().all(|e| e.message.contains("misnested")));
                }

                #[test]
                fn interleaved_pairs_no_nesting() {
                    // <a><b></a><c></b></c>
                    let errors = run(&[
                        open("a", 0),
                        open("b", 5),
                        close("a", 10),
                        open("c", 15),
                        close("b", 20),
                        close("c", 25),
                    ]);
                    // </a>10: evicts <b>5, credit b=1. Stack: []. Pop <a>0.
                    // </b>20: no stack match, credit b→0. Silent.
                    // <c>15 pushed. </c>25 matches. Clean.
                    assert_eq!(errors.len(), 1);
                    assert!(errors[0].message.contains("misnested <b>"));
                }

                #[test]
                fn same_tag_open_close_interleaved_stress() {
                    // <a><a><a></a></a></a> — valid Russian doll
                    let errors = run(&[
                        open("a", 0),
                        open("a", 5),
                        open("a", 10),
                        close("a", 15),
                        close("a", 20),
                        close("a", 25),
                    ]);
                    assert!(errors.is_empty());
                }

                #[test]
                fn same_tag_all_opens_all_closes_reversed() {
                    // <a><a><a></a></a></a> is the same as above — rposition always matches innermost.
                    // But what about: <a><a></a> </a> </a> with extra closer?
                    // <a>0 <a>5 </a>10 </a>15 </a>20
                    let errors = run(&[
                        open("a", 0),
                        open("a", 5),
                        close("a", 10),
                        close("a", 15),
                        close("a", 20),
                    ]);
                    // </a>10 matches <a>5. </a>15 matches <a>0. </a>20 orphan.
                    assert_eq!(errors.len(), 1);
                    assert!(errors[0].message.contains("no matching opener"));
                    assert_eq!(errors[0].offset.value(), 20);
                }

                #[test]
                fn duplicate_offset_does_not_confuse() {
                    // Two events at same offset — shouldn't happen in practice but shouldn't panic.
                    let errors = run(&[open("a", 0), open("b", 0), close("a", 0)]);
                    // <b> misnested at 0, forced by </a> at 0
                    assert_eq!(errors.len(), 1);
                    assert!(errors[0].message.contains("misnested <b>"));
                }

                #[test]
                #[cfg(debug_assertions)]
                #[should_panic(expected = "void element leaked into tag_events")]
                fn void_element_panics() {
                    run(&[open("br", 0)]);
                }

                #[test]
                fn empty_tag_name() {
                    // Shouldn't happen from tokenizer, but tag_balance shouldn't panic.
                    let errors = run(&[open("", 0), close("", 5)]);
                    assert!(errors.is_empty());
                }
            }
        }
    }

    #[cfg(test)]
    mod integration_tests {
        use super::*;
        use markdown_it::MarkdownIt;

        fn lint(input: &str) -> Vec<String> {
            let mut md = MarkdownIt::new();
            markdown_it::plugins::cmark::add(&mut md);
            markdown_it::plugins::html::add(&mut md);

            let mut root = md.parse(input);
            let errors = validate(&mut root);
            errors.into_iter().map(|e| e.message).collect()
        }

        fn assert_no_errors(input: &str) {
            let errors = lint(input);
            assert!(
                errors.is_empty(),
                "expected no errors for input:\n{input}\ngot: {errors:?}"
            );
        }

        fn assert_has_message(errors: &[String], needle: &str) {
            assert!(
                errors.iter().any(|m| m.contains(needle)),
                "expected message containing {needle:?} in {errors:?}"
            );
        }

        // ── 1. Valid HTML in markdown ────────────────────────────────────

        mod valid_html {
            use super::*;

            #[test]
            fn inline_tag_pair() {
                assert_no_errors("some *text* <span>content</span> here");
            }

            #[test]
            fn block_level_html() {
                assert_no_errors("<div>\n  <p>hello</p>\n</div>");
            }

            #[test]
            fn nested_block_html() {
                assert_no_errors("<div>\n  <section>\n    <p>deep</p>\n  </section>\n</div>");
            }

            #[test]
            fn mixed_inline_and_block() {
                assert_no_errors("# Title\n\n<div>\n\nparagraph with <em>emphasis</em>\n\n</div>");
            }

            #[test]
            fn void_elements_no_errors() {
                assert_no_errors("text <br> more <hr>\n\n<img src=\"x.png\">");
            }

            #[test]
            fn self_closing_void() {
                assert_no_errors("line<br/>break");
            }

            #[test]
            fn multiple_valid_blocks() {
                assert_no_errors("<div>first</div>\n\n<div>second</div>");
            }
        }

        // ── 2. Errors within a single HTML node ─────────────────────────

        mod single_node_errors {
            use super::*;

            #[test]
            fn unclosed_in_block() {
                let errors = lint("<div>\n  <span>\n</div>");
                assert_has_message(&errors, "misnested <span>");
            }

            #[test]
            fn orphan_closer_inline() {
                let errors = lint("text </span> more");
                assert_has_message(&errors, "closing </span> has no matching opener");
            }

            #[test]
            fn misnesting_in_single_block() {
                let errors = lint("<div>\n  <a><b></a></b>\n</div>");
                assert_has_message(&errors, "misnested <b>");
            }

            #[test]
            fn unclosed_in_single_block() {
                let errors = lint("<div>\n  <span>\n  <em>\n</div>");
                // span and em misnested, forced by </div>
                assert_has_message(&errors, "misnested <span>");
                assert_has_message(&errors, "misnested <em>");
            }
        }

        // ── 3. Errors across HTML nodes ─────────────────────────────────

        mod cross_node {
            use super::*;

            #[test]
            fn open_and_close_in_separate_blocks() {
                // markdown paragraph between two HTML blocks
                assert_no_errors("<div>\n\nsome paragraph\n\n</div>");
            }

            #[test]
            fn unclosed_across_blocks() {
                let errors = lint("<div>\n\nsome paragraph\n\n<span>\n\nmore text");
                assert_has_message(&errors, "unclosed <div>");
                assert_has_message(&errors, "unclosed <span>");
            }

            #[test]
            fn orphan_close_in_later_block() {
                let errors = lint("paragraph\n\n</div>");
                assert_has_message(&errors, "closing </div> has no matching opener");
            }

            #[test]
            fn misnesting_across_blocks() {
                // <a> in one inline, </b></a> pattern across nodes
                let errors = lint("<a>\n\n<b>\n\n</a>\n\n</b>");
                assert_has_message(&errors, "misnested <b>");
            }

            #[test]
            fn multiple_blocks_valid_together() {
                assert_no_errors("<div>\n\n<p>\n\ntext\n\n</p>\n\n</div>");
            }
        }

        // ── 4. Offset/sourcemap correctness ─────────────────────────────

        mod offsets {
            use super::*;

            fn lint_full(input: &str) -> Vec<HtmlError> {
                let mut md = MarkdownIt::new();
                markdown_it::plugins::cmark::add(&mut md);
                markdown_it::plugins::html::add(&mut md);
                add(&mut md);

                let mut root = md.parse(input);
                validate(&mut root)
            }

            #[test]
            fn inline_error_offset_nonzero() {
                // error should not be at offset 0 — there's preceding text
                let errors = lint_full("hello </div> world");
                assert!(!errors.is_empty());
                assert!(errors[0].offset.value() > 0, "offset should reflect preceding text");
            }

            #[test]
            fn block_error_after_content() {
                let input = "# Title\n\nSome paragraph.\n\n<div>";
                let errors = lint_full(input);
                assert!(!errors.is_empty());
                let offset = errors[0].offset.value();
                // offset should be somewhere after "# Title\n\nSome paragraph.\n\n"
                assert!(offset >= 26, "offset {offset} should be after preceding markdown");
            }

            #[test]
            fn exact_vs_node_start() {
                let errors = lint_full("text </span> more");
                assert!(!errors.is_empty());
                // tag_balance always produces Exact offsets
                assert!(matches!(errors[0].offset, ErrorOffset::Exact(_)));
            }

            #[test]
            fn errors_sorted_by_offset() {
                let errors = lint_full("</b>\n\n<a>\n\n</c>");
                let offsets: Vec<usize> = errors.iter().map(|e| e.offset.value()).collect();
                let mut sorted = offsets.clone();
                sorted.sort_unstable();
                assert_eq!(offsets, sorted, "errors should be sorted by offset");
            }
        }

        // ── 5. Non-HTML markdown doesn't interfere ──────────────────────

        mod non_html {
            use super::*;

            #[test]
            fn code_block_angle_brackets() {
                assert_no_errors("```\n<div><span></div>\n```");
            }

            #[test]
            fn inline_code_angle_brackets() {
                assert_no_errors("use `<div>` for containers and `</span>` to close");
            }

            #[test]
            fn no_html_at_all() {
                assert_no_errors("# Hello\n\nJust **markdown** with [links](url).");
            }

            #[test]
            fn angle_brackets_in_text() {
                // markdown-it may or may not parse these as HTML — either way, no crash
                let _ = lint("5 < 10 and 20 > 15");
            }
        }

        // ── 6. Edge cases ───────────────────────────────────────────────

        mod edge_cases {
            use super::*;

            #[test]
            fn empty_document() {
                assert_no_errors("");
            }

            #[test]
            fn only_html_no_markdown() {
                assert_no_errors("<div><p><em>all html</em></p></div>");
            }

            #[test]
            fn html_in_blockquote() {
                // blockquotes may or may not pass through HTML — don't crash
                let _ = lint("> <div>quoted</div>");
            }

            #[test]
            fn html_in_list_items() {
                let _ = lint("- <span>item one</span>\n- <em>item two</em>");
            }

            #[test]
            fn adjacent_html_blocks_no_gap() {
                assert_no_errors("<div>first</div>\n<div>second</div>");
            }

            #[test]
            fn deeply_nested_markdown_with_html() {
                assert_no_errors("> > - <em>deep</em>");
            }

            #[test]
            fn whitespace_only_html_block() {
                // block with only whitespace — extract_html_content trims and skips
                assert_no_errors("<div>   </div>");
            }

            #[test]
            fn html_after_thematic_break() {
                assert_no_errors("---\n\n<div>after break</div>");
            }
        }
    }
}
