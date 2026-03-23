use std::path::PathBuf;

use markdown_it::MarkdownIt;

pub mod icon;
pub mod span;
pub mod sup_sub;

pub fn add(md: &mut MarkdownIt, icon_dirs: Vec<PathBuf>) {
    icon::add(md, icon_dirs);
    span::add(md);
    sup_sub::add(md);
}
