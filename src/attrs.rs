//! HTML attributes
//!
//! looks like `{attrs}`
//!
//! <https://github.com/jgm/djot/blob/main/doc/syntax.md#inline-attributes>
//! <https://github.com/jgm/djot/blob/main/doc/syntax.md#block-attributes>
//!
use markdown_it::parser::core::CoreRule;
use markdown_it::parser::inline::builtin::InlineParserRule;
use markdown_it::parser::inline::{Text, TextSpecial};
use markdown_it::plugins::cmark::block::list::{BulletList, ListItem};
use markdown_it::plugins::cmark::block::paragraph::Paragraph;
use markdown_it::plugins::cmark::inline::newline::Softbreak;
use markdown_it::plugins::html::html_block::HtmlBlock;
use markdown_it::{MarkdownIt, Node};

use crate::intern::intern;
use crate::jotdown_attr::Attributes as JotdownAttrs;

pub fn add(md: &mut MarkdownIt) {
    md.add_rule::<AttrsRule>().after::<InlineParserRule>();
}

pub struct AttrsRule;

impl CoreRule for AttrsRule {
    fn run(root: &mut Node, _md: &MarkdownIt) {
        crate::debug_write("01-ast.txt", &format!("{root:#?}"));

        // Pass 1: Inline attributes
        root.walk_mut(|node, _| {
            apply_inline_attrs(node);
        });

        crate::debug_write("02-ast-inline_attrs.txt", &format!("{root:#?}"));

        // Pass 2: Block-level attributes
        root.walk_mut(|node, _| {
            apply_block_attrs(node);
        });

        crate::debug_write("03-ast-block_attrs.txt", &format!("{root:#?}"));

        // Pass 3: Derived attributes
        root.walk_mut(|node, _| {
            apply_derived_attrs(node);
        });

        crate::debug_write("04-ast-_attrs.txt", &format!("{root:#?}"));
    }
}

fn apply_inline_attrs(node: &mut Node) {
    if node.children.is_empty() {
        return;
    }

    let mut to_remove = Vec::new();

    // Start at 1: inline attrs always apply to the *preceding* sibling,
    // so index 0 can never be an attrs target (nothing before it).
    for i in 1..node.children.len() {
        let (attrs, remaining) = {
            // Must follow a structural inline element (not plain text or whitespace)
            if node.children[i - 1].cast::<Text>().is_some()
                || node.children[i - 1].cast::<TextSpecial>().is_some()
                || node.children[i - 1].cast::<Softbreak>().is_some()
            {
                continue;
            }

            // Must be a Text node
            let Some(text) = node.children[i].cast::<Text>() else {
                continue;
            };

            if !text.content.starts_with('{') {
                continue;
            }

            let (attrs_raw, remaining_raw) = split_attrs_remaining(&text.content);
            if attrs_raw.is_empty() {
                continue;
            }

            let attrs = parse_jotdown(attrs_raw);
            let remaining = (!remaining_raw.is_empty()).then(|| remaining_raw.to_string());
            (attrs, remaining)
        };

        let prev_sibling = &mut node.children[i - 1];
        for (key, value) in attrs {
            prev_sibling.attrs.push((intern(&key), value));
        }

        if let Some(new_content) = remaining {
            node.children[i]
                .cast_mut::<Text>()
                .expect("node was verified as Text above")
                .content = new_content;
        } else {
            // The entire Text was attrs, and so mark it for removal
            to_remove.push(i);
        }
    }

    remove_children(node, &to_remove);
}

