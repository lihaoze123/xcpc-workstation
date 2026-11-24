use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{PluginError, Result};

fn default_entry() -> String {
    "index.js".to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    OjSource,
    Tool,
    Commands,
    Importer,
    Exporter,
    Credentials,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginPermissions {
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub workspace_read: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialFieldSpec {
    pub key: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub secret: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolInputSpec {
    pub name: String,
    pub label: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub inputs: Vec<ToolInputSpec>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommandContext {
    #[default]
    Global,
    Problem,
    Contest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSpec {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub context: CommandContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImporterSpec {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub extensions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExporterSpec {
    pub id: String,
    pub name: String,
    pub extension: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default = "default_entry")]
    pub entry: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_entry: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<PluginCapability>,
    #[serde(default)]
    pub permissions: PluginPermissions,
    #[serde(default)]
    pub credentials: Vec<CredentialFieldSpec>,
    #[serde(default)]
    pub tools: Vec<ToolSpec>,
    #[serde(default)]
    pub commands: Vec<CommandSpec>,
    #[serde(default)]
    pub importers: Vec<ImporterSpec>,
    #[serde(default)]
    pub exporters: Vec<ExporterSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginPermissionSummary {
    pub plugin_id: String,
    pub domains: Vec<String>,
    pub credentials: Vec<String>,
    pub workspace_read: bool,
    pub has_unsandboxed_ui: bool,
}

impl PluginManifest {
    pub fn from_toml_str(src: &str) -> Result<Self> {
        let manifest: Self = toml::from_str(src)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        Self::from_toml_str(&raw)
    }

    pub fn validate(&self) -> Result<()> {
        validate_plugin_id(&self.id)?;
        if self.name.trim().is_empty() {
            return Err(PluginError::ManifestInvalid(
                "manifest.name cannot be empty".into(),
            ));
        }
        if self.version.trim().is_empty() {
            return Err(PluginError::ManifestInvalid(
                "manifest.version cannot be empty".into(),
            ));
        }
        validate_relative_entry(&self.entry, "entry")?;
        if let Some(ref ui) = self.ui_entry {
            validate_relative_entry(ui, "ui_entry")?;
        }
        for domain in &self.permissions.domains {
            let trimmed = domain.trim();
            if trimmed.is_empty() || trimmed == "*" || trimmed == "*." {
                return Err(PluginError::ManifestInvalid(format!(
                    "invalid domain rule '{domain}'"
                )));
            }
        }
        Ok(())
    }

    pub fn has_capability(&self, cap: PluginCapability) -> bool {
        self.capabilities.contains(&cap)
    }

    pub fn permission_summary(&self) -> PluginPermissionSummary {
        PluginPermissionSummary {
            plugin_id: self.id.clone(),
            domains: self.permissions.domains.clone(),
            credentials: self.credentials.iter().map(|c| c.key.clone()).collect(),
            workspace_read: self.permissions.workspace_read,
            has_unsandboxed_ui: self.ui_entry.is_some(),
        }
    }

    pub fn is_domain_allowed(&self, host: &str) -> bool {
        is_domain_allowed(&self.permissions.domains, host)
    }
}

pub fn validate_plugin_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 64 {
        return Err(PluginError::ManifestInvalid(format!(
            "plugin id '{id}' must be 1..=64 chars"
        )));
    }
    let mut chars = id.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err(PluginError::ManifestInvalid(format!(
            "plugin id '{id}' must start with lowercase ascii letter or digit"
        )));
    }
    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
        return Err(PluginError::ManifestInvalid(format!(
            "plugin id '{id}' may only contain lowercase ascii letters, digits, '-' or '_'"
        )));
    }
    Ok(())
}

fn validate_relative_entry(entry: &str, field: &str) -> Result<()> {
    let p = Path::new(entry);
    if entry.trim().is_empty() || p.is_absolute() {
        return Err(PluginError::ManifestInvalid(format!(
            "manifest.{field} must be a non-empty relative path"
        )));
    }
    for comp in p.components() {
        if matches!(
            comp,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        ) {
            return Err(PluginError::ManifestInvalid(format!(
                "manifest.{field} '{entry}' cannot escape plugin directory"
            )));
        }
    }
    Ok(())
}

/// Checks whether `host` matches any rule in `allowed_domains`.
/// Supports exact host match (case-insensitive) and wildcard prefix `*.example.com`.
pub fn is_domain_allowed(allowed_domains: &[String], host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }
    for rule in allowed_domains {
        let rule = rule.trim().trim_end_matches('.').to_ascii_lowercase();
        if rule.is_empty() || rule == "*" || rule == "*." {
            continue;
        }
        if let Some(suffix) = rule.strip_prefix("*.") {
            if !suffix.is_empty()
                && host.len() > suffix.len() + 1
                && host.ends_with(suffix)
                && host.as_bytes()[host.len() - suffix.len() - 1] == b'.'
            {
                return true;
            }
        } else if host == rule {
            return true;
        }
    }
    false
}
