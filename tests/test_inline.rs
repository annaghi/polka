use std::sync::LazyLock;

use markdown_it::MarkdownIt;
use markdown_it::plugins::{cmark, extra};

fn create_test_parser() -> MarkdownIt {
    let mut md = MarkdownIt::new();
    cmark::add(&mut md);
    // Markdown Extensions
    // Don't enable extra::typographer, it replaces ASCII dashes in Text nodes,
    // breaking CSS identifiers like `.btn--primary`.
    // extra::typographer::add(&mut md);
    extra::beautify_links::add(&mut md);
    // extra::heading_anchors::add(&mut md, slugify);
    extra::linkify::add(&mut md);
    // Don't enable extra::smartquotes, it replaces ASCII quotes in Text nodes,
    // breaking CSS attribute values like `data-attr="value"`.
    // extra::smartquotes::add(&mut md);
    extra::strikethrough::add(&mut md);
    extra::tables::add(&mut md);
    polka::add(&mut md, Vec::new());
    md
}

static TEST_PARSER: LazyLock<MarkdownIt> = LazyLock::new(create_test_parser);

fn render(input: &str) -> String {
    let md = &*TEST_PARSER;
    md.parse(input).render()
}

// These tests were mainly taken from jotdown/src/test_inline.rs

#[test]
fn str() {
    assert_eq!(render("abc"), "<p>abc</p>\n");
    assert_eq!(render("abc def"), "<p>abc def</p>\n");
}

#[test]
fn verbatim() {
    assert_eq!(render("`abc`"), "<p><code>abc</code></p>\n");
    assert_eq!(render("`abc\ndef`"), "<p><code>abc def</code></p>\n");
    assert_eq!(render("`abc&def`"), "<p><code>abc&amp;def</code></p>\n");
    assert_eq!(render("`abc"), "<p>`abc</p>\n");
    assert_eq!(render("``abc``"), "<p><code>abc</code></p>\n");
    assert_eq!(render("abc `def`"), "<p>abc <code>def</code></p>\n");
    assert_eq!(render("abc`def`"), "<p>abc<code>def</code></p>\n");
}

#[test]
fn hard_break() {
    assert_eq!(render("abc\\\ndef"), "<p>abc<br>\ndef</p>\n");
    assert_eq!(render("abc\\"), "<p>abc\\</p>\n");
    assert_eq!(render("abc\\\n"), "<p>abc\\</p>\n");
}

#[test]
fn verbatim_attr() {
    assert_eq!(
        render("pre `raw`{#id} post"),
        r#"<p>pre <code id="id">raw</code> post</p>
"#
    );
}

#[test]
fn verbatim_attr_inside() {
    assert_eq!(render("`a{i=0}`"), "<p><code>a{i=0}</code></p>\n");
    // skip inline math
}

#[test]
fn verbatim_whitespace() {
    assert_eq!(render("`  `"), "<p><code>  </code></p>\n");
    assert_eq!(render("` abc `"), "<p><code>abc</code></p>\n");
}

#[test]
fn verbatim_trim() {
    assert_eq!(render("` ``abc`` `"), "<p><code>``abc``</code></p>\n");
}

#[test]
fn math() {
    // skip inline math
}

#[test]
fn span() {
    assert_eq!(render("|text|"), "<p><span>text</span></p>\n");
    assert_eq!(render("before|text|after"), "<p>before<span>text</span>after</p>\n");
    assert_eq!(render("before |text| after"), "<p>before <span>text</span> after</p>\n");
}

#[test]
fn span_nested() {
    assert_eq!(render("||text||"), "<p><span><span>text</span></span></p>\n");
    assert_eq!(render("|some *text*|"), "<p><span>some <em>text</em></span></p>\n");
}

#[test]
fn span_marker() {
    assert_eq!(render("|"), "<p>|</p>\n");
    assert_eq!(render("||"), "<p>||</p>\n");
    assert_eq!(render("|||"), "<p>|||</p>\n");
    assert_eq!(render("||||"), "<p>||||</p>\n");
}

#[test]
fn span_attr() {
    assert_eq!(render("|abc|{.def}"), "<p><span class=\"def\">abc</span></p>\n");
    assert_eq!(render("||{.cls}"), "<p>||{.cls}</p>\n");
    assert_eq!(
        render("not |attached| {#id}."),
        "<p>not <span>attached</span> {#id}.</p>\n"
    );
}

