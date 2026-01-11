use crate::client_common::tools::ToolSpec;
use crate::codex::Session;
use crate::codex::TurnContext;
use crate::function_tool::FunctionCallError;
use crate::hooks::HookContext;
use crate::hooks::HookDecisionKind;
use crate::hooks::HookDecisionWithContext;
use crate::hooks::ToolHookPayload;
use crate::hooks::build_apply_patch_hook_payloads;
use crate::protocol::EventMsg;
use crate::protocol::HookActivityEvent;
use crate::protocol::HookActivityHook;
use crate::protocol::HookActivityStatus;
use crate::protocol::HookActivityTool;
use crate::protocol::WarningEvent;
use crate::sandboxing::SandboxPermissions;
use crate::tools::context::SharedTurnDiffTracker;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ConfiguredToolSpec;
use crate::tools::registry::ToolRegistry;
use crate::tools::spec::ApplyPatchToolArgs;
use crate::tools::spec::ToolsConfig;
use crate::tools::spec::build_specs;
use codex_apply_patch::MaybeApplyPatchVerified;
use codex_protocol::models::LocalShellAction;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::ShellToolCallParams;
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::instrument;

#[derive(Clone, Debug)]
pub struct ToolCall {
    pub tool_name: String,
    pub call_id: String,
    pub payload: ToolPayload,
}

pub struct ToolRouter {
    registry: ToolRegistry,
    specs: Vec<ConfiguredToolSpec>,
}

impl ToolRouter {
    pub fn from_config(
        config: &ToolsConfig,
        mcp_tools: Option<HashMap<String, mcp_types::Tool>>,
    ) -> Self {
        let builder = build_specs(config, mcp_tools);
        let (specs, registry) = builder.build();

        Self { registry, specs }
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.specs
            .iter()
            .map(|config| config.spec.clone())
            .collect()
    }

    pub fn tool_supports_parallel(&self, tool_name: &str) -> bool {
        self.specs
            .iter()
            .filter(|config| config.supports_parallel_tool_calls)
            .any(|config| config.spec.name() == tool_name)
    }

    #[instrument(level = "trace", skip_all, err)]
    pub async fn build_tool_call(
        session: &Session,
        item: ResponseItem,
    ) -> Result<Option<ToolCall>, FunctionCallError> {
        match item {
            ResponseItem::FunctionCall {
                name,
                arguments,
                call_id,
                ..
            } => {
                if let Some((server, tool)) = session.parse_mcp_tool_name(&name).await {
                    Ok(Some(ToolCall {
                        tool_name: name,
                        call_id,
                        payload: ToolPayload::Mcp {
                            server,
                            tool,
                            raw_arguments: arguments,
                        },
                    }))
                } else {
                    Ok(Some(ToolCall {
                        tool_name: name,
                        call_id,
                        payload: ToolPayload::Function { arguments },
                    }))
                }
            }
            ResponseItem::CustomToolCall {
                name,
                input,
                call_id,
                ..
            } => Ok(Some(ToolCall {
                tool_name: name,
                call_id,
                payload: ToolPayload::Custom { input },
            })),
            ResponseItem::LocalShellCall {
                id,
                call_id,
                action,
                ..
            } => {
                let call_id = call_id
                    .or(id)
                    .ok_or(FunctionCallError::MissingLocalShellCallId)?;

                match action {
                    LocalShellAction::Exec(exec) => {
                        let params = ShellToolCallParams {
                            command: exec.command,
                            workdir: exec.working_directory,
                            timeout_ms: exec.timeout_ms,
                            sandbox_permissions: Some(SandboxPermissions::UseDefault),
                            justification: None,
                        };
                        Ok(Some(ToolCall {
                            tool_name: "local_shell".to_string(),
                            call_id,
                            payload: ToolPayload::LocalShell { params },
                        }))
                    }
                }
            }
            _ => Ok(None),
        }
    }

    #[instrument(level = "trace", skip_all, err)]
    pub async fn dispatch_tool_call(
        &self,
        session: Arc<Session>,
        turn: Arc<TurnContext>,
        tracker: SharedTurnDiffTracker,
        call: ToolCall,
    ) -> Result<ResponseInputItem, FunctionCallError> {
        let ToolCall {
            tool_name,
            call_id,
            payload,
        } = call;
        let payload_outputs_custom = matches!(payload, ToolPayload::Custom { .. });
        let failure_call_id = call_id.clone();
        let hook_session = session.clone();
        let pre_hook_payloads = build_tool_hook_payloads(
            session.as_ref(),
            turn.as_ref(),
            &tool_name,
            &payload,
            "PreToolUse",
        )
        .await;

        if let Some(blocked) =
            pre_tool_use_decision(session.as_ref(), pre_hook_payloads.as_deref()).await?
        {
            let reason = blocked
                .decision
                .reason
                .unwrap_or_else(|| "blocked by hook".to_string());
            let message = format!("blocked by hook: {reason}");
            let display_tool_name = pre_hook_payloads
                .as_ref()
                .and_then(|payloads| payloads.first())
                .map(|payload| payload.tool_name.clone())
                .unwrap_or_else(|| tool_name.clone());
            let hook_name = blocked
                .hook
                .command
                .first()
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());
            let hook_decision = blocked
                .decision
                .decision
                .as_ref()
                .map(|decision| match decision {
                    HookDecisionKind::Block => "block",
                    HookDecisionKind::Allow => "allow",
                })
                .unwrap_or("block")
                .to_string();
            session
                .send_event(
                    turn.as_ref(),
                    EventMsg::HookActivity(HookActivityEvent {
                        status: HookActivityStatus::Blocked,
                        tool: Some(HookActivityTool {
                            name: display_tool_name.clone(),
                            past_tense: tool_past_tense(&display_tool_name),
                        }),
                        hooks: vec![HookActivityHook {
                            name: hook_name,
                            decision: hook_decision,
                        }],
                        reason: Some(reason.clone()),
                    }),
                )
                .await;
            session
                .send_event(
                    turn.as_ref(),
                    EventMsg::Warning(WarningEvent {
                        message: message.clone(),
                    }),
                )
                .await;
            let _ = session
                .inject_input(vec![codex_protocol::user_input::UserInput::Text {
                    text: reason.clone(),
                }])
                .await;
            return Ok(Self::failure_response(
                failure_call_id,
                payload_outputs_custom,
                FunctionCallError::RespondToModel(message),
            ));
        }

