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
