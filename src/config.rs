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
                Some(css)
                    if css.trim().is_empty()
                        || css.trim() != css
                        || css.contains(['{', '}', ';', '\n', '\r']) =>
                {
                    return Err(format!(
                        "mode {name} css must be one selector or at-rule without braces, semicolons, or surrounding whitespace"
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
        }
        for e in &self.exceptions {
            if e.rule != "source.color-literal"
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
pub fn safe_path(p: &std::path::Path) -> bool {
    !p.as_os_str().is_empty()
        && p.components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

fn one() -> usize {
    1
}
