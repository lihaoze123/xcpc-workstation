use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::credentials::MemoryCredentialStore;
use crate::error::{PluginError, Result};
use crate::http::UreqHttpClient;
use crate::kv::KvStore;
use crate::manager::PluginManager;
use crate::manifest::{CommandContext, CommandSpec, PluginCapability};
use crate::runtime::{HostContext, PluginRuntime, SandboxLimits};
use crate::tool::ToolExecutionResult;
use crate::workspace::WorkspaceReader;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredPluginCommand {
    pub plugin_id: String,
    pub plugin_name: String,
    pub command: CommandSpec,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginCommandAction {
    Message {
        text: String,
    },
    OpenTool {
        plugin_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        initial_inputs: Option<serde_json::Value>,
    },
    ToolResult {
        result: ToolExecutionResult,
    },
    None,
}

impl PluginManager {
    /// Lists all commands declared by enabled plugins with the `commands` capability.
    ///
    /// When `context` is `Some(ctx)`, returns commands matching `ctx` as well as `Global` commands.
    /// When `context` is `None`, returns all registered commands.
    pub fn list_commands(&self, context: Option<CommandContext>) -> Vec<RegisteredPluginCommand> {
        let Ok(plugins) = self.list_plugins() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for p in plugins {
            if !p.enabled
                || p.pending_trust
                || !p.manifest.has_capability(PluginCapability::Commands)
            {
                continue;
            }
            for cmd in p.manifest.commands {
                let matches_ctx = match context {
                    None => true,
                    Some(CommandContext::Global) => cmd.context == CommandContext::Global,
                    Some(target) => cmd.context == target || cmd.context == CommandContext::Global,
                };
                if matches_ctx {
                    out.push(RegisteredPluginCommand {
                        plugin_id: p.manifest.id.clone(),
                        plugin_name: p.manifest.name.clone(),
                        command: cmd,
                    });
                }
            }
        }
        out
    }

    /// Invokes a plugin command via `runCommand(commandId, ctx)` in the QuickJS sandbox.
    pub fn run_command(
        &self,
        plugin_id: &str,
        command_id: &str,
        context_payload: serde_json::Value,
        workspace: Option<Arc<dyn WorkspaceReader>>,
    ) -> Result<PluginCommandAction> {
        self.run_command_with_writer(plugin_id, command_id, context_payload, workspace, None)
    }

    /// Invokes a plugin command with optional workspace read and write bridges.
    pub fn run_command_with_writer(
        &self,
        plugin_id: &str,
        command_id: &str,
        context_payload: serde_json::Value,
        workspace: Option<Arc<dyn WorkspaceReader>>,
        workspace_writer: Option<Arc<dyn crate::workspace::WorkspaceWriter>>,
    ) -> Result<PluginCommandAction> {
        let loaded = self.load_plugin(plugin_id)?;
        if !loaded.info.enabled {
            return Err(PluginError::Disabled(plugin_id.to_string()));
        }
        if !loaded
            .info
            .manifest
            .has_capability(PluginCapability::Commands)
        {
            return Err(PluginError::PermissionDenied {
                plugin_id: plugin_id.to_string(),
                permission: "commands".to_string(),
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
            workspace_writer,
            limits: SandboxLimits::default(),
        };

        let args = [serde_json::json!(command_id), context_payload];
        match PluginRuntime::call_function(&loaded.entry_source, "runCommand", &args, &host_ctx) {
            Ok(call) => {
                let action = if call.value.is_null() {
                    PluginCommandAction::None
                } else {
                    serde_json::from_value(call.value)?
                };
                let _ = self.clear_error(plugin_id);
                Ok(action)
            }
            Err(err) => {
                let _ = self.record_error(plugin_id, err.to_string());
                Err(err)
            }
        }
    }
}
