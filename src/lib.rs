use std::cell::RefCell;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use markdown_it::MarkdownIt;

mod intern;
#[allow(dead_code)]
mod jotdown_attr;

pub mod attrs;
pub mod icon;
pub mod span;
pub mod sup_sub;

pub fn add(md: &mut MarkdownIt, icon_dirs: Vec<PathBuf>) {
    icon::add(md, icon_dirs);
    span::add(md);
    sup_sub::add(md);
    attrs::add(md);
}

thread_local! {
    static DEBUG_DIR: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

pub fn set_debug(dir: Option<PathBuf>) {
    DEBUG_DIR.with(|d| *d.borrow_mut() = dir);
}

pub(crate) fn debug_write(filename: &str, content: &str) {
    DEBUG_DIR.with(|d| {
        if let Some(ref dir) = *d.borrow() {
            let _ = fs::create_dir_all(dir);
            let _ = File::create(dir.join(filename)).and_then(|mut f| f.write_all(content.as_bytes()));
        }
    });
}
