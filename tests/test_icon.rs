use std::sync::LazyLock;

use markdown_it::MarkdownIt;
use markdown_it::plugins::cmark;

fn create_test_parser() -> (MarkdownIt, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    // Lucide set
    let lucide = tmp.path().join("lucide");
    std::fs::create_dir_all(&lucide).unwrap();
    std::fs::write(lucide.join("sun.svg"), r#"<svg><path d="lucide-sun"/></svg>"#).unwrap();
    std::fs::write(lucide.join("sun-dim.svg"), r#"<svg><path d="lucide-sun-dim"/></svg>"#).unwrap();
    // Font Awesome set
    let fontawesome = tmp.path().join("fontawesome").join("solid");
    std::fs::create_dir_all(&fontawesome).unwrap();
    std::fs::write(
        fontawesome.join("sun.svg"),
        r#"<svg><path d="fontawesome-solid-sun"/></svg>"#,
    )
    .unwrap();
    std::fs::write(
        fontawesome.join("sun-plant-wilt.svg"),
        r#"<svg><path d="fontawesome-solid-sun-plant-wilt"/></svg>"#,
    )
    .unwrap();

    let mut md = MarkdownIt::new();
    cmark::add(&mut md);
    polka::add(&mut md, vec![tmp.path().to_path_buf()]);
    (md, tmp)
}

static TEST_PARSER: LazyLock<(MarkdownIt, tempfile::TempDir)> = LazyLock::new(create_test_parser);

fn render(input: &str) -> String {
    let (md, _) = &*TEST_PARSER;
    md.parse(input).render()
}

fn expected_svg(name: &str) -> String {
    format!(r#"<svg aria-hidden="true" width="24" height="24" fill="currentColor"><path d="{name}"/></svg>"#)
}

fn assert_renders_icon(input: &str, name: &str) {
    let html = render(input);
    assert!(html.contains(&expected_svg(name)), "got: {html}");
}

mod lucide {
    use super::*;

    #[test]
    fn two_segments() {
        assert_renders_icon(":lucide-sun:", "lucide-sun");
    }

    #[test]
    fn three_segments() {
        assert_renders_icon(":lucide-sun-dim:", "lucide-sun-dim");
    }
}

mod fontawesome {
    use super::*;

    #[test]
    fn three_segments() {
        assert_renders_icon(":fontawesome-solid-sun:", "fontawesome-solid-sun");
    }

    #[test]
    fn five_segments() {
        assert_renders_icon(":fontawesome-solid-sun-plant-wilt:", "fontawesome-solid-sun-plant-wilt");
    }
}

mod missing {
    use super::*;

    #[test]
    fn preserved_as_text() {
        let html = render(":missing-icon:");
        assert!(html.contains(":missing-icon:"), "got: {html}");
    }

    #[test]
    fn no_svg_emitted() {
        let html = render(":missing-icon:");
        assert!(!html.contains("<svg"), "got: {html}");
    }
}

mod surrounding_content {
    use super::*;

    fn p(inner: &str) -> String {
        format!("<p>{inner}</p>\n")
    }

    #[test]
    fn inline_between_text() {
        let svg = expected_svg("lucide-sun");
        assert_eq!(render("hello:lucide-sun:world"), p(&format!("hello{svg}world")));
    }

    #[test]
    fn inline_with_text() {
        let svg = expected_svg("lucide-sun");
        assert_eq!(render("hello :lucide-sun: world"), p(&format!("hello {svg} world")));
    }

    #[test]
    fn adjacent_colons() {
        assert_eq!(render("::lucide-sun::"), "<p>::lucide-sun::</p>\n");
    }

    #[test]
    fn two_icons_with_space() {
        let sun = expected_svg("lucide-sun");
        let dim = expected_svg("lucide-sun-dim");
        assert_eq!(render(":lucide-sun: :lucide-sun-dim:"), p(&format!("{sun} {dim}")));
    }

    #[test]
    fn two_icons_no_space() {
        assert_eq!(
            render(":lucide-sun::lucide-sun-dim:"),
            "<p>:lucide-sun::lucide-sun-dim:</p>\n"
        );
    }

    #[test]
    fn mixed_valid_and_missing() {
        let svg = expected_svg("lucide-sun");
        assert_eq!(
            render(":lucide-sun: and :missing-icon:"),
            p(&format!("{svg} and :missing-icon:"))
        );
    }

    #[test]
    fn three_icons_inline() {
        let sun = expected_svg("lucide-sun");
        let dim = expected_svg("lucide-sun-dim");
        let fa = expected_svg("fontawesome-solid-sun");
        assert_eq!(
            render(":lucide-sun: :lucide-sun-dim: :fontawesome-solid-sun:"),
            p(&format!("{sun} {dim} {fa}"))
        );
    }
}