#[test]
fn span_attr_cont() {
    assert_eq!(
        render("|x_y|{.bar_}"),
        r#"<p><span class="bar_">x_y</span></p>
"#
    );
}

#[test]
fn container_marker() {
    assert_eq!(render("{}"), "<p>{}</p>\n");
    assert_eq!(render("{inner}"), "<p>{inner}</p>\n");
    assert_eq!(render("*abc*{}"), "<p><em>abc</em></p>\n");
}

#[test]
fn container_basic() {
    assert_eq!(render("_abc_"), "<p><em>abc</em></p>\n");
    assert_eq!(render("{_abc_}"), "<p>{<em>abc</em>}</p>\n");
    assert_eq!(render("{_{_abc_}_}"), "<p>{<em>{<em>abc</em>}</em>}</p>\n");
}

#[test]
fn container_unclosed_attr() {
    assert_eq!(render("^.^{unclosed"), "<p><sup>.</sup>{unclosed</p>\n");
}

#[test]
fn verbatim_unclosed_attr() {
    assert_eq!(render("`.`{unclosed"), "<p><code>.</code>{unclosed</p>\n");
}

#[test]
fn container_unopened() {
    assert_eq!(render("*}abc"), "<p>*}abc</p>\n");
}

#[test]
fn container_close_block() {
    assert_eq!(render("{_abc"), "<p>{_abc</p>\n");
    assert_eq!(render("{_{*{_abc"), "<p>{_{*{_abc</p>\n");
}

#[test]
fn container_attr() {
    assert_eq!(
        render("_abc def_{.attr}"),
        r#"<p><em class="attr">abc def</em></p>
"#
    );
}

#[test]
fn container_attr_empty() {
    assert_eq!(render("_abc def_{}"), "<p><em>abc def</em></p>\n");
    assert_eq!(render("_abc def_{ % comment % } ghi"), "<p><em>abc def</em> ghi</p>\n");
}

#[test]
fn container_attr_multiple() {
    assert_eq!(
        render("_abc def_{}{}{.a}{}{.b}"),
        r#"<p><em class="a b">abc def</em></p>
"#
    );
    assert_eq!(
        render("_abc def_{.a}{.b}{.c} {.d}"),
        r#"<p><em class="a b c">abc def</em> {.d}</p>
"#
    );
}

#[test]
fn attr() {
    assert_eq!(render("word{a=b}"), "<p>word{a=b}</p>\n");
    assert_eq!(
        render("some word{.a}{.b} with attrs"),
        "<p>some word{.a}{.b} with attrs</p>\n"
    );
}

// The test is disabled because our attribute parser runs as a postprocessor,
// after the markdown parser has already processed the input.
// By that point, the backticks inside the attribute value have already been
// interpreted as inline code spans, so the attribute parser never sees
// the raw {a="`verb`"} string intact.
// The markdown parser consumes the backticks first, breaking the attribute
// syntax before our postprocessor gets a chance to parse it.
// #[test]
// fn attr_quoted() {
//     assert_eq!(render(r#"word{a="`verb`"}"#), r#"<p>word{a="`verb`"}</p>\n"#);
// }

#[test]
fn attr_whitespace() {
    assert_eq!(render("word {%comment%}"), "<p>word {%comment%}</p>\n");
    assert_eq!(render("word {%comment%} word"), "<p>word {%comment%} word</p>\n");
    assert_eq!(render("word {a=b}"), "<p>word {a=b}</p>\n");
    assert_eq!(render(" {a=b}"), "<p>{a=b}</p>\n");
}

#[test]
fn attr_start() {
    assert_eq!(render("{a=b} word"), "<p>{a=b} word</p>\n");
}

#[test]
fn attr_empty() {
    assert_eq!(render("word{}"), "<p>word{}</p>\n");
    assert_eq!(
        render("word{ % comment % } trail"),
        "<p>word{ % comment % } trail</p>\n"
    );
}

#[test]
fn quote() {
    assert_eq!(render("'a'"), "<p>'a'</p>\n");
    assert_eq!(render(" 'a' "), "<p>'a'</p>\n");
}

#[test]
fn quote_attr() {
    assert_eq!(render("'a'{.b}"), "<p>'a'{.b}</p>\n");
}