/// Two patterns:
/// 1. Detached: `{ attrs }`\n\ntext → apply to next sibling, remove paragraph
/// 2. Attached: `{ attrs }\ntext` → apply to this paragraph, remove attrs text
fn apply_block_attrs(node: &mut Node) {
    if node.children.is_empty() {
        return;
    }

    let mut to_remove = Vec::new();

    for i in 0..node.children.len() {
        // P    > TA [SB TA]*
        // P    > TA [SB TA]* SB X Y ...
        // LI   > TA [SB TA]*
        // LI   > TA [SB TA]* SB X Y ...
        if node.children[i].cast::<Paragraph>().is_none() && node.children[i].cast::<ListItem>().is_none() {
            continue;
        }

        let Some((all_attrs_raw, attrs_end_idx)) = scan_leading_attrs(&node.children[i].children) else {
            continue;
        };

        let all_attrs = parse_jotdown(&all_attrs_raw);
        if all_attrs.is_empty() {
            continue;
        }

        let children_len = node.children[i].children.len();

        if attrs_end_idx == children_len {
            // node.children[i].children: TA [SB TA]* - all children are attrs

            // Special case: ListItem all children are attrs, we add attrs to the empty <li> element
            // LI   > TA [SB TA]*
            if node.children[i].cast::<ListItem>().is_some() {
                // Apply those attrs to the <li> itself.
                for (key, value) in all_attrs {
                    node.children[i].attrs.push((intern(&key), value));
                }

                // strip first j+1 children (attrs + trailing softbreak)
                // The Paragraph's or ListItem's srcmap is deliberately
                // kept spanning the original range, attrs were part of
                // this paragraph's or listitem's source, just extracted
                // as metadata.
                node.children[i].children.drain(..attrs_end_idx);

                continue;
            }

            // No next sibling to apply the attrs to: {.orphan}
            let has_next_sibling = i + 1 < node.children.len();
            if !has_next_sibling {
                continue;
            }

            // Do not apply attrs to html blocks, use HTML attributes directly
            if node.children[i + 1].cast::<HtmlBlock>().is_some() {
                continue;
            }

            // Special case: ListItem is the node, first child Paragraph is all-attrs, we add attrs to the <li> element
            //    ┌ P > TA [SB TA]*
            // LI ├ X
            //    ├ Y
            if node.cast::<ListItem>().is_some() && i == 0 {
                // Apply those attrs to the <li> itself.
                for (key, value) in all_attrs {
                    node.attrs.push((intern(&key), value));
                }

                // The entire Paragraph was attrs, and so mark it for removal
                to_remove.push(i);

                continue;
            }

            // Next sibling has an attrs-only first child starting a new attrs block: {.widow}
            let first_is_attr = node.children[i + 1]
                .children
                .first()
                .is_some_and(|c| is_attr_only_text(c).is_some());
            if first_is_attr {
                continue;
            }

            // Apply attrs to next sibling (node.children[i + 1])
            for (key, value) in all_attrs {
                node.children[i + 1].attrs.push((intern(&key), value));
            }

            // The entire Paragraph was attrs, and so mark it for removal
            to_remove.push(i);
        } else if attrs_end_idx < children_len && node.children[i].children[attrs_end_idx].cast::<Softbreak>().is_some()
        {
            // node.children[i].children: TA [SB TA]* SB X Y ... - a prefix of attrs and an SB between content

            // Apply attrs to the current Paragraph or ListItem (node.children[i])
            for (key, value) in all_attrs {
                node.children[i].attrs.push((intern(&key), value));
            }

            // strip first j+1 children (attrs + trailing softbreak)
            // The Paragraph's or ListItem's srcmap is deliberately
            // kept spanning the original range, attrs were part of
            // this paragraph's or listitem's source, just extracted
            // as metadata.
            node.children[i].children.drain(..=attrs_end_idx);
        }
    }

    remove_children(node, &to_remove);
}

/// Scans a paragraph's inline children for a leading sequence of attribute-only
/// Text nodes separated by Softbreaks.
///
/// Returns `(all_attrs, j)` where `j` is the index past the last consumed
/// attr node, or `None` if the first child isn't a valid attr-only Text.
fn scan_leading_attrs(children: &[Node]) -> Option<(String, usize)> {
    let mut all_attrs = String::new();

    // Process first child, and check if it is a TA
    // TA
    let attrs = is_attr_only_text(children.first()?)?;
    all_attrs.push_str(attrs);

    // Process children starting from the second
    // Looking for [SB TA] pairs
    let mut j = 1;
    while j + 1 < children.len() {
        // SB
        if children[j].cast::<Softbreak>().is_none() {
            break;
        }

        // TA
        let Some(attrs) = is_attr_only_text(&children[j + 1]) else {
            break;
        };
        all_attrs.push_str(attrs);

        j += 2;
    }

    Some((all_attrs, j))
}

fn is_attr_only_text(node: &Node) -> Option<&str> {
    let text = node.cast::<Text>()?;
    let (attrs, remaining) = split_attrs_remaining(&text.content);
    if attrs.is_empty() || !remaining.is_empty() {
        return None;
    }
    Some(attrs)
}

fn split_attrs_remaining(s: &str) -> (&str, &str) {
    let mut pos = 0;

    loop {
        let rest = &s[pos..];

        if rest.is_empty() || !rest.starts_with('{') {
            break;
        }

        if let Some(close) = find_closing_brace(&s[pos + 1..]) {
            pos = pos + 1 + close + 1;
        } else {
            break;
        }
    }

    (&s[..pos], &s[pos..])
}

