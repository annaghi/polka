//! Inline SVG icon
//!
//! looks like `:family-name:`
//!
//! Parses `:family-name:` shortcodes and resolves them to SVG file content
//! using the configured icon directories.
use std::io::Cursor;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use markdown_it::parser::extset::{InlineRootExt, MarkdownItExt};
use markdown_it::parser::inline::{InlineRule, InlineState};
use markdown_it::{MarkdownIt, Node, NodeValue, Renderer};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, Writer};

pub fn add(md: &mut MarkdownIt, icon_dirs: Vec<PathBuf>) {
    add_with::<':'>(md, icon_dirs);
}

fn add_with<const MARKER: char>(md: &mut MarkdownIt, icon_dirs: Vec<PathBuf>) {
    md.ext.insert(IconConfig::<MARKER> { dirs: icon_dirs });
    md.inline.add_rule::<IconScanner<MARKER>>();
}

#[derive(Debug, Default)]
struct IconCache<const MARKER: char> {
    scanned: bool,
    max: Vec<usize>,
}
impl<const MARKER: char> InlineRootExt for IconCache<MARKER> {}

#[derive(Debug)]
struct IconConfig<const MARKER: char> {
    dirs: Vec<PathBuf>,
}

impl<const MARKER: char> MarkdownItExt for IconConfig<MARKER> {}

#[derive(Debug)]
pub struct Icon {
    #[allow(dead_code)]
    pub marker: char,
    #[allow(dead_code)]
    pub marker_len: usize,
    #[allow(dead_code)]
    pub name: String,
    #[allow(dead_code)]
    pub path: PathBuf,
    pub svg: String,
}

impl NodeValue for Icon {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        fmt.text_raw(&inject_inline_attrs(&self.svg, &node.attrs));
    }
}

fn inject_inline_attrs(svg: &str, attrs: &[(&str, String)]) -> String {
    let mut reader = Reader::from_str(svg);
    let mut writer = Writer::new(Cursor::new(Vec::new()));

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) if e.name().as_ref() == b"svg" => {
                let mut merged: IndexMap<String, String> = IndexMap::new();

                // defaults (lowest priority)
                merged.insert("aria-hidden".into(), "true".into());
                merged.insert("width".into(), "24".into());
                merged.insert("height".into(), "24".into());
                merged.insert("fill".into(), "currentColor".into());

                // existing svg attributes (override defaults)
                for a in e.attributes().filter_map(Result::ok) {
                    let key = String::from_utf8_lossy(a.key.as_ref()).to_string();
                    let val = String::from_utf8_lossy(&a.value).to_string();
                    merged.insert(key, val);
                }

                // caller overrides (highest priority)
                for (k, v) in attrs {
                    merged.insert(k.to_string(), v.clone());
                }

                let mut elem = BytesStart::new("svg");
                for (k, v) in &merged {
                    elem.push_attribute((k.as_str(), v.as_str()));
                }

                writer.write_event(Event::Start(elem)).unwrap();
            }

            Ok(Event::Eof) => break,
            Ok(e) => {
                writer.write_event(e).unwrap();
            }
            Err(e) => {
                eprintln!("[inject] parse error: {e:?}");
                return svg.to_string();
            }
        }
    }

    String::from_utf8(writer.into_inner().into_inner()).unwrap_or_else(|_| svg.to_string())
}

pub struct IconScanner<const MARKER: char>;

#[doc(hidden)]
impl<const MARKER: char> InlineRule for IconScanner<MARKER> {
    const MARKER: char = MARKER;