        let invocation = ToolInvocation {
            session,
            turn,
            tracker,
            call_id,
            tool_name,
            payload,
        };

        let response = match self.registry.dispatch(invocation).await {
            Ok(response) => response,
            Err(FunctionCallError::Fatal(message)) => {
                return Err(FunctionCallError::Fatal(message));
            }
            Err(err) => Self::failure_response(failure_call_id, payload_outputs_custom, err),
        };

        if let Some(hooks) = hook_session.services.hooks.as_ref()
            && let Some(payloads) = pre_hook_payloads.as_ref()
        {
            let post_payloads: Vec<ToolHookPayload> = payloads
                .iter()
                .map(|payload| ToolHookPayload {
                    hook_event_name: "PostToolUse".to_string(),
                    ..payload.clone()
                })
                .collect();
            let _ = hooks.run_post_tool_use(&post_payloads).await;
        }

        Ok(response)
    }

    fn failure_response(
        call_id: String,
        payload_outputs_custom: bool,
        err: FunctionCallError,
    ) -> ResponseInputItem {
        let message = err.to_string();
        if payload_outputs_custom {
            ResponseInputItem::CustomToolCallOutput {
                call_id,
                output: message,
            }
        } else {
            ResponseInputItem::FunctionCallOutput {
                call_id,
                output: codex_protocol::models::FunctionCallOutputPayload {
                    content: message,
                    success: Some(false),
                    ..Default::default()
                },
            }
        }
    }
}

async fn build_tool_hook_payloads(
    session: &Session,
    turn: &TurnContext,
    tool_name: &str,
    payload: &ToolPayload,
    hook_event_name: &str,
) -> Option<Vec<ToolHookPayload>> {
    let transcript_path = session
        .rollout_path()
        .await
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let context = HookContext {
        session_id: session.conversation_id().to_string(),
        transcript_path,
        hook_event_name: hook_event_name.to_string(),
    };

    if tool_name == "apply_patch" {
        return build_apply_patch_payloads(&context, turn, payload);
    }

    let tool_input = match payload {
        ToolPayload::Function { arguments } => {
            serde_json::from_str(arguments).unwrap_or_else(|_| serde_json::json!(arguments))
        }
        ToolPayload::Custom { input } => serde_json::json!(input),
        ToolPayload::LocalShell { params } => serde_json::json!({
            "command": params.command,
            "workdir": params.workdir,
            "timeout_ms": params.timeout_ms,
            "sandbox_permissions": params.sandbox_permissions,
            "justification": params.justification,
        }),
        ToolPayload::Mcp {
            server,
            tool,
            raw_arguments,
        } => {
            let arguments = serde_json::from_str(raw_arguments)
                .unwrap_or_else(|_| serde_json::json!(raw_arguments));
            serde_json::json!({
                "server": server,
                "tool": tool,
                "arguments": arguments,
            })
        }
    };

    Some(vec![ToolHookPayload {
        session_id: context.session_id,
        transcript_path: context.transcript_path,
        hook_event_name: context.hook_event_name,
        tool_name: tool_name.to_string(),
        tool_input,
    }])
}

fn build_apply_patch_payloads(
    context: &HookContext,
    turn: &TurnContext,
    payload: &ToolPayload,
) -> Option<Vec<ToolHookPayload>> {
    let patch_input = match payload {
        ToolPayload::Function { arguments } => {
            match serde_json::from_str::<ApplyPatchToolArgs>(arguments) {
                Ok(args) => args.input,
                Err(_) => return None,
            }
        }
        ToolPayload::Custom { input } => input.clone(),
        _ => return None,
    };

    let command = vec!["apply_patch".to_string(), patch_input];
    let action = match codex_apply_patch::maybe_parse_apply_patch_verified(&command, &turn.cwd) {
        MaybeApplyPatchVerified::Body(action) => action,
        _ => return None,
    };

    let payloads = build_apply_patch_hook_payloads(context, &action);
    if payloads.is_empty() {
        None
    } else {
        Some(payloads)
    }
}

async fn pre_tool_use_decision(
    session: &Session,
    payloads: Option<&[ToolHookPayload]>,
) -> Result<Option<HookDecisionWithContext>, FunctionCallError> {
    let Some(hooks) = session.services.hooks.as_ref() else {
        return Ok(None);
    };

    let Some(payloads) = payloads else {
        return Ok(None);
    };

    hooks
        .run_pre_tool_use(payloads)
        .await
        .map_err(|err| FunctionCallError::RespondToModel(format!("hook error: {err}")))
}

fn tool_past_tense(tool_name: &str) -> String {
    match tool_name {
        "Edit" | "MultiEdit" => "Edited",
        "Write" => "Wrote",
        "TodoWrite" => "Updated",
        "Read" => "Read",
        "List" => "Listed",
        "Shell" => "Ran",
        _ => "Ran",
    }
    .to_string()
}