/// Find closing `}` index, respecting double-quoted values and backslash escapes.
///
/// Matches Jotdown's quoting rules: only `"..."` double quoated values with `\"` escape.
fn find_closing_brace(s: &str) -> Option<usize> {
    let mut in_quote = false;
    let mut chars = s.char_indices();

    while let Some((i, c)) = chars.next() {
        match (c, in_quote) {
            ('\\', true) => {
                chars.next();
            } // skip escaped char in quoted value
            ('"', _) => in_quote = !in_quote,
            ('}', false) => return Some(i),
            _ => {}
        }
    }
    None
}

/// Parse attributes using Jotdown's parser.
///
/// Accepts either `{.class #id}` or raw `.class #id` (auto-wrapped).
fn parse_jotdown(input: &str) -> Vec<(String, String)> {
    let input = input.trim();
    if input.is_empty() {
        return Vec::new();
    }

    // Wrap in braces if not already
    let owned;
    let to_parse = if input.starts_with('{') {
        input
    } else {
        owned = format!("{{{input}}}");
        &owned
    };

    let Ok(attrs) = JotdownAttrs::try_from(to_parse) else {
        return Vec::new();
    };

    attrs
        .unique_pairs()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

fn remove_children(node: &mut Node, to_remove: &[usize]) {
    if to_remove.is_empty() {
        return;
    }

    let mut ri = 0;
    let mut write = 0;
    for read in 0..node.children.len() {
        if ri < to_remove.len() && to_remove[ri] == read {
            ri += 1;
        } else {
            if write != read {
                node.children.swap(write, read);
            }
            write += 1;
        }
    }
    node.children.truncate(write);
}

fn apply_derived_attrs(node: &mut Node) {
    if node.attrs.is_empty() {
        return;
    }

    // Unordered lists with custom attrs likely have custom styling.
    // Add role="list" to preserve screen reader semantics,
    // since our CSS strips markers via ul[role="list"] { list-style: none }
    if node.cast::<BulletList>().is_some() {
        node.attrs.push(("role", "list".to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use markdown_it::plugins::cmark;
    use markdown_it::plugins::html;
    use std::sync::LazyLock;

    mod find_closing_brace {
        use super::*;

        #[test]
        fn empty() {
            assert_eq!(find_closing_brace(""), None);
        }

        #[test]
        fn no_brace() {
            assert_eq!(find_closing_brace("abc"), None);
        }

        #[test]
        fn immediate_close() {
            assert_eq!(find_closing_brace("}"), Some(0));
        }

        #[test]
        fn immediate_close_multiple() {
            assert_eq!(find_closing_brace("}}"), Some(0));
        }

        #[test]
        fn immediate_close_whitespace() {
            assert_eq!(find_closing_brace("} "), Some(0));
        }

        #[test]
        fn delayed_close() {
            assert_eq!(find_closing_brace(" }"), Some(1));
        }

        #[test]
        fn tab_close() {
            assert_eq!(find_closing_brace(r"\t}"), Some(2));
        }

        #[test]
        fn newline_close() {
            assert_eq!(find_closing_brace(r"\n}"), Some(2));
        }

        #[test]
        fn close_after_content() {
            assert_eq!(find_closing_brace("key=val}"), Some(7));
        }

        #[test]
        fn close_after_quoted_content() {
            assert_eq!(find_closing_brace(r#"key="val"}"#), Some(9));
        }

        #[test]
        fn quoted_brace_ignored() {
            // `"}"` should not match — brace inside quotes
            assert_eq!(find_closing_brace(r#"key="}"}"#), Some(7));
        }

        #[test]
        fn quoted_brace_ignored_multiple() {
            // `"}"` should not match — brace inside quotes
            assert_eq!(find_closing_brace(r#"key="}" "}"}"#), Some(11));
        }

        #[test]
        fn escaped_quote_in_quoted_value() {
            // `"val\"}"` — escaped quote keeps us in-quote, real `}` is after close-quote
            assert_eq!(find_closing_brace(r#"key="v\""}"#), Some(9));
        }

        #[test]
        fn backslash_outside_quote_not_special() {
            // `\` outside quotes is literal, `}` still matches
            assert_eq!(find_closing_brace(r"\}"), Some(1));
        }

        #[test]
        fn multiple_quoted_segments() {
            assert_eq!(find_closing_brace(r#"a="x" b="y"}"#), Some(11));
        }

        #[test]
        fn nested_open_brace_ignored() {
            // `{` inside content doesn't nest — first `}` wins
            assert_eq!(find_closing_brace("a{b}"), Some(3));
        }

        #[test]
        fn unclosed_quote() {
            assert_eq!(find_closing_brace(r#""no close"#), None);
        }

        #[test]
        fn unclosed_quote_with_brace() {
            // brace inside unclosed quote — never leaves quote mode
            assert_eq!(find_closing_brace(r#""}"#), None);
        }

        #[test]
        fn brace_closing_multibyte() {
            assert_eq!(find_closing_brace("ñ=ü}"), Some(5));
        }

        #[test]
        fn quoted_multibyte_brace_ignored() {
            // `}` inside quoted multibyte string — skipped
            assert_eq!(find_closing_brace(r#"κευ="λ}μ"}"#), Some(14));
        }
    }

    mod split_attrs_remaining {
        use super::*;

        #[test]
        fn empty() {
            assert_eq!(split_attrs_remaining(""), ("", ""));
        }

        #[test]
        fn no_attr_block() {
            assert_eq!(split_attrs_remaining("text"), ("", "text"));
        }

        #[test]
        fn unclosed_brace() {
            assert_eq!(split_attrs_remaining("{"), ("", "{"));
        }

        #[test]
        fn lone_close_brace() {
            assert_eq!(split_attrs_remaining("}"), ("", "}"));
        }

        #[test]
        fn single_empty_block() {
            assert_eq!(split_attrs_remaining("{}"), ("{}", ""));
        }

        #[test]
        fn two_adjacent_blocks() {
            assert_eq!(split_attrs_remaining("{}{}"), ("{}{}", ""));
        }

        #[test]
        fn two_blocks_space_separated() {
            assert_eq!(split_attrs_remaining("{} {}"), ("{}", " {}"));
        }

        #[test]
        fn block_then_text() {
            assert_eq!(split_attrs_remaining("{} text"), ("{}", " text"));
        }

        #[test]
        fn block_with_content() {
            assert_eq!(split_attrs_remaining("{a}rest"), ("{a}", "rest"));
        }

        #[test]
        fn multiple_blocks_then_text() {
            assert_eq!(split_attrs_remaining("{a}{b} rest"), ("{a}{b}", " rest"));
        }

        #[test]
        fn block_with_quoted_brace() {
            assert_eq!(split_attrs_remaining(r#"{key="}"} rest"#), (r#"{key="}"}"#, " rest"));
        }

        #[test]
        fn block_with_multibyte_key() {
            assert_eq!(split_attrs_remaining("{café=1}rest"), ("{café=1}", "rest"));
        }

        #[test]
        fn block_with_multibyte_value() {
            assert_eq!(split_attrs_remaining("{k=über}rest"), ("{k=über}", "rest"));
        }

        #[test]
        fn block_with_quoted_multibyte_value() {
            assert_eq!(split_attrs_remaining(r#"{k="日本語"}"#), (r#"{k="日本語"}"#, ""));
        }

        #[test]
        fn block_with_emoji_key_and_value() {
            assert_eq!(split_attrs_remaining("{🔥=🎉}tail"), ("{🔥=🎉}", "tail"));
        }

        #[test]
        fn two_adjacent_multibyte_blocks() {
            assert_eq!(split_attrs_remaining("{ä=1}{ö=2}end"), ("{ä=1}{ö=2}", "end"));
        }

        #[test]
        fn multibyte_after_block() {
            assert_eq!(split_attrs_remaining("{x=1}日本語"), ("{x=1}", "日本語"));
        }

        #[test]
        fn quoted_multibyte_with_escaped_quote() {
            assert_eq!(
                split_attrs_remaining(r#"{k="café\"bar"}z"#),
                (r#"{k="café\"bar"}"#, "z")
            );
        }
    }

    mod parse_jotdown {
        use super::*;

        // --- Empty / whitespace ---

        #[test]
        fn empty_string() {
            assert_eq!(parse_jotdown(""), Vec::<(String, String)>::new());
        }

        #[test]
        fn only_whitespace() {
            assert_eq!(parse_jotdown("   "), Vec::<(String, String)>::new());
        }

        #[test]
        fn only_newlines_and_tabs() {
            assert_eq!(parse_jotdown("\n\t\n"), Vec::<(String, String)>::new());
        }

        // --- Braces without content ---

        #[test]
        fn empty_braces() {
            assert_eq!(parse_jotdown("{}"), Vec::<(String, String)>::new());
        }

        #[test]
        fn braces_with_only_whitespace() {
            assert_eq!(parse_jotdown("{  }"), Vec::<(String, String)>::new());
        }

        // --- Malformed braces ---

        #[test]
        fn only_opening_brace() {
            // jotdown should fail to parse -> empty vec
            assert_eq!(parse_jotdown("{"), Vec::<(String, String)>::new());
        }

        #[test]
        fn only_closing_brace() {
            // auto-wrapped to "{}}") -> likely parse error
            assert_eq!(parse_jotdown("}"), Vec::<(String, String)>::new());
        }

        #[test]
        fn opening_brace_with_content_no_close() {
            assert_eq!(parse_jotdown("{.foo"), Vec::<(String, String)>::new());
        }

        #[test]
        fn nested_braces() {
            // "{{{.foo}}}" or similar — undefined behavior, just assert no panic
            let _ = parse_jotdown("{{{.foo}}}");
        }

        #[test]
        fn reversed_braces() {
            assert_eq!(parse_jotdown("}{"), Vec::<(String, String)>::new());
        }

        // --- No attribute markers ---

        #[test]
        fn plain_text_no_markers() {
            // "hello" -> wrapped to "{hello}" -> jotdown may or may not parse
            let result = parse_jotdown("hello");
            // assert based on actual jotdown behavior — likely empty or key-value
            let _ = result; // at minimum: no panic
        }

        #[test]
        fn plain_text_in_braces() {
            let result = parse_jotdown("{hello}");
            let _ = result;
        }

        // --- Special characters ---

        #[test]
        fn unicode_input() {
            let _ = parse_jotdown(".émoji-🎉");
        }

        #[test]
        fn null_byte() {
            let _ = parse_jotdown(".foo\0bar");
        }

        #[test]
        fn only_dots_and_hashes() {
            let _ = parse_jotdown(". #");
        }

        #[test]
        fn dot_without_value() {
            let _ = parse_jotdown(".");
        }

        #[test]
        fn hash_without_value() {
            let _ = parse_jotdown("#");
        }

        // --- Whitespace handling around auto-wrap ---

        #[test]
        fn leading_trailing_whitespace_no_braces() {
            // trimmed first, then wrapped — should behave same as without whitespace
            let a = parse_jotdown("  .foo  ");
            let b = parse_jotdown(".foo");
            assert_eq!(a, b);
        }

        #[test]
        fn leading_trailing_whitespace_with_braces() {
            // trimmed to "{.foo}", parsed directly
            let a = parse_jotdown("  {.foo}  ");
            let b = parse_jotdown("{.foo}");
            assert_eq!(a, b);
        }

        // --- Brace detection edge cases ---

        // #[test]
        // fn starts_with_brace_but_invalid() {
        //     // starts_with('{') is true, so NOT auto-wrapped — passed as-is
        //     assert_eq!(parse_jotdown("{.foo}{.bar}"), Vec::<(String, String)>::new());
        //     // or could parse first block — depends on jotdown
        // }

        #[test]
        fn brace_in_middle_not_start() {
            // doesn't start with '{', so auto-wrapped: "{something {.foo}}"
            let _ = parse_jotdown("something {.foo}");
        }
    }

    mod scan_leading_attrs {
        use super::*;

        fn create_test_parser() -> MarkdownIt {
            let mut md = MarkdownIt::new();
            cmark::add(&mut md);
            md
        }

        static TEST_PARSER: LazyLock<MarkdownIt> = LazyLock::new(create_test_parser);

        fn parse_paragraph_children(content: &str) -> Vec<Node> {
            let mut ast = TEST_PARSER.parse(content);
            let para = ast
                .children
                .iter_mut()
                .find(|n| n.cast::<Paragraph>().is_some())
                .expect("no paragraph found");
            std::mem::take(&mut para.children)
        }

        #[test]
        fn plain_text() {
            let children = parse_paragraph_children("text\n");
            assert!(scan_leading_attrs(&children).is_none());
        }

        #[test]
        fn hardbreak() {
            let children = parse_paragraph_children("\\");
            assert!(scan_leading_attrs(&children).is_none());
        }

        #[test]
        fn attr_with_hardbreak() {
            let children = parse_paragraph_children("{.a}\\");
            assert!(scan_leading_attrs(&children).is_none());
        }

        #[test]
        fn attr_with_remaining_text() {
            let children = parse_paragraph_children("{.a}remaining\n");
            assert!(scan_leading_attrs(&children).is_none());
        }

        #[test]
        fn attr_with_remaining_text_whitespace() {
            let children = parse_paragraph_children("{.a} remaining\n");
            assert!(scan_leading_attrs(&children).is_none());
        }

        #[test]
        fn single_attr() {
            // TA
            let children = parse_paragraph_children("{.a}");
            let (attrs, end_idx) = scan_leading_attrs(&children).unwrap();
            assert_eq!(attrs, "{.a}");
            assert_eq!(end_idx, 1);
        }

        #[test]
        fn two_attrs() {
            // TA (SB TA)
            let children = parse_paragraph_children("{.a}\n{.b}");
            let (attrs, end_idx) = scan_leading_attrs(&children).unwrap();
            assert_eq!(attrs, "{.a}{.b}");
            assert_eq!(end_idx, 3);
        }

        #[test]
        fn three_attrs() {
            // TA (SB TA) (SB TA)
            let children = parse_paragraph_children("{.a}\n{.b}\n{.c}");
            let (attrs, end_idx) = scan_leading_attrs(&children).unwrap();
            assert_eq!(attrs, "{.a}{.b}{.c}");
            assert_eq!(end_idx, 5);
        }

        #[test]
        fn single_attr_text() {
            // TA SB T
            let children = parse_paragraph_children("{.a}\ntext");
            let (attrs, end_idx) = scan_leading_attrs(&children).unwrap();
            assert_eq!(attrs, "{.a}");
            assert_eq!(end_idx, 1);
        }

        #[test]
        fn two_attrs_text() {
            // TA (SB TA) SB T
            let children = parse_paragraph_children("{.a}\n{.b}\ntext");
            let (attrs, end_idx) = scan_leading_attrs(&children).unwrap();
            assert_eq!(attrs, "{.a}{.b}");
            assert_eq!(end_idx, 3);
        }

        #[test]
        fn three_attrs_text() {
            // TA (SB TA) (SB TA) SB T
            let children = parse_paragraph_children("{.a}\n{.b}\n{.c}\ntext");
            let (attrs, end_idx) = scan_leading_attrs(&children).unwrap();
            assert_eq!(attrs, "{.a}{.b}{.c}");
            assert_eq!(end_idx, 5);
        }
    }

    mod parse_and_render_tests {
        use super::*;

        fn create_test_parser() -> MarkdownIt {
            let mut md = MarkdownIt::new();
            cmark::add(&mut md);
            html::add(&mut md);
            super::add(&mut md);
            md
        }

        static TEST_PARSER: LazyLock<MarkdownIt> = LazyLock::new(create_test_parser);

        fn render(input: &str) -> String {
            let md = &*TEST_PARSER;
            md.parse(input).render()
        }

        mod dangling {
            use super::*;

            #[test]
            fn attrs_widow() {
                let html = render("{.a}");
                assert!(html.contains("<p>{.a}</p>"), "got: {html}");
            }

            #[test]
            fn attrs_widow_softbreak() {
                let html = render("{.a}\n");
                assert!(html.contains("<p>{.a}</p>"), "got: {html}");
            }

            #[test]
            fn attrs_widow_newline() {
                let html = render("{.a}\n\n");
                assert!(html.contains("<p>{.a}</p>"), "got: {html}");
            }

            #[test]
            fn attrs_widow_hardbreak() {
                let html = render("{.a}\\\ntext");
                assert!(html.contains("<p>{.a}<br>\ntext</p>"), "got: {html}");
            }

            #[test]
            fn attrs_orphan() {
                let html = render("\n\n{.a}");
                assert!(html.contains("<p>{.a}</p>"), "got: {html}");
            }

            #[test]
            fn attrs_orphan_softbreak() {
                let html = render("\n\n{.a}\n");
                assert!(html.contains("<p>{.a}</p>"), "got: {html}");
            }

            #[test]
            fn attrs_orphan_newline() {
                let html = render("\n\n{.a}\n\n");
                assert!(html.contains("<p>{.a}</p>"), "got: {html}");
            }

            #[test]
            fn attrs_orphan_hardbreak() {
                let html = render("\n\n{.a}\\\ntext");
                assert!(html.contains("<p>{.a}<br>\ntext</p>"), "got: {html}");
            }
        }

        mod leaf_block_tests {
            use super::*;

            mod attached {
                use super::*;

                // Thematic breaks

                #[test]
                fn hr_stars() {
                    let html = render("{.a}\n***");
                    assert!(html.contains(r#"<hr class="a">"#), "got: {html}");
                }

                #[test]
                fn hr_dashes() {
                    let html = render("{.a}\n---");
                    assert!(html.contains("<h2>{.a}</h2>"), "got: {html}");
                    assert!(!html.contains(r#"class="a"#), "got: {html}");
                }

                #[test]
                fn hr_underscores() {
                    let html = render("{.a}\n___");
                    assert!(html.contains(r#"<hr class="a">"#), "got: {html}");
                }

                // ATX headings

                #[test]
                fn atx_heading() {
                    let html = render("{.a}\n# foo");
                    assert!(html.contains(r#"<h1 class="a">foo</h1>"#), "got: {html}");
                }

                // Setext headings

                #[test]
                fn settext_heading_equal_signs() {
                    let html = render("{.a}\nfoo\n===");
                    assert!(html.contains("<h1>{.a}\nfoo</h1>"), "got: {html}");
                    assert!(!html.contains(r#"class="a"#), "got: {html}");
                }

                #[test]
                fn settext_heading_dashes() {
                    let html = render("{.a}\nfoo\n---");
                    assert!(html.contains("<h2>{.a}\nfoo</h2>"), "got: {html}");
                    assert!(!html.contains(r#"class="a"#), "got: {html}");
                }

                // Indented code blocks
                // Fenced code blocks

                // HTML blocks

                #[test]
                fn div() {
                    let html = render("{.a}\n<div></div>");
                    assert!(html.contains("<p>{.a}</p>\n<div></div>"), "got: {html}");
                    assert!(!html.contains(r#"class="a"#), "got: {html}");
                }

                // Paragraphs

                #[test]
                fn paragraph() {
                    let html = render("{.a}\ntext");
                    assert!(html.contains(r#"<p class="a">text</p>"#), "got: {html}");
                }
            }

            mod detached {
                use super::*;

                // Thematic breaks

                #[test]
                fn hr_stars() {
                    let html = render("{.a}\n\n***");
                    assert!(html.contains(r#"<hr class="a">"#), "got: {html}");
                }

                #[test]
                fn hr_dashes() {
                    let html = render("{.a}\n\n---");
                    assert!(html.contains(r#"<hr class="a">"#), "got: {html}");
                }

                #[test]
                fn hr_underscores() {
                    let html = render("{.a}\n\n___");
                    assert!(html.contains(r#"<hr class="a">"#), "got: {html}");
                }

                // ATX headings

                #[test]
                fn atx_heading() {
                    let html = render("{.a}\n\n# foo");
                    assert!(html.contains(r#"<h1 class="a">foo</h1>"#), "got: {html}");
                }

                // Setext headings

                #[test]
                fn settext_heading_equal_signs() {
                    let html = render("{.a}\n\nfoo\n===");
                    assert!(html.contains(r#"<h1 class="a">foo</h1>"#), "got: {html}");
                }

                #[test]
                fn settext_heading_dashes() {
                    let html = render("{.a}\n\nfoo\n---");
                    assert!(html.contains(r#"<h2 class="a">foo</h2>"#), "got: {html}");
                }

                // Indented code blocks
                // Fenced code blocks
                // HTML blocks

                #[test]
                fn div() {
                    let html = render("{.a}\n\n<div></div>");
                    assert!(html.contains("<p>{.a}</p>\n<div></div>"), "got: {html}");
                    assert!(!html.contains(r#"class="a"#), "got: {html}");
                }

                // Paragraphs

                #[test]
                fn paragraph() {
                    let html = render("{.a}\n\ntext");
                    assert!(html.contains(r#"<p class="a">text</p>"#), "got: {html}");
                }
            }
        }

        mod container_block_test {
            use super::*;

            mod blockquote_tests {
                use super::*;

                #[test]
                fn blockquote_container() {
                    let html = render("{.a}\n>");
                    assert!(html.contains(r#"<blockquote class="a">"#), "got: {html}");
                }

                #[test]
                fn blockquote_conatiner_and_inner() {
                    let html = render("{.a}\n>{.b}\n># Title");
                    assert!(html.contains(r#"<blockquote class="a">"#), "got: {html}");
                    assert!(html.contains(r#"<h1 class="b">Title</h1>"#), "got: {html}");
                }

                #[test]
                fn blockquote_inner() {
                    let html = render(">{.a}\n># Title");
                    assert!(html.contains(r#"<h1 class="a">Title</h1>"#), "got: {html}");
                }

                #[test]
                fn blockquote_inner_empty() {
                    let html = render(">{.a}\n>\n># Title");
                    assert!(html.contains(r#"<h1 class="a">Title</h1>"#), "got: {html}");
                }

                #[test]
                fn blockquote_inner_many_empty() {
                    let html = render(">{.a}\n>\n>\n># Title");
                    assert!(html.contains(r#"<h1 class="a">Title</h1>"#), "got: {html}");
                }

                #[test]
                fn blockquote_inner_no_lazyness() {
                    let html = render(">{.a}\n# Title");
                    assert!(html.contains("<h1>Title</h1>"), "got: {html}");
                    assert!(!html.contains(r#"class="a"#), "got: {html}");
                }

                #[test]
                fn blockquote_container_detached() {
                    let html = render("{.a}\n\n>");
                    assert!(html.contains(r#"<blockquote class="a">"#), "got: {html}");
                }

                #[test]
                fn blockquote_conatiner_and_inner_detached() {
                    let html = render("{.a}\n\n>{.b}\n># Title");
                    assert!(html.contains(r#"<blockquote class="a">"#), "got: {html}");
                    assert!(html.contains(r#"<h1 class="b">Title</h1>"#), "got: {html}");
                }
            }

            mod listitem_tests {
                use super::*;

                #[test]
                fn attr_only() {
                    let html = render("- {.a}");
                    assert!(html.contains(r#"<li class="a"></li>"#), "got: {html}");
                }

                #[test]
                fn attr_blank_line_only() {
                    let html = render("-\n  {.a}");
                    assert!(html.contains(r#"<li class="a"></li>"#), "got: {html}");
                }

                #[test]
                fn attr_lazy() {
                    let html = render("- {.a}\nitem");
                    assert!(html.contains(r#"<li class="a">item</li>"#), "got: {html}");
                }

                #[test]
                fn attr_indented() {
                    let html = render("- {.a}\n  item");
                    assert!(html.contains(r#"<li class="a">item</li>"#), "got: {html}");
                }

                #[test]
                fn attr_blank_line_lazy() {
                    let html = render("-\n  {.a}\nitem");
                    assert!(html.contains(r#"<li class="a">item</li>"#), "got: {html}");
                }

                #[test]
                fn attr_blank_line_indented() {
                    let html = render("-\n  {.a}\n  item");
                    assert!(html.contains(r#"<li class="a">item</li>"#), "got: {html}");
                }

                ////////////////////////////

                #[test]
                fn attr_lazy_more() {
                    let html = render("- {.a}\nitem\n\n  more");
                    assert!(html.contains("<li>"), "got: {html}");
                    assert!(html.contains(r#"<p class="a">item</p>"#), "got: {html}");
                    assert!(html.contains("<p>more</p>"), "got: {html}");
                }

                #[test]
                fn attr_indented_more() {
                    let html = render("- {.a}\n  item\n\n  more");
                    assert!(html.contains("<li>"), "got: {html}");
                    assert!(html.contains(r#"<p class="a">item</p>"#), "got: {html}");
                    assert!(html.contains("<p>more</p>"), "got: {html}");
                }

                #[test]
                fn attr_blank_line_lazy_more() {
                    let html = render("-\n  {.a}\nitem\n\n  more");
                    assert!(html.contains("<li>"), "got: {html}");
                    assert!(html.contains(r#"<p class="a">item</p>"#), "got: {html}");
                    assert!(html.contains("<p>more</p>"), "got: {html}");
                }

                #[test]
                fn attr_blank_line_indented_more() {
                    let html = render("-\n  {.a}\n  item\n\n  more");
                    assert!(html.contains("<li>"), "got: {html}");
                    assert!(html.contains(r#"<p class="a">item</p>"#), "got: {html}");
                    assert!(html.contains("<p>more</p>"), "got: {html}");
                }

                ////////////////////////////

                #[test]
                fn attr_paragraph() {
                    let html = render("- {.a}\n\n  item");
                    assert!(html.contains(r#"<li class="a">"#), "got: {html}");
                    assert!(html.contains("<p>item</p>"), "got: {html}");
                }

                #[test]
                fn attr_blank_line_paragraph() {
                    let html = render("-\n  {.a}\n\n  item");
                    assert!(html.contains(r#"<li class="a">"#), "got: {html}");
                    assert!(html.contains("<p>item</p>"), "got: {html}");
                }

                #[test]
                fn attr_paragraph_attrs() {
                    let html = render("- {.a}\n\n  {.b}\n  item");
                    assert!(html.contains(r#"<li class="a">"#), "got: {html}");
                    assert!(html.contains(r#"<p class="b">item</p>"#), "got: {html}");
                }

                #[test]
                fn attr_blank_line_paragraph_attrs() {
                    let html = render("-\n  {.a}\n\n  {.b}\n  item");
                    assert!(html.contains(r#"<li class="a">"#), "got: {html}");
                    assert!(html.contains(r#"<p class="b">item</p>"#), "got: {html}");
                }
            }

            mod list_tests {
                use super::*;

                #[test]
                fn list_container_attached() {
                    let html = render("{.a}\n- item>");
                    assert!(html.contains(r#"<ul class="a">"#), "got: {html}");
                }

                #[test]
                fn list_container_detached() {
                    let html = render("{.a}\n\n- item>");
                    assert!(html.contains(r#"<ul class="a">"#), "got: {html}");
                }
            }
        }
    }
}
