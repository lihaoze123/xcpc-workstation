use std::path::{Component, Path};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::credentials::MemoryCredentialStore;
use crate::error::{PluginError, Result};
use crate::http::UreqHttpClient;
use crate::kv::KvStore;
use crate::manager::{PluginManager, PluginOrigin};
use crate::manifest::{PluginCapability, ToolSpec};
use crate::runtime::{HostContext, PluginRuntime, SandboxLimits};
use crate::workspace::WorkspaceReader;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directed: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolOutputBlock {
    Text {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        monospace: Option<bool>,
    },
    Markdown {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        content: String,
    },
    Table {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        align: Option<Vec<String>>,
    },
    Graph {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        directed: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weighted: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        one_indexed: Option<bool>,
        #[serde(default)]
        nodes: Vec<GraphNode>,
        #[serde(default)]
        edges: Vec<GraphEdge>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layout: Option<String>,
    },
    Svg {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        svg: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height: Option<u32>,
    },
    Image {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        png_base64: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height: Option<u32>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolExecutionResult {
    #[serde(default)]
    pub blocks: Vec<ToolOutputBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginToolGroup {
    pub plugin_id: String,
    pub plugin_name: String,
    pub plugin_description: String,
    pub has_ui: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_entry: Option<String>,
    #[serde(default)]
    pub tools: Vec<ToolSpec>,
    pub is_builtin: bool,
    #[serde(default)]
    pub is_linked: bool,
}

impl PluginManager {
    /// Lists all enabled plugins that declare the `tool` capability.
    pub fn list_tools(&self) -> Vec<PluginToolGroup> {
        let Ok(plugins) = self.list_plugins() else {
            return Vec::new();
        };
        plugins
            .into_iter()
            .filter(|p| p.enabled && p.manifest.has_capability(PluginCapability::Tool))
            .map(|p| {
                let has_ui = p.manifest.ui_entry.is_some();
                PluginToolGroup {
                    plugin_id: p.manifest.id,
                    plugin_name: p.manifest.name,
                    plugin_description: p.manifest.description,
                    has_ui,
                    ui_entry: p.manifest.ui_entry,
                    tools: p.manifest.tools,
                    is_builtin: matches!(p.origin, PluginOrigin::Builtin),
                    is_linked: p.is_linked,
                }
            })
            .collect()
    }

    /// Executes a headless tool action `runTool(toolId, inputs, ctx)` in the plugin's QuickJS sandbox.
    pub fn run_tool(
        &self,
        plugin_id: &str,
        tool_id: &str,
        inputs: serde_json::Value,
        workspace: Option<Arc<dyn WorkspaceReader>>,
    ) -> Result<ToolExecutionResult> {
        let loaded = self.load_plugin(plugin_id)?;
        if !loaded.info.enabled {
            return Err(PluginError::Disabled(plugin_id.to_string()));
        }
        if !loaded.info.manifest.has_capability(PluginCapability::Tool) {
            return Err(PluginError::PermissionDenied {
                plugin_id: plugin_id.to_string(),
                permission: "tool".to_string(),
            });
        }

        let kv = self
            .kv_store(plugin_id)
            .unwrap_or_else(|_| KvStore::in_memory());
        let host_ctx = HostContext {
            manifest: loaded.info.manifest.clone(),
            http_client: Arc::new(UreqHttpClient::new()),
            credentials: Arc::new(MemoryCredentialStore::new()),
            kv: Arc::new(kv),
            workspace,
            limits: SandboxLimits::default(),
        };

        let ctx_arg = serde_json::json!({
            "plugin_id": plugin_id,
            "tool_id": tool_id,
        });
        let args = [serde_json::json!(tool_id), inputs, ctx_arg];
        match PluginRuntime::call_function(&loaded.entry_source, "runTool", &args, &host_ctx) {
            Ok(call) => {
                let result: ToolExecutionResult = serde_json::from_value(call.value)?;
                let _ = self.clear_error(plugin_id);
                Ok(result)
            }
            Err(err) => {
                let _ = self.record_error(plugin_id, err.to_string());
                Err(err)
            }
        }
    }

    /// Reads the `ui_entry` JS source of an enabled plugin (built-in or local override),
    /// enforcing strict path-escape checks for local directories.
    pub fn read_ui_source(&self, plugin_id: &str) -> Result<Option<String>> {
        let loaded = self.load_plugin(plugin_id)?;
        if !loaded.info.enabled {
            return Err(PluginError::Disabled(plugin_id.to_string()));
        }
        let Some(ref ui_entry) = loaded.info.manifest.ui_entry else {
            return Ok(None);
        };

        if let Some(ref dir) = loaded.info.local_dir {
            validate_safe_ui_path(ui_entry)?;
            let canonical_dir = std::fs::canonicalize(dir)?;
            let target = dir.join(ui_entry);
            let canonical_target = std::fs::canonicalize(&target).map_err(|e| {
                PluginError::InvalidPackage(format!("failed to resolve ui_entry '{ui_entry}': {e}"))
            })?;
            if !canonical_target.starts_with(&canonical_dir) {
                return Err(PluginError::ZipSlip(format!(
                    "ui_entry '{ui_entry}' escapes plugin directory"
                )));
            }
            let src = std::fs::read_to_string(&canonical_target)?;
            Ok(Some(src))
        } else {
            Ok(loaded.ui_source)
        }
    }
}

fn validate_safe_ui_path(rel: &str) -> Result<()> {
    if rel.trim().is_empty() || rel.contains('\\') || rel.starts_with('/') || rel.contains('\0') {
        return Err(PluginError::ZipSlip(rel.to_string()));
    }
    let path = Path::new(rel);
    if path.is_absolute() {
        return Err(PluginError::ZipSlip(rel.to_string()));
    }
    for comp in path.components() {
        match comp {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(PluginError::ZipSlip(rel.to_string()));
            }
        }
    }
    Ok(())
}
