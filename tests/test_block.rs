use std::sync::LazyLock;

use markdown_it::MarkdownIt;
use markdown_it::plugins::{cmark, extra};

fn create_test_parser() -> MarkdownIt {
    let mut md = MarkdownIt::new();
    cmark::add(&mut md);
    extra::add(&mut md);
    polka::add(&mut md, Vec::new());
    md
}

static TEST_PARSER: LazyLock<MarkdownIt> = LazyLock::new(create_test_parser);

fn render(input: &str) -> String {
    let md = &*TEST_PARSER;
    md.parse(input).render()
}

#[test]
fn parse_attr() {
    assert_eq!(
        render("{.a}\npara\n"),
        r#"<p class="a">para</p>
"#
    );
    assert_eq!(
        render("{.a}\n\n{.b}\n\npara\n"),
        r#"<p>{.a}</p>
<p class="b">para</p>
"#
    );
}

#[test]
fn parse_attr_multiple() {
    assert_eq!(
        render("{.a}{.b}\npara\n"),
        r#"<p class="a b">para</p>
"#
    );
    assert_eq!(
        render("{.a}{.b}\n\npara\n"),
        r#"<p class="a b">para</p>
"#
    );
    assert_eq!(
        render("{.a}{.b}\n{.c}\npara\n"),
        r#"<p class="a b c">para</p>
"#
    );
    assert_eq!(
        render("{.a}{.b}\n{.c}\n\npara\n"),
        r#"<p class="a b c">para</p>
"#
    );
}

#[test]
fn listitem_attr_empty_item() {
    assert_eq!(
        render("- {.a}"),
        r#"<ul>
<li class="a"></li>
</ul>
"#
    );
    assert_eq!(
        render("-\n  {.b}"),
        r#"<ul>
<li class="b"></li>
</ul>
"#
    );
}

#[test]
fn listitem_attr_with_text() {
    assert_eq!(
        render("- {.c}\nitem c"),
        r#"<ul>
<li class="c">item c</li>
</ul>
"#
    );
    assert_eq!(
        render("- {.d}\n  item d"),
        r#"<ul>
<li class="d">item d</li>
</ul>
"#
    );
    assert_eq!(
        render("-\n  {.e}\nitem e"),
        r#"<ul>
<li class="e">item e</li>
</ul>
"#
    );
    assert_eq!(
        render("-\n  {.f}\n  item f"),
        r#"<ul>
<li class="f">item f</li>
</ul>
"#
    );
}

#[test]
fn listitem_attr_multiple_classes() {
    assert_eq!(
        render("- {.x}\n  {.xx}"),
        r#"<ul>
<li class="x xx"></li>
</ul>
"#
    );
    assert_eq!(
        render("-\n  {.y}\n  {.yy}"),
        r#"<ul>
<li class="y yy"></li>
</ul>
"#
    );
    assert_eq!(
        render("- {.z}\n{.zz}\nitem z zz"),
        r#"<ul>
<li class="z zz">item z zz</li>
</ul>
"#
    );
    assert_eq!(
        render("- {.u}\n  {.uu}\n  item u uu"),
        r#"<ul>
<li class="u uu">item u uu</li>
</ul>
"#
    );
    assert_eq!(
        render("-\n  {.v}\n  {.vv}\nitem v vv"),
        r#"<ul>
<li class="v vv">item v vv</li>
</ul>
"#
    );
    assert_eq!(
        render("-\n  {.w}\n  {.ww}\n  item w ww"),
        r#"<ul>
<li class="w ww">item w ww</li>
</ul>
"#
    );
}

#[test]
fn listitem_attr_loose_para() {
    assert_eq!(
        render("- {.aa}\nitem aa\n\n  more aa"),
        r#"<ul>
<li>
<p class="aa">item aa</p>
<p>more aa</p>
</li>
</ul>
"#
    );
    assert_eq!(
        render("- {.bb}\n  item bb\n\n  more bb"),
        r#"<ul>
<li>
<p class="bb">item bb</p>
<p>more bb</p>
</li>
</ul>
"#
    );
    assert_eq!(
        render("-\n  {.cc}\nitem cc\n\n  more cc"),
        r#"<ul>
<li>
<p class="cc">item cc</p>
<p>more cc</p>
</li>
</ul>
"#
    );
    assert_eq!(
        render("-\n  {.dd}\n  item dd\n\n  more dd"),
        r#"<ul>
<li>
<p class="dd">item dd</p>
<p>more dd</p>
</li>
</ul>
"#
    );
}

