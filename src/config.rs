use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub required_version: String,
    pub tokens: BTreeMap<String, Value>,
    pub outputs: BTreeMap<String, PathBuf>,
    pub sources: Option<Sources>,
    #[serde(default)]
    pub exceptions: Vec<Exception>,
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
        for (target, path) in &self.outputs {
            if !["css", "scss", "dart"].contains(&target.as_str()) {
                return Err(format!("unknown output target {target}"));
            }
            if !safe_path(path) {
                return Err(format!(
                    "output must be a relative path without parent traversal: {}",
                    path.display()
                ));
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
