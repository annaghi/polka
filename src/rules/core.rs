use markdown_it::MarkdownIt;

pub mod attrs;
pub mod utils;

pub fn add(md: &mut MarkdownIt) {
    attrs::add(md);
}
