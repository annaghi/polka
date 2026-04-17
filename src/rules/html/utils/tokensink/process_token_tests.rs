
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
