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

struct ScanResult {
    errors: Vec<HtmlError>,
    warnings: Vec<HtmlError>,
    tag_events: Vec<tokensink::TagEvent>,
}

pub struct HtmlValidatorRule;

impl CoreRule for HtmlValidatorRule {
    fn run(root: &mut Node, _md: &MarkdownIt) {
        let errors = validate(root);
        log_errors(errors, root);
    }
}

fn validate(root: &Node) -> Vec<HtmlError> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut tag_events = Vec::new();

    crate::debug_write("01-ast.txt", &format!("{root:#?}"));

    root.walk(|node, _| {
        let Some((node_html, node_offset)) = extract_html_content(node) else {
            return;
        };

        let result = scan_node_html(&node_html, node_offset);

        errors.extend(result.errors);
        warnings.extend(result.warnings);
        tag_events.extend(result.tag_events);
    });

    crate::debug_write("02-ast-html-validator.txt", &format!("{root:#?}"));

    validate_tag_nesting(&tag_events, &mut errors);

    errors.extend(warnings);

    errors.sort_by_key(|e| match e.offset {
        ErrorOffset::Exact(o) => (0, o),
        ErrorOffset::NodeStart(o) => (1, o),
    });

    errors
}

fn extract_html_content(node: &Node) -> Option<(String, usize)> {
    let byte_offset_start = node.srcmap.as_ref().map_or(0, |sp| sp.get_byte_offsets().0);

    if let Some(hb) = node.cast::<HtmlBlock>()
        && !hb.content.trim().is_empty()
    {
        return Some((hb.content.clone(), byte_offset_start));
    }

    if let Some(hb) = node.cast::<HtmlInline>()
        && !hb.content.trim().is_empty()
    {
        return Some((hb.content.clone(), byte_offset_start));
    }

    None
}

fn scan_node_html(node_html: &str, node_offset: usize) -> ScanResult {
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

    ScanResult {
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
fn validate_tag_nesting(tag_events: &[tokensink::TagEvent], errors: &mut Vec<HtmlError>) {
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

fn log_errors(errors: Vec<HtmlError>, root: &Node) {
    let source = root
        .cast::<markdown_it::parser::core::Root>()
        .expect("root node")
        .content
        .clone();
    let source_map = SourceWithLineStarts::new(&source);

    for error in errors {
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
}

#[cfg(test)]
#[path = "validator/validate_tag_nesting_tests.rs"]
mod validate_tag_nesting_tests;

#[cfg(test)]
#[path = "validator/validate_tests.rs"]
mod validate_tests;
