pub mod catalog;
pub mod config;
pub mod scan;
use serde::Serialize;
#[derive(Serialize)]
pub struct Diagnostic {
    pub rule_id: String,
    pub path: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}
impl Diagnostic {
    pub fn new(rule: &str, path: &str, message: &str) -> Self {
        Self {
            rule_id: rule.into(),
            path: path.into(),
            message: message.into(),
            line: None,
        }
    }
}
