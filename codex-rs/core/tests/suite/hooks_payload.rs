#![cfg(not(target_os = "windows"))]

use anyhow::Result;
use codex_apply_patch::ApplyPatchAction;
use codex_core::hooks::HookContext;
use codex_core::hooks::ToolHookPayload;
use codex_core::hooks::build_apply_patch_hook_payloads;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn apply_patch_add_maps_to_write_payload() -> Result<()> {
    let tmp = tempdir()?;
    let path = tmp.path().join("new.txt");
    let action = ApplyPatchAction::new_add_for_test(&path, "hello".to_string());

    let context = HookContext {
        session_id: "session-1".to_string(),
        transcript_path: "/tmp/rollout.jsonl".to_string(),
        hook_event_name: "PreToolUse".to_string(),
    };

    let payloads = build_apply_patch_hook_payloads(&context, &action);

    assert_eq!(
        payloads,
        vec![ToolHookPayload {
            session_id: "session-1".to_string(),
            transcript_path: "/tmp/rollout.jsonl".to_string(),
            hook_event_name: "PreToolUse".to_string(),
            tool_name: "Write".to_string(),
            tool_input: json!({
                "file_path": path.to_string_lossy(),
                "content": "hello",
            }),
        }]
    );

    Ok(())
}
