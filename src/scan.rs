use crate::{
    Diagnostic,
    config::Config,
    rules::{Finding, Rules},
};
use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;
use std::path::Path;
fn globs(items: &[String]) -> Result<GlobSet, String> {
    let mut b = GlobSetBuilder::new();
    for s in items {
        b.add(Glob::new(s).map_err(|e| e.to_string())?);
    }
    b.build().map_err(|e| e.to_string())
}
pub fn scan(root: &Path, c: &Config) -> Result<(usize, Vec<Diagnostic>), String> {
    let Some(sources) = &c.sources else {
        return Ok((
            0,
            vec![Diagnostic::new(
                "source.no-files",
                ".despec.toml",
                "sources.include must select source files",
            )],
        ));
    };
    let include = globs(&sources.include)?;
    let exclude = globs(&sources.exclude)?;
    let re=Regex::new(r"(?i)#[0-9a-f]{3}(?:[0-9a-f]{1}|[0-9a-f]{3}|[0-9a-f]{5})?\b|\b(?:rgba?|hsla?)\(\s*[0-9.+%-]+(?:[\s,/]+[0-9.+%-]+){2,3}\s*\)|\bColor\s*\(\s*0x[0-9a-f]{8}\s*\)").unwrap();
    let mut paths = std::collections::BTreeSet::new();
    for pattern in &sources.include {
        let fixed: String = pattern
            .chars()
            .take_while(|c| !"*?[{\\".contains(*c))
            .collect();
        let start = if fixed == *pattern {
            root.join(pattern)
        } else {
            let prefix = fixed.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
            root.join(prefix)
        };
        if !start.starts_with(root)
            || Path::new(pattern).components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::RootDir
                )
            })
        {
            return Err(format!(
                "include must be relative without parent traversal: {pattern}"
            ));
        }
        if !start.exists() {
            return Ok((
                0,
                vec![Diagnostic::new(
                    "source.no-files",
                    pattern,
                    "include pattern path does not exist",
                )],
            ));
        }
        let selector = globs(std::slice::from_ref(pattern))?;
        let mut selected = 0;
        for entry in walkdir::WalkDir::new(&start)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                !matches!(
                    e.file_name().to_str(),
                    Some(".git" | "node_modules" | "target")
                ) && (e.depth() == 0
                    || !exclude.is_match(e.path().strip_prefix(root).unwrap_or(e.path())))
            })
        {
            let entry = entry.map_err(|e| e.to_string())?;
            let p = entry.path().strip_prefix(root).map_err(|e| e.to_string())?;
            if selector.is_match(p) && include.is_match(p) && !exclude.is_match(p) {
                if entry.file_type().is_symlink() {
                    return Err(format!("selected source is symlink: {}", p.display()));
                }
                if entry.file_type().is_file() && !c.is_output(p) && p != Path::new(".despec.toml")
                {
                    paths.insert(p.to_owned());
                    selected += 1;
                }
            }
        }
        if selected == 0 {
            return Ok((
                0,
                vec![Diagnostic::new(
                    "source.no-files",
                    pattern,
                    "include pattern selected zero source files",
                )],
            ));
        }
    }
    let rules = Rules::new(&c.rules)?;
    let mut ds = vec![];
    let mut used = vec![0usize; c.exceptions.len()];
    let mut count = 0;
    for p in paths {
        if c.is_output(&p) || p == Path::new(".despec.toml") {
            continue;
        }
        count += 1;
        let text =
            std::fs::read_to_string(root.join(&p)).map_err(|e| format!("{}: {e}", p.display()))?;
        let text = strip_comments(&text, p.extension().is_some_and(|e| e != "css"));
        let extension = p.extension().and_then(|e| e.to_str());
        let mut findings = vec![];
        let mut line_starts = vec![];
        let mut offset = 0;
        for raw in text.split_inclusive('\n') {
            line_starts.push(offset);
            let line = raw.strip_suffix('\n').unwrap_or(raw);
            let line = line.strip_suffix('\r').unwrap_or(line);
            for m in re.find_iter(line) {
                // A standalone ID selector is unambiguous; complex selectors still
                // need a language parser and are documented as a lexical limitation.
                if matches!(extension, Some("css" | "scss" | "vue"))
                    && m.as_str().starts_with('#')
                    && line[..m.start()].trim().is_empty()
                    && line[m.end()..].trim_start().starts_with('{')
                {
                    continue;
                }
                findings.push(Finding {
                    rule: "color",
                    offset: offset + m.start(),
                    literal: m.as_str().to_owned(),
                });
            }
            offset += raw.len();
        }
        findings.extend(rules.find(&text, extension));
        // Stable, so same-offset findings keep detection order.
        findings.sort_by_key(|f| f.offset);
        let path = p.to_string_lossy();
        for f in findings {
            let line = line_starts.partition_point(|&s| s <= f.offset);
            let rule_id = format!("source.{}-literal", f.rule);
            let mut suppressed = false;
            for (i, e) in c.exceptions.iter().enumerate() {
                if e.rule == rule_id
                    && e.path == path
                    && e.literal == f.literal
                    && e.line.is_none_or(|n| n == line)
                    && !suppressed
                {
                    used[i] += 1;
                    suppressed = true;
                }
            }
            if !suppressed {
                let message = if f.rule == "color" {
                    format!("use a design token instead of {}", f.literal)
                } else {
                    format!("use a {} token instead of {}", f.rule, f.literal)
                };
                let mut d = Diagnostic::new(&rule_id, &path, &message);
                d.line = Some(line);
                d.literal = Some(f.literal);
                ds.push(d)
            }
        }
    }
    if count == 0 {
        ds.push(Diagnostic::new(
            "source.no-files",
            ".despec.toml",
            "source scan selected zero files",
        ))
    }
    for (i, e) in c.exceptions.iter().enumerate() {
        if used[i] != e.count {
            ds.push(Diagnostic::new(
                "exception.unused",
                &e.path,
                &format!(
                    "exception count mismatch for {}: expected {}, found {}; {}",
                    e.literal, e.count, used[i], e.reason
                ),
            ))
        }
    }
    Ok((count, ds))
}

// Preserve strings and newlines, masking comments before lexical matching.
fn strip_comments(text: &str, slash_comments: bool) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    let mut quote = None;
    let mut block = false;
    let mut html = false;
    let mut line = false;
    while let Some(c) = chars.next() {
        if line {
            if c == '\n' {
                line = false;
                out.push(c)
            } else {
                out.push(' ')
            };
            continue;
        }
        if html {
            if c == '-' && chars.clone().take(2).eq("->".chars()) {
                chars.next();
                chars.next();
                out.push_str("   ");
                html = false;
            } else {
                out.push(if c == '\n' { '\n' } else { ' ' });
            }
            continue;
        }
        if block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                out.push_str("  ");
                block = false
            } else {
                out.push(if c == '\n' { '\n' } else { ' ' })
            };
            continue;
        }
        if let Some(q) = quote {
            out.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next)
                }
            } else if c == q {
                quote = None
            };
            continue;
        }
        if c == '"' || c == '\'' || c == '`' {
            quote = Some(c);
            out.push(c)
        } else if c == '<' && chars.clone().take(3).eq("!--".chars()) {
            for _ in 0..3 {
                chars.next();
            }
            html = true;
            out.push_str("    ");
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            block = true;
            out.push_str("  ")
        } else if slash_comments && c == '/' && chars.peek() == Some(&'/') {
            chars.next();
            line = true;
            out.push_str("  ")
        } else {
            out.push(c)
        }
    }
    out
}
