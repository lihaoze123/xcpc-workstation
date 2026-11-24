use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::runtime::{HostContext, PluginRuntime};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OjContext {
    pub handle: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OjVerifyResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OjRatingChange {
    pub contest_id: String,
    pub contest_name: String,
    pub rating_before: i64,
    pub rating_after: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<i64>,
    /// Unix timestamp in seconds.
    pub at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OjAccountSync {
    pub handle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rating: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<String>,
    #[serde(default)]
    pub rating_history: Vec<OjRatingChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OjSummarySync {
    pub handle: String,
    pub solved_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submission_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rating: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<String>,
    #[serde(default)]
    pub solved_problem_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OjSubmission {
    pub submission_id: String,
    pub handle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contest_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem_index: Option<String>,
    pub problem_key: String,
    pub problem_title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_rating: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_rating: Option<i64>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Normalized verdict: "AC", "WA", "TLE", "MLE", "RE", "CE", "PE", "OTHER".
    pub verdict: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Unix timestamp in seconds.
    pub submitted_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OjSubmissionsBatch {
    #[serde(default)]
    pub submissions: Vec<OjSubmission>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OjContest {
    pub contest_id: String,
    pub name: String,
    /// Unix timestamp in seconds.
    pub start_time: i64,
    pub duration_sec: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedProblemUrl {
    pub platform: String,
    pub problem_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contest_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem_index: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OjSample {
    pub input: String,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OjProblemDetail {
    pub problem_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_rating: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_rating: Option<i64>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement_md: Option<String>,
    #[serde(default)]
    pub samples: Vec<OjSample>,
}

/// Typed wrapper around an `oj_source` plugin script running in the QuickJS sandbox.
pub struct OjPluginClient<'a> {
    source: &'a str,
    host_ctx: &'a HostContext,
}

impl<'a> OjPluginClient<'a> {
    pub fn new(source: &'a str, host_ctx: &'a HostContext) -> Self {
        Self { source, host_ctx }
    }

    pub fn supports(&self, fn_name: &str) -> Result<bool> {
        PluginRuntime::has_export(self.source, fn_name, self.host_ctx)
    }

    pub fn verify(&self, ctx: &OjContext) -> Result<OjVerifyResult> {
        let arg = serde_json::to_value(ctx)?;
        let res = PluginRuntime::call_function(self.source, "verify", &[arg], self.host_ctx)?;
        Ok(serde_json::from_value(res.value)?)
    }

    pub fn sync_account(&self, ctx: &OjContext) -> Result<OjAccountSync> {
        let arg = serde_json::to_value(ctx)?;
        let res = PluginRuntime::call_function(self.source, "syncAccount", &[arg], self.host_ctx)?;
        Ok(serde_json::from_value(res.value)?)
    }

    pub fn sync_summary(&self, ctx: &OjContext) -> Result<OjSummarySync> {
        let arg = serde_json::to_value(ctx)?;
        let res = PluginRuntime::call_function(self.source, "syncSummary", &[arg], self.host_ctx)?;
        Ok(serde_json::from_value(res.value)?)
    }

    pub fn sync_submissions(
        &self,
        ctx: &OjContext,
        cursor: Option<&str>,
    ) -> Result<OjSubmissionsBatch> {
        let arg0 = serde_json::to_value(ctx)?;
        let arg1 = match cursor {
            Some(c) => serde_json::Value::String(c.to_string()),
            None => serde_json::Value::Null,
        };
        let res = PluginRuntime::call_function(
            self.source,
            "syncSubmissions",
            &[arg0, arg1],
            self.host_ctx,
        )?;
        Ok(serde_json::from_value(res.value)?)
    }

    pub fn fetch_contests(&self, ctx: &OjContext) -> Result<Vec<OjContest>> {
        let arg = serde_json::to_value(ctx)?;
        let res =
            PluginRuntime::call_function(self.source, "fetchContests", &[arg], self.host_ctx)?;
        Ok(serde_json::from_value(res.value)?)
    }

    pub fn normalize_rating(&self, raw: f64) -> Result<Option<i64>> {
        let arg = serde_json::json!(raw);
        let res =
            PluginRuntime::call_function(self.source, "normalizeRating", &[arg], self.host_ctx)?;
        if res.value.is_null() {
            Ok(None)
        } else {
            Ok(Some(serde_json::from_value(res.value)?))
        }
    }

    pub fn parse_problem_url(&self, url: &str) -> Result<Option<ParsedProblemUrl>> {
        let arg = serde_json::Value::String(url.to_string());
        let res =
            PluginRuntime::call_function(self.source, "parseProblemUrl", &[arg], self.host_ctx)?;
        if res.value.is_null() {
            Ok(None)
        } else {
            Ok(Some(serde_json::from_value(res.value)?))
        }
    }

    pub fn fetch_problem(&self, ctx: &OjContext, id: &str) -> Result<Option<OjProblemDetail>> {
        let arg0 = serde_json::to_value(ctx)?;
        let arg1 = serde_json::Value::String(id.to_string());
        let res = PluginRuntime::call_function(
            self.source,
            "fetchProblem",
            &[arg0, arg1],
            self.host_ctx,
        )?;
        if res.value.is_null() {
            Ok(None)
        } else {
            Ok(Some(serde_json::from_value(res.value)?))
        }
    }
}