    fn run(state: &mut InlineState) -> Option<(Node, usize)> {
        let mut chars = state.src[state.pos..state.pos_max].chars();
        if chars.next().unwrap() != MARKER {
            return None;
        }
        if state.trailing_text_get().ends_with(MARKER) {
            return None;
        }

        let mut pos = state.pos + 1;

        // scan marker length
        while Some(MARKER) == chars.next() {
            pos += 1;
        }

        let opener_len = pos - state.pos;

        // opener marker length must be 1
        if opener_len != 1 {
            return None;
        }

        // marker length => last seen position
        let markers = state.inline_ext.get_or_insert_default::<IconCache<MARKER>>();

        if markers.scanned && markers.max.get(opener_len).copied().unwrap_or(0) <= state.pos {
            // performance note: adding entire sequence into pending is 5x faster,
            // but it will interfere with other rules working on the same char;
            // and it is extremely rare that user would put a thousand "`" in text
            return None;
        }

        let mut match_start;
        let mut match_end = pos;

        // Nothing found in the cache, scan until the end of the line (or until marker is found)
        while let Some(p) = state.src[match_end..state.pos_max].find(MARKER) {
            match_start = match_end + p;

            // scan marker length
            match_end = match_start + 1;
            chars = state.src[match_end..state.pos_max].chars();

            while Some(MARKER) == chars.next() {
                match_end += 1;
            }

            let closer_len = match_end - match_start;

            if closer_len == opener_len {
                // Found matching closer length.
                let content = state.src[pos..match_start].to_owned();
                if content.contains(char::is_whitespace) {
                    return None;
                }

                let name = normalize_icon_name(&content)?;
                let config = state.md.ext.get::<IconConfig<MARKER>>()?;

                let (path, svg_content) = resolve_from_dirs(&config.dirs, name)?;
                let svg = strip_html_comments(&svg_content);

                let node = Node::new(Icon {
                    marker: MARKER,
                    marker_len: 1,
                    name: name.to_string(),
                    path,
                    svg,
                });

                return Some((node, match_end - state.pos));
            }

            // Some different length found, put it in cache as upper limit of where closer can be found
            let markers = state.inline_ext.get_mut::<IconCache<MARKER>>().unwrap();
            while markers.max.len() <= closer_len {
                markers.max.push(0);
            }
            markers.max[closer_len] = match_start;
        }

        // Scanned through the end, didn't find anything
        let markers = state.inline_ext.get_mut::<IconCache<MARKER>>().unwrap();
        markers.scanned = true;

        None
    }
}

/// Normalize and validate an icon name.
///
/// - Starts with an ASCII letter or digit (no leading hyphen)
/// - Contains only ASCII letters, digits, or hyphens
/// - Contains at least one hyphen (the icon family is the segment before
///   the first hyphen, and maps to a directory on disk)
/// - Ends with an ASCII letter or digit (no trailing hyphen)
fn normalize_icon_name(name: &str) -> Option<&str> {
    let mut has_hyphen = false;
    let mut first = true;
    let mut last = '\0';

    for c in name.chars() {
        match c {
            '-' if first => return None,
            '-' => has_hyphen = true,
            _ if !c.is_ascii_alphanumeric() => return None,
            _ => {}
        }
        first = false;
        last = c;
    }

    (has_hyphen && last.is_ascii_alphanumeric()).then_some(name)
}

fn resolve_from_dirs(dirs: &[PathBuf], name: &str) -> Option<(PathBuf, String)> {
    for dir in dirs {
        if let Some(result) = resolve_icon(dir, name) {
            return Some(result);
        }
    }
    None
}

fn resolve_icon(dir: &Path, name: &str) -> Option<(PathBuf, String)> {
    icon_path_candidates(dir, name)
        .into_iter()
        .find_map(|p| std::fs::read_to_string(&p).ok().map(|svg| (p, svg)))
}

/// Yield candidate paths from most-specific to least-specific.
///
/// `fontawesome-solid-sun-plant-wilt` yields:
///   1. `fontawesome/solid/sun/plant/wilt.svg`
///   2. `fontawesome/solid/sun/plant-wilt.svg`
///   3. `fontawesome/solid/sun-plant-wilt.svg`
///   4. `fontawesome/solid-sun-plant-wilt.svg`
fn icon_path_candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    let segments: Vec<&str> = name.split('-').collect();
    let n = segments.len();

    (1..n)
        .map(|join_count| {
            let mut path = dir.to_path_buf();
            for part in &segments[..n - join_count] {
                path.push(part);
            }
            path.push(format!("{}.svg", segments[n - join_count..].join("-")));
            path
        })
        .collect()
}

