use std::collections::HashMap;
use std::path::Path;

#[derive(Clone)]
pub struct Case {
    pub slug: String,
    pub input: String,
    pub expected: String,
    pub line: usize,
}

pub fn parse(src: &str, path: &Path) -> Result<Vec<Case>, String> {
    let mut cases = Vec::new();
    let mut slugs: HashMap<String, usize> = HashMap::new();
    let mut desc_lines: Vec<&str> = Vec::new();
    let mut just_saw_blank = false;
    let mut lines = src.lines().enumerate().peekable();

    while let Some((i, line)) = lines.next() {
        if let Some(fence_len) = plain_fence(line) {
            let description = desc_lines.join(" ").trim().to_string();
            desc_lines.clear();
            just_saw_blank = false;
            if description.is_empty() {
                return Err(format!("{}:{}: fence without description", path.display(), i + 1));
            }

            let mut input = String::new();
            let mut expected = String::new();
            let mut seen_sep = false;
            let mut closed = false;

            for (_, body) in lines.by_ref() {
                if is_fence_close(body, fence_len) {
                    closed = true;
                    break;
                }
                if !seen_sep && body == "." {
                    seen_sep = true;
                    continue;
                }
                let target = if seen_sep { &mut expected } else { &mut input };
                target.push_str(body);
                target.push('\n');
            }

            if !closed {
                return Err(format!("{}:{}: unterminated fence", path.display(), i + 1));
            }
            if !seen_sep {
                return Err(format!("{}:{}: missing '.' separator", path.display(), i + 1));
            }

            let slug = slugify(&description);
            if slug.is_empty() {
                return Err(format!(
                    "{}:{}: description produces empty slug: {description:?}",
                    path.display(),
                    i + 1
                ));
            }
            let n = slugs.entry(slug.clone()).or_insert(0);
            *n += 1;
            let final_slug = if *n == 1 { slug } else { format!("{slug}_{n}") };

            cases.push(Case {
                slug: final_slug,
                input,
                expected,
                line: i + 1,
            });
        } else if let Some(fence_len) = info_fence(line) {
            for (_, body) in lines.by_ref() {
                if is_fence_close(body, fence_len) {
                    break;
                }
            }
            desc_lines.clear();
            just_saw_blank = false;
        } else if line.trim().is_empty() {
            just_saw_blank = true;
        } else {
            if just_saw_blank {
                desc_lines.clear();
            }
            desc_lines.push(line);
            just_saw_blank = false;
        }
    }

    Ok(cases)
}

fn plain_fence(line: &str) -> Option<usize> {
    let t = line.trim_end();
    (t.len() >= 3 && t.chars().all(|c| c == '`')).then_some(t.len())
}

fn info_fence(line: &str) -> Option<usize> {
    let n = line.chars().take_while(|&c| c == '`').count();
    (n >= 3 && line.len() > n).then_some(n)
}

fn is_fence_close(line: &str, len: usize) -> bool {
    let t = line.trim_end();
    t.len() == len && t.chars().all(|c| c == '`')
}

fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_underscore = true;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_underscore = false;
        } else if !prev_underscore {
            out.push('_');
            prev_underscore = true;
        }
    }
    out.trim_end_matches('_').to_string()
}
