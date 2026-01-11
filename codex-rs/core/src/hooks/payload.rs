use codex_apply_patch::ApplyPatchAction;
use codex_apply_patch::ApplyPatchFileChange;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HookContext {
    pub session_id: String,
    pub transcript_path: String,
    pub hook_event_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolHookPayload {
    pub session_id: String,
    pub transcript_path: String,
    pub hook_event_name: String,
    pub tool_name: String,
    pub tool_input: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionStartHookPayload {
    pub session_id: String,
    pub transcript_path: String,
    pub hook_event_name: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserPromptSubmitHookPayload {
    pub session_id: String,
    pub transcript_path: String,
    pub hook_event_name: String,
    pub prompt: String,
    pub cwd: String,
}

pub fn build_apply_patch_hook_payloads(
    context: &HookContext,
    action: &ApplyPatchAction,
) -> Vec<ToolHookPayload> {
    let mut payloads = Vec::new();
    for (path, change) in action.changes() {
        match change {
            ApplyPatchFileChange::Add { content } => {
                payloads.push(ToolHookPayload {
                    session_id: context.session_id.clone(),
                    transcript_path: context.transcript_path.clone(),
                    hook_event_name: context.hook_event_name.clone(),
                    tool_name: "Write".to_string(),
                    tool_input: json!({
                        "file_path": path.to_string_lossy(),
                        "content": content,
                    }),
                });
            }
            ApplyPatchFileChange::Update {
                move_path,
                new_content,
                ..
            } => {
                let file_path = move_path.as_ref().unwrap_or(path);
                let old_content = fs::read_to_string(path).unwrap_or_default();
                payloads.push(ToolHookPayload {
                    session_id: context.session_id.clone(),
                    transcript_path: context.transcript_path.clone(),
                    hook_event_name: context.hook_event_name.clone(),
                    tool_name: "Edit".to_string(),
                    tool_input: json!({
                        "file_path": file_path.to_string_lossy(),
                        "old_string": old_content,
                        "new_string": new_content,
                    }),
                });
            }
            ApplyPatchFileChange::Delete { .. } => {}
        }
    }
    payloads
}

pub fn build_session_start_hook_payload(
    context: &HookContext,
    source: &str,
) -> SessionStartHookPayload {
    SessionStartHookPayload {
        session_id: context.session_id.clone(),
        transcript_path: context.transcript_path.clone(),
        hook_event_name: context.hook_event_name.clone(),
        source: source.to_string(),
    }
}

pub fn build_user_prompt_submit_hook_payload(
    context: &HookContext,
    prompt: &str,
    cwd: &Path,
) -> UserPromptSubmitHookPayload {
    UserPromptSubmitHookPayload {
        session_id: context.session_id.clone(),
        transcript_path: context.transcript_path.clone(),
        hook_event_name: context.hook_event_name.clone(),
        prompt: prompt.to_string(),
        cwd: cwd.display().to_string(),
    }
}