#[test]
fn listitem_attr_loose_detached() {
    assert_eq!(
        render("- {.aaa}\n\n  item aaa"),
        r#"<ul>
<li class="aaa">
<p>item aaa</p>
</li>
</ul>
"#
    );
    assert_eq!(
        render("-\n  {.bbb}\n\n  item bbb"),
        r#"<ul>
<li class="bbb">
<p>item bbb</p>
</li>
</ul>
"#
    );
}

#[test]
fn listitem_attr_loose_detached_with_para_attr() {
    assert_eq!(
        render("- {.ccc}\n\n  {.c}\n  item ccc c"),
        r#"<ul>
<li class="ccc">
<p class="c">item ccc c</p>
</li>
</ul>
"#
    );
    assert_eq!(
        render("-\n  {.ddd}\n\n  {.d}\n  item ddd d"),
        r#"<ul>
<li class="ddd">
<p class="d">item ddd d</p>
</li>
</ul>
"#
    );
    assert_eq!(
        render("- {.eee}\n\n  {.e}\n\n  item eee e"),
        r#"<ul>
<li class="eee">
<p class="e">item eee e</p>
</li>
</ul>
"#
    );
    assert_eq!(
        render("-\n  {.fff}\n\n  {.f}\n\n  item fff f"),
        r#"<ul>
<li class="fff">
<p class="f">item fff f</p>
</li>
</ul>
"#
    );
}

#[test]
fn list_attr_on_list() {
    assert_eq!(
        render("{.cls}\n- a\n- b"),
        r#"<ul class="cls">
<li>a</li>
<li>b</li>
</ul>
"#
    );
}

#[test]
fn list_attr_on_list_ordered() {
    assert_eq!(
        render("{.cls}\n1. a\n2. b"),
        r#"<ol class="cls">
<li>a</li>
<li>b</li>
</ol>
"#
    );
}

#[test]
fn list_attr_on_list_and_item() {
    assert_eq!(
        render("{.list}\n- {.item}\n  text"),
        r#"<ul class="list">
<li class="item">text</li>
</ul>
"#
    );
}

#[test]
fn list_attr_multiple_items() {
    assert_eq!(
        render("- {.a}\n  one\n- {.b}\n  two"),
        r#"<ul>
<li class="a">one</li>
<li class="b">two</li>
</ul>
"#
    );
}

#[test]
fn list_attr_on_list_with_blank() {
    assert_eq!(
        render("{.cls}\n\n- a\n- b"),
        r#"<ul class="cls">
<li>a</li>
<li>b</li>
</ul>
"#
    );
}

#[test]
fn blockquote_basic() {
    assert_eq!(render("> hello"), "<blockquote><p>hello</p></blockquote>");
}

#[test]
fn blockquote_attr() {
    assert_eq!(
        render("{.cls}\n> hello"),
        r#"<blockquote class="cls">
<p>hello</p>
</blockquote>
"#
    );
}

#[test]
fn blockquote_attr_with_blank() {
    assert_eq!(
        render("{.cls}\n\n> hello"),
        r#"<blockquote class="cls">
<p>hello</p>
</blockquote>
"#
    );
}

#[test]
fn blockquote_attr_on_inner_para() {
    assert_eq!(
        render("> {.cls}\n> hello"),
        r#"<blockquote>
<p class="cls">hello</p>
</blockquote>
"#
    );
}

#[test]
fn blockquote_attr_both() {
    assert_eq!(
        render("{.outer}\n> {.inner}\n> hello"),
        r#"<blockquote class="outer">
<p class="inner">hello</p>
</blockquote>
"#
    );
}

#[test]
fn blockquote_nested_attr() {
    assert_eq!(
        render("{.outer}\n> {.inner}\n> > nested"),
        r#"<blockquote class="outer">
<blockquote class="inner">
<p>nested</p>
</blockquote>
</blockquote>
"#
    );
}

#[test]
fn blockquote_multiple_paras_attr() {
    assert_eq!(
        render("{.cls}\n> para one\n>\n> para two"),
        r#"<blockquote class="cls">
<p>para one</p>
<p>para two</p>
</blockquote>
"#
    );
}
