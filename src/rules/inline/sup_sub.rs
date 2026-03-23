//! Superscript syntax (like `^this^`)
//! Subscript syntax (like `~this~`)
//!
use markdown_it::generics::inline::emph_pair;
use markdown_it::{MarkdownIt, Node, NodeValue, Renderer};

pub fn add(md: &mut MarkdownIt) {
    emph_pair::add_with::<'^', 1, true>(md, || Node::new(Superscript { marker: '^' }));
    emph_pair::add_with::<'~', 1, true>(md, || Node::new(Subscript { marker: '~' }));
}

#[derive(Debug)]
pub struct Superscript {
    #[allow(dead_code)]
    pub marker: char,
}

impl NodeValue for Superscript {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        fmt.open("sup", &node.attrs);
        fmt.contents(&node.children);
        fmt.close("sup");
    }
}

#[derive(Debug)]
pub struct Subscript {
    #[allow(dead_code)]
    pub marker: char,
}

impl NodeValue for Subscript {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        fmt.open("sub", &node.attrs);
        fmt.contents(&node.children);
        fmt.close("sub");
    }
}
