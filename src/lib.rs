use std::cell::Cell;
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
    static DEBUG_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static DEBUG_DIR: Cell<Option<PathBuf>> = const { Cell::new(None) };
}

pub fn set_debug(active: bool, dir: PathBuf) {
    DEBUG_ACTIVE.set(active);
    DEBUG_DIR.set(Some(dir));
}

pub(crate) fn debug_write(output_filename: &str, content: &str) {
    if !DEBUG_ACTIVE.get() {
        return;
    }

    let Some(dir) = DEBUG_DIR.take() else { return };
    let _ = fs::create_dir_all(&dir);
    let _ = File::create(dir.join(output_filename)).and_then(|mut f| f.write_all(content.as_bytes()));

    DEBUG_DIR.set(Some(dir));
}
