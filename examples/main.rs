use std::io::{Read, Write};

use clap::Parser;
use markdown_it::parser::inline::{Text, TextSpecial};

#[derive(Parser)]
#[command(about = "Renders Markdown to HTML.")]
struct Args {
    /// file to read (default: stdin)
    #[arg(default_value = "-")]
    file: String,

    /// directories to search for icons
    #[arg(long = "icon-dir")]
    icon_dirs: Vec<String>,

    /// file to write (default: stdout)
    #[arg(short, long, default_value = "-")]
    output: String,

    /// print syntax tree for debugging
    #[arg(long)]
    tree: bool,
}

fn main() {
    let args = Args::parse();

    let vec = if args.file == "-" {
        let mut vec = Vec::new();
        std::io::stdin().read_to_end(&mut vec).unwrap();
        vec
    } else {
        std::fs::read(&args.file).unwrap()
    };

    let source = String::from_utf8_lossy(&vec);
    let md = &mut markdown_it::MarkdownIt::new();
    markdown_it::plugins::cmark::add(md);
    // Don't enable extra::typographer, because it converts `--` to `–` in Text nodes,
    // breaking CSS custom property names like `--color` in attributes.
    markdown_it::plugins::extra::beautify_links::add(md);
    // markdown_it::plugins::extra::heading_anchors::add(md, slugify);
    markdown_it::plugins::extra::linkify::add(md);
    markdown_it::plugins::extra::smartquotes::add(md);
    markdown_it::plugins::extra::strikethrough::add(md);
    markdown_it::plugins::extra::tables::add(md);
    markdown_it::plugins::html::add(md);
    // Register core rules before core rules
    let icon_dirs: Vec<std::path::PathBuf> = args.icon_dirs.into_iter().map(Into::into).collect();
    polka::rules::inline::icon::add(md, icon_dirs);
    polka::rules::inline::span::add(md);
    // Register core rules after inline/block rules
    polka::rules::core::attrs::add(md);

    let ast = md.parse(&source);

    if args.tree {
        ast.walk(|node, depth| {
            print!("{}", "    ".repeat(depth as usize));
            let name = &node.name()[node.name().rfind("::").map(|x| x + 2).unwrap_or_default()..];
            if let Some(data) = node.cast::<Text>() {
                println!("{name}: {:?}", data.content);
            } else if let Some(data) = node.cast::<TextSpecial>() {
                println!("{name}: {:?}", data.content);
            } else {
                println!("{name}");
            }
        });
        return;
    }

    let result = ast.render();

    if args.output == "-" {
        std::io::stdout().write_all(result.as_bytes()).unwrap();
    } else {
        std::fs::write(&args.output, &result).unwrap();
    }
}
