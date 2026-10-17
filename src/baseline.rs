//! A ratchet for existing hard-coded values: the number of remaining
//! `source.*-literal` findings per (rule, path, literal). Counts may only go down;
//! new hard-coded values belong in `[[exceptions]]` with a reason.
use crate::{Diagnostic, rules::DIMENSION_RULES};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// (rule ID, source path, literal). Ordered so that the file is stable.
pub type Key = (String, String, String);
pub type Counts = BTreeMap<Key, usize>;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    schema_version: u32,
    entries: Vec<Entry>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    rule: String,
    path: String,
    literal: String,
    count: usize,
}

/// Whether a diagnostic is a hard-coded value that a baseline can record.
pub fn is_literal(d: &Diagnostic) -> bool {
    d.literal.is_some() && known_rule(&d.rule_id)
}
fn known_rule(rule: &str) -> bool {
    rule.strip_prefix("source.")
        .and_then(|r| r.strip_suffix("-literal"))
        .is_some_and(|r| r == "color" || DIMENSION_RULES.contains(&r))
}
pub fn key(d: &Diagnostic) -> Key {
    (
        d.rule_id.clone(),
        d.path.clone(),
        d.literal.clone().unwrap_or_default(),
    )
}
/// Counts the literal diagnostics that remain after exceptions were applied.
pub fn count(ds: &[Diagnostic]) -> Counts {
    let mut counts = Counts::new();
    for d in ds.iter().filter(|d| is_literal(d)) {
        *counts.entry(key(d)).or_default() += 1;
    }
    counts
}
/// Parses a baseline file. Hand-edited files may be unsorted; they are re-sorted
/// when written.
pub fn parse(text: &str) -> Result<Counts, String> {
    let file: File = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if file.schema_version != 1 {
        return Err(format!(
            "baseline schema_version must be 1, found {}",
            file.schema_version
        ));
    }
    let mut counts = Counts::new();
    for e in file.entries {
        if !known_rule(&e.rule)
            || e.literal.is_empty()
            || !crate::config::safe_path(std::path::Path::new(&e.path))
            || e.count == 0
        {
            return Err(format!(
                "baseline entry {} {} {} requires a known source rule, relative path, literal, and positive count",
                e.rule, e.path, e.literal
            ));
        }
        let entry = (e.rule, e.path, e.literal);
        if counts.contains_key(&entry) {
            return Err(format!(
                "baseline lists {} {} {} more than once",
                entry.0, entry.1, entry.2
            ));
        }
        counts.insert(entry, e.count);
    }
    Ok(counts)
}
/// Renders entries in rule, path, literal order with a trailing newline.
pub fn render(counts: &Counts) -> String {
    let file = File {
        schema_version: 1,
        entries: counts
            .iter()
            .filter(|(_, n)| **n > 0)
            .map(|((rule, path, literal), count)| Entry {
                rule: rule.clone(),
                path: path.clone(),
                literal: literal.clone(),
                count: *count,
            })
            .collect(),
    };
    let mut text = serde_json::to_string_pretty(&file).unwrap();
    text.push('\n');
    text
}
/// Keys whose count grew, or that the baseline does not list, as
/// (key, found, recorded).
pub fn increases<'a>(found: &'a Counts, recorded: &Counts) -> Vec<(&'a Key, usize, usize)> {
    found
        .iter()
        .filter_map(|(k, &n)| {
            let m = recorded.get(k).copied().unwrap_or(0);
            (n > m).then_some((k, n, m))
        })
        .collect()
}
/// Applies a baseline to `check` diagnostics: literal findings within the recorded
/// count are dropped, keys over it keep every finding with the counts appended, and
/// keys that shrank produce `baseline.stale` at `baseline_path`.
pub fn apply(ds: Vec<Diagnostic>, recorded: &Counts, baseline_path: &str) -> Vec<Diagnostic> {
    let found = count(&ds);
    let mut out = vec![];
    for mut d in ds {
        if is_literal(&d) {
            let k = key(&d);
            let n = found[&k];
            let m = recorded.get(&k).copied().unwrap_or(0);
            if n <= m {
                continue;
            }
            d.message = format!("{} (found {n}, baseline {m})", d.message);
        }
        out.push(d);
    }
    for (k, &m) in recorded {
        let n = found.get(k).copied().unwrap_or(0);
        if n < m {
            out.push(Diagnostic::new(
                "baseline.stale",
                baseline_path,
                &format!(
                    "{} {} {}: found {n}, baseline {m}; run despec baseline to lower it",
                    k.0, k.1, k.2
                ),
            ));
        }
    }
    out
}
