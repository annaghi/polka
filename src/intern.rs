use std::collections::HashSet;
use std::sync::LazyLock;
use std::sync::Mutex;

static INTERNED: LazyLock<Mutex<HashSet<&'static str>>> = LazyLock::new(Default::default);

pub fn intern(s: &str) -> &'static str {
    let mut set = INTERNED.lock().unwrap();
    if let Some(&existing) = set.get(s) {
        existing
    } else {
        let leaked: &'static str = Box::leak(s.to_owned().into_boxed_str());
        set.insert(leaked);
        leaked
    }
}
