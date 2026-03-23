use markdown_it::MarkdownIt;

pub mod attrs;

pub fn add(md: &mut MarkdownIt) {
    attrs::add(md);
}
