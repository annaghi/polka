
use super::*;
use markdown_it::MarkdownIt;

fn lint(input: &str) -> Vec<String> {
    let mut md = MarkdownIt::new();
    markdown_it::plugins::cmark::add(&mut md);
    markdown_it::plugins::html::add(&mut md);

    let root = md.parse(input);
    let errors = validate(&root);
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

        let root = md.parse(input);
        validate(&root)
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
        // validate_tag_nesting always produces Exact offsets
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
