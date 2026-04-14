use markdown_it::MarkdownIt;

mod utils;
pub mod validator;

pub fn add(md: &mut MarkdownIt) {
    validator::add(md);
}
