//! Span
//!
//! looks like `|this|`
//!
//! <https://spec.commonmark.org/0.30/#emphasis-and-strong-emphasis>
use markdown_it::generics::inline::emph_pair;
use markdown_it::{MarkdownIt, Node, NodeValue, Renderer};

pub fn add(md: &mut MarkdownIt) {
    emph_pair::add_with::<'|', 1, true>(md, || Node::new(Span { marker: '|' }));
}

#[derive(Debug)]
pub struct Span {
    #[allow(dead_code)]
    pub marker: char,
}

impl NodeValue for Span {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        fmt.open("span", &node.attrs);
        fmt.contents(&node.children);
        fmt.close("span");
    }
}