fn strip_html_comments(svg: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;

    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        if let Some(end) = rest[start..].find("-->") {
            rest = &rest[start + end + 3..];
        } else {
            out.push_str(&rest[start..]);
            return out;
        }
    }

    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    mod normalize_icon_name_tests {
        use super::*;

        #[test]
        fn simple_two_part() {
            assert_eq!(normalize_icon_name("a-b"), Some("a-b"));
            assert_eq!(normalize_icon_name("1-2"), Some("1-2"));
        }

        #[test]
        fn three_part() {
            assert_eq!(normalize_icon_name("a-b-c"), Some("a-b-c"));
            assert_eq!(normalize_icon_name("1-2-3"), Some("1-2-3"));
        }

        #[test]
        fn empty() {
            assert_eq!(normalize_icon_name(""), None);
        }

        #[test]
        fn non_ascii() {
            assert_eq!(normalize_icon_name("π"), None);
        }

        #[test]
        fn no_hyphen() {
            assert_eq!(normalize_icon_name("text"), None);
            assert_eq!(normalize_icon_name("lucide"), None);
        }

        #[test]
        fn underscore() {
            assert_eq!(normalize_icon_name("has_under"), None);
        }

        #[test]
        fn leading_or_trailing_hyphen() {
            assert_eq!(normalize_icon_name("-lucide"), None);
            assert_eq!(normalize_icon_name("lucide-"), None);
        }

        #[test]
        fn whitespace() {
            assert_eq!(normalize_icon_name("more text"), None);
            assert_eq!(normalize_icon_name(" lucide-sun"), None);
            assert_eq!(normalize_icon_name("lucide-sun "), None);
            assert_eq!(normalize_icon_name("lucide- sun"), None);
        }

        #[test]
        fn tabs() {
            assert_eq!(normalize_icon_name("\tlucide-sun"), None);
            assert_eq!(normalize_icon_name("lucide-sun\t"), None);
            assert_eq!(normalize_icon_name("lucide-\tsun"), None);
        }

        #[test]
        fn newlines() {
            assert_eq!(normalize_icon_name("\nlucide-sun"), None);
            assert_eq!(normalize_icon_name("lucide-sun\n"), None);
            assert_eq!(normalize_icon_name("lucide-\nsun"), None);
        }
    }

    mod icon_path_candidates_tests {
        use super::*;
        use std::path::{Path, PathBuf};

        const DIR: &str = "/.icons";

        fn candidates(name: &str) -> Vec<PathBuf> {
            icon_path_candidates(Path::new(DIR), name)
        }

        #[test]
        fn two_segments() {
            assert_eq!(candidates("lucide-sun"), vec![PathBuf::from("/.icons/lucide/sun.svg")]);
        }

        #[test]
        fn three_segments() {
            assert_eq!(
                candidates("lucide-arrow-right"),
                vec![
                    PathBuf::from("/.icons/lucide/arrow/right.svg"),
                    PathBuf::from("/.icons/lucide/arrow-right.svg"),
                ]
            );
        }

        #[test]
        fn four_segments() {
            assert_eq!(
                candidates("fa-solid-circle-check"),
                vec![
                    PathBuf::from("/.icons/fa/solid/circle/check.svg"),
                    PathBuf::from("/.icons/fa/solid/circle-check.svg"),
                    PathBuf::from("/.icons/fa/solid-circle-check.svg"),
                ]
            );
        }

        #[test]
        fn five_segments() {
            assert_eq!(
                candidates("fontawesome-solid-sun-plant-wilt"),
                vec![
                    PathBuf::from("/.icons/fontawesome/solid/sun/plant/wilt.svg"),
                    PathBuf::from("/.icons/fontawesome/solid/sun/plant-wilt.svg"),
                    PathBuf::from("/.icons/fontawesome/solid/sun-plant-wilt.svg"),
                    PathBuf::from("/.icons/fontawesome/solid-sun-plant-wilt.svg"),
                ]
            );
        }
    }

    mod resolve_from_dirs_tests {
        use super::*;
        use std::sync::LazyLock;

        static TMP: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
            let tmp = tempfile::tempdir().unwrap();

            let lucide = tmp.path().join("lucide");
            std::fs::create_dir_all(&lucide).unwrap();
            std::fs::write(lucide.join("sun.svg"), svg("lucide-sun")).unwrap();
            std::fs::write(lucide.join("sun-dim.svg"), svg("lucide-sun-dim")).unwrap();

            let fa = tmp.path().join("fontawesome").join("solid");
            std::fs::create_dir_all(&fa).unwrap();
            std::fs::write(fa.join("sun.svg"), svg("fontawesome-solid-sun")).unwrap();
            std::fs::write(fa.join("sun-plant-wilt.svg"), svg("fontawesome-solid-sun-plant-wilt")).unwrap();

            tmp
        });

        fn dirs() -> Vec<PathBuf> {
            vec![TMP.path().to_path_buf()]
        }

        fn svg(name: &str) -> String {
            format!(r#"<svg><path d="{name}"/></svg>"#)
        }

        fn assert_resolves(name: &str) {
            let (path, content) = resolve_from_dirs(&dirs(), name).unwrap();
            assert_eq!(content, svg(name));
            assert!(path.exists());
        }

        mod lucide {
            use super::*;

            #[test]
            fn two_segments() {
                assert_resolves("lucide-sun");
            }

            #[test]
            fn three_segments() {
                assert_resolves("lucide-sun-dim");
            }
        }

        mod fontawesome {
            use super::*;

            #[test]
            fn three_segments() {
                assert_resolves("fontawesome-solid-sun");
            }

            #[test]
            fn five_segments() {
                assert_resolves("fontawesome-solid-sun-plant-wilt");
            }
        }

        mod non_existing {
            use super::*;

            #[test]
            fn unknown_prefix() {
                assert!(resolve_from_dirs(&dirs(), "nope-nope").is_none());
            }

            #[test]
            fn lucide_missing() {
                assert!(resolve_from_dirs(&dirs(), "lucide-nope").is_none());
            }

            #[test]
            fn fontawesome_missing_two_segments() {
                assert!(resolve_from_dirs(&dirs(), "fontawesome-nope").is_none());
            }

            #[test]
            fn fontawesome_missing_three_segments() {
                assert!(resolve_from_dirs(&dirs(), "fontawesome-nope-nope").is_none());
            }

            #[test]
            fn fontawesome_known_style_missing_icon() {
                assert!(resolve_from_dirs(&dirs(), "fontawesome-solid-nope").is_none());
            }
        }
    }

    mod strip_html_comments {
        use super::*;

        #[test]
        fn no_comments() {
            assert_eq!(strip_html_comments("<svg></svg>"), "<svg></svg>");
        }

        #[test]
        fn empty() {
            assert_eq!(strip_html_comments(""), "");
        }

        #[test]
        fn single_comment() {
            assert_eq!(strip_html_comments("<!-- hi --><svg></svg>"), "<svg></svg>");
        }

        #[test]
        fn comment_in_middle() {
            assert_eq!(strip_html_comments("<svg><!-- x --><g/></svg>"), "<svg><g/></svg>");
        }

        #[test]
        fn multiple_comments() {
            assert_eq!(
                strip_html_comments("<!-- a --><svg><!-- b --></svg><!-- c -->"),
                "<svg></svg>"
            );
        }

        #[test]
        fn adjacent_comments() {
            assert_eq!(strip_html_comments("<!-- a --><!-- b -->rest"), "rest");
        }

        #[test]
        fn empty_comment() {
            assert_eq!(strip_html_comments("<!---->rest"), "rest");
        }

        #[test]
        fn comment_only() {
            assert_eq!(strip_html_comments("<!-- only -->"), "");
        }

        #[test]
        fn unclosed_comment() {
            assert_eq!(strip_html_comments("before<!-- unclosed"), "before<!-- unclosed");
        }

        #[test]
        fn partial_opener() {
            assert_eq!(strip_html_comments("<!- not a comment -->"), "<!- not a comment -->");
        }

        #[test]
        fn nested_opener_in_comment() {
            assert_eq!(strip_html_comments("<!-- <!-- inner --> after"), " after");
        }

        #[test]
        fn dashes_inside_comment() {
            assert_eq!(strip_html_comments("<!-- -- ---> after"), " after");
        }

        #[test]
        fn preserves_whitespace() {
            assert_eq!(strip_html_comments("  <!-- x -->  "), "    ");
        }

        #[test]
        fn multibyte_around_comment() {
            assert_eq!(strip_html_comments("日本<!-- x -->語"), "日本語");
        }

        #[test]
        fn multibyte_inside_comment() {
            assert_eq!(strip_html_comments("a<!-- 日本語 -->b"), "ab");
        }
    }

    mod inject_inline_attrs_tests {
        use super::*;

        mod defaults {
            use super::*;

            #[test]
            fn injects_defaults_on_bare_svg() {
                let result = inject_inline_attrs("<svg><path/></svg>", &[]);
                assert!(result.contains(r#"aria-hidden="true""#), "got: {result}");
                assert!(result.contains(r#"width="24""#), "got: {result}");
                assert!(result.contains(r#"height="24""#), "got: {result}");
                assert!(result.contains(r#"fill="currentColor""#), "got: {result}");
            }

            #[test]
            fn svg_attrs_override_defaults() {
                let result = inject_inline_attrs(r#"<svg width="48" height="48"><path/></svg>"#, &[]);
                assert!(result.contains(r#"width="48""#), "got: {result}");
                assert!(result.contains(r#"height="48""#), "got: {result}");
            }

            #[test]
            fn svg_fill_overrides_default() {
                let result = inject_inline_attrs(r#"<svg fill="none"><path/></svg>"#, &[]);
                assert!(result.contains(r#"fill="none""#), "got: {result}");
                assert!(!result.contains("currentColor"), "got: {result}");
            }
        }

        mod caller_overrides {
            use super::*;

            #[test]
            fn adds_new_attr() {
                let result = inject_inline_attrs("<svg><path/></svg>", &[("class", "icon".into())]);
                assert!(result.contains(r#"class="icon""#), "got: {result}");
            }

            #[test]
            fn overrides_svg_attr() {
                let result = inject_inline_attrs(r#"<svg width="48"><path/></svg>"#, &[("width", "16".into())]);
                assert!(result.contains(r#"width="16""#), "got: {result}");
                assert!(!result.contains(r#"width="48""#), "got: {result}");
            }

            #[test]
            fn overrides_default() {
                let result = inject_inline_attrs("<svg><path/></svg>", &[("fill", "red".into())]);
                assert!(result.contains(r#"fill="red""#), "got: {result}");
                assert!(!result.contains("currentColor"), "got: {result}");
            }

            #[test]
            fn overrides_svg_attr_over_default() {
                let result = inject_inline_attrs(r#"<svg fill="none"><path/></svg>"#, &[("fill", "red".into())]);
                assert!(result.contains(r#"fill="red""#), "got: {result}");
                assert!(!result.contains("none"), "got: {result}");
            }
        }

        #[test]
        fn no_duplicate_width() {
            let result = inject_inline_attrs(r#"<svg width="48"><path/></svg>"#, &[("width", "16".into())]);
            assert_eq!(result.matches("width=").count(), 1, "got: {result}");
        }

        #[test]
        fn no_duplicate_fill() {
            let result = inject_inline_attrs(r#"<svg fill="none"><path/></svg>"#, &[("fill", "red".into())]);
            assert_eq!(result.matches("fill=").count(), 1, "got: {result}");
        }

        #[test]
        fn escapes_value() {
            let result = inject_inline_attrs("<svg><path/></svg>", &[("data-x", r#"a"b<c>"#.into())]);
            assert!(result.contains(r#"data-x="a&quot;b&lt;c&gt;""#), "got: {result}");
        }

        #[test]
        fn no_svg_tag() {
            let input = "<div>hello</div>";
            assert_eq!(inject_inline_attrs(input, &[("class", "x".into())]), input);
        }

        #[test]
        fn preserves_existing_xmlns() {
            let result = inject_inline_attrs(
                r#"<svg xmlns="http://www.w3.org/2000/svg"><path/></svg>"#,
                &[("class", "icon".into())],
            );
            assert!(result.contains("xmlns="), "got: {result}");
            assert!(result.contains(r#"class="icon""#), "got: {result}");
        }
    }
}
