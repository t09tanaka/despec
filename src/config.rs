use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub required_version: String,
    pub tokens: BTreeMap<String, Value>,
    pub outputs: BTreeMap<String, OutputPaths>,
    pub sources: Option<Sources>,
    #[serde(default)]
    pub exceptions: Vec<Exception>,
    #[serde(default)]
    pub modes: BTreeMap<String, Mode>,
    /// Opt-in source rules. Colors are always checked; each dimension rule runs only
    /// when its table is present.
    #[serde(default)]
    pub rules: BTreeMap<String, RuleConfig>,
}
/// Settings for one source rule: values to accept and class names to forbid.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleConfig {
    pub allow: Option<Vec<String>>,
    #[serde(default)]
    pub classes: Vec<String>,
    /// `[rules.color]` only: also report CSS named colors such as `black`.
    #[serde(default)]
    pub keywords: bool,
}
/// An alternative set of token values, such as a dark theme. `css` is the selector or
/// at-rule that activates it; at-rules wrap a `:root` block.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mode {
    pub css: Option<String>,
}
/// One target may be written to several destinations, for example one CSS file per
/// front-end project that shares the catalog.
#[derive(Deserialize)]
#[serde(untagged)]
pub enum OutputPaths {
    One(PathBuf),
    Many(Vec<PathBuf>),
}
impl OutputPaths {
    pub fn paths(&self) -> &[PathBuf] {
        match self {
            OutputPaths::One(p) => std::slice::from_ref(p),
            OutputPaths::Many(ps) => ps,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sources {
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Recorded counts of existing hard-coded values; see `despec baseline`.
    pub baseline: Option<PathBuf>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exception {
    pub rule: String,
    pub path: String,
    pub literal: String,
    pub reason: String,
    pub line: Option<usize>,
    #[serde(default = "one")]
    pub count: usize,
}
impl Config {
    /// Every (target, destination) pair, in configuration order.
    pub fn output_paths(&self) -> impl Iterator<Item = (&str, &PathBuf)> {
        self.outputs
            .iter()
            .flat_map(|(target, paths)| paths.paths().iter().map(move |p| (target.as_str(), p)))
    }
    pub fn is_output(&self, p: &std::path::Path) -> bool {
        self.output_paths().any(|(_, o)| o == p)
    }
    pub fn baseline(&self) -> Option<&PathBuf> {
        self.sources.as_ref().and_then(|s| s.baseline.as_ref())
    }
    pub fn is_baseline(&self, p: &std::path::Path) -> bool {
        self.baseline().is_some_and(|b| b == p)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("schema_version must be 1".into());
        }
        if self.required_version != env!("CARGO_PKG_VERSION") {
            return Err(format!(
                "required_version is {}, running despec {}",
                self.required_version,
                env!("CARGO_PKG_VERSION")
            ));
        }
        if self.tokens.is_empty() {
            return Err("tokens must not be empty".into());
        }
        if self.outputs.is_empty() {
            return Err("outputs must not be empty".into());
        }
        for (target, paths) in &self.outputs {
            if !["css", "scss", "dart"].contains(&target.as_str()) {
                return Err(format!("unknown output target {target}"));
            }
            if paths.paths().is_empty() {
                return Err(format!(
                    "output target {target} must list at least one path"
                ));
            }
            for path in paths.paths() {
                if !safe_path(path) {
                    return Err(format!(
                        "output must be a relative path without parent traversal: {}",
                        path.display()
                    ));
                }
            }
        }
        let kebab = regex::Regex::new(r"^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$").unwrap();
        for (name, mode) in &self.modes {
            if !kebab.is_match(name) {
                return Err(format!("mode name must be lowercase kebab-case: {name}"));
            }
            match &mode.css {
                Some(css) if !mode_css(css) => {
                    return Err(format!(
                        "mode {name} css must be one selector, or an @media, @supports, or @container condition, without braces, semicolons, comments, or unbalanced quotes and brackets"
                    ));
                }
                None if self.outputs.contains_key("css") => {
                    return Err(format!(
                        "mode {name} requires css when a css output is configured"
                    ));
                }
                _ => (),
            }
        }
        if let Some(sources) = &self.sources {
            for pattern in sources.include.iter().chain(&sources.exclude) {
                if pattern.is_empty()
                    || std::path::Path::new(pattern).components().any(|c| {
                        matches!(
                            c,
                            std::path::Component::ParentDir | std::path::Component::RootDir
                        )
                    })
                {
                    return Err("source patterns must be relative without parent traversal".into());
                }
                globset::Glob::new(pattern).map_err(|e| e.to_string())?;
            }
            if let Some(baseline) = &sources.baseline {
                if !safe_path(baseline) {
                    return Err(format!(
                        "sources.baseline must be a relative path without parent traversal: {}",
                        baseline.display()
                    ));
                }
                if self.is_output(baseline) {
                    return Err("sources.baseline must not be an output".into());
                }
            }
        }
        for (id, rule) in &self.rules {
            if id == "color" {
                if rule.allow.is_some() {
                    return Err(
                        "rules.color accepts only classes and keywords; colors are always checked"
                            .into(),
                    );
                }
            } else if !crate::rules::DIMENSION_RULES.contains(&id.as_str()) {
                return Err(format!(
                    "unknown rule {id}; expected color or one of {}",
                    crate::rules::DIMENSION_RULES.join(", ")
                ));
            }
            if id != "color" && rule.keywords {
                return Err(format!("rules.{id}.keywords applies only to rules.color"));
            }
            if rule.allow.iter().flatten().any(|v| v.trim().is_empty()) {
                return Err(format!("rules.{id}.allow entries must not be empty"));
            }
            for class in &rule.classes {
                if class.is_empty() {
                    return Err(format!("rules.{id}.classes entries must not be empty"));
                }
                globset::Glob::new(class).map_err(|e| format!("rules.{id}.classes: {e}"))?;
            }
        }
        for e in &self.exceptions {
            if let Some(rule) = e
                .rule
                .strip_prefix("source.")
                .and_then(|r| r.strip_suffix("-literal"))
                && rule != "color"
                && crate::rules::DIMENSION_RULES.contains(&rule)
                && !self.rules.contains_key(rule)
            {
                return Err(format!(
                    "exception for {} requires a [rules.{rule}] table",
                    e.rule
                ));
            }
            if !literal_rule(&e.rule)
                || e.reason.trim().is_empty()
                || e.literal.is_empty()
                || !safe_path(std::path::Path::new(&e.path))
                || e.line == Some(0)
                || e.count == 0
            {
                return Err("exception requires known rule, relative path, literal, nonempty reason, and positive optional line".into());
            }
        }
        Ok(())
    }
}
/// Whether `rule` is `source.color-literal` or a dimension rule's literal ID.
pub fn literal_rule(rule: &str) -> bool {
    rule.strip_prefix("source.")
        .and_then(|r| r.strip_suffix("-literal"))
        .is_some_and(|r| r == "color" || crate::rules::DIMENSION_RULES.contains(&r))
}
pub fn safe_path(p: &std::path::Path) -> bool {
    !p.as_os_str().is_empty()
        && p.components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

/// A mode's `css` is written verbatim before a block, so reject anything that could
/// end that block early or leave the rest of the file inside a comment or string.
fn mode_css(css: &str) -> bool {
    let at_rule_ok = !css.starts_with('@')
        || ["@media ", "@supports ", "@container "]
            .iter()
            .any(|p| css.starts_with(p) && css.len() > p.len());
    let mut closers = vec![];
    let mut quote = None;
    for c in css.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => (),
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => closers.push(')'),
            (None, '[') => closers.push(']'),
            (None, ')' | ']') if closers.pop() != Some(c) => return false,
            _ => (),
        }
    }
    at_rule_ok
        && quote.is_none()
        && closers.is_empty()
        && !css.trim().is_empty()
        && css.trim() == css
        && !css.contains(['{', '}', ';', '\\', '\n', '\r'])
        && !css.contains("/*")
        && !css.contains("*/")
}
fn one() -> usize {
    1
}
