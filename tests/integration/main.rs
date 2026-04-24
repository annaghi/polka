use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use libtest_mimic::{Arguments, Failed, Trial};
use markdown_it::MarkdownIt;
use markdown_it::plugins::{cmark, extra};

mod parser;

static PARSER: LazyLock<MarkdownIt> = LazyLock::new(|| {
    let mut md = MarkdownIt::new();
    cmark::add(&mut md);
    // Don't enable extra::typographer — replaces ASCII dashes in Text nodes,
    // breaking CSS identifiers like `.btn--primary`.
    extra::beautify_links::add(&mut md);
    extra::linkify::add(&mut md);
    // Don't enable extra::smartquotes — replaces ASCII quotes in Text nodes,
    // breaking CSS attribute values like `data-attr="value"`.
    extra::strikethrough::add(&mut md);
    extra::tables::add(&mut md);
    polka::add(&mut md, Vec::new());
    md
});

fn main() {
    let args = Arguments::from_args();
    let cases_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/integration/cases");

    let trials = collect_trials(&cases_dir).unwrap_or_else(|e| {
        eprintln!("failed to collect test cases: {e}");
        std::process::exit(2);
    });

    libtest_mimic::run(&args, trials).exit();
}

fn collect_trials(dir: &Path) -> Result<Vec<Trial>, String> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("read_dir {}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "test"))
        .collect();
    files.sort();

    let mut trials = Vec::new();
    for path in files {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let src = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let cases = parser::parse(&src, &path)?;

        for case in cases {
            let name = format!("integration::{stem}::{}", case.slug);
            let loc = format!("{}:{}", path.display(), case.line);
            trials.push(Trial::test(name, move || run_case(&case, &loc)));
        }
    }
    Ok(trials)
}

fn run_case(case: &parser::Case, loc: &str) -> Result<(), Failed> {
    let actual = PARSER.parse(&case.input).render();
    if actual == case.expected {
        Ok(())
    } else {
        Err(format!(
            "mismatch at {loc}\n\
             --- input ---\n{}\
             --- expected ---\n{}\
             --- actual ---\n{}",
            case.input, case.expected, actual
        )
        .into())
    }
}
