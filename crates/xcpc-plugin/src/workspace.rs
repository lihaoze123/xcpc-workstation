use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceProblemSummary {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceProblemDetail {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement_md: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editorial_md: Option<String>,
}

/// Strictly read-only workspace projection exposed to plugins with `workspace_read = true`.
/// Never exposes filesystem paths or mutation methods.
pub trait WorkspaceReader: Send + Sync {
    fn list_problems(&self) -> Result<Vec<WorkspaceProblemSummary>>;
    fn get_problem(&self, id: &str) -> Result<Option<WorkspaceProblemDetail>>;
}
