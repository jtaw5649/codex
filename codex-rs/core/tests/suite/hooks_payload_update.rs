#![cfg(not(target_os = "windows"))]

use anyhow::Result;
use codex_apply_patch::MaybeApplyPatchVerified;
use codex_apply_patch::maybe_parse_apply_patch_verified;
use codex_core::hooks::HookContext;
use codex_core::hooks::ToolHookPayload;
use codex_core::hooks::build_apply_patch_hook_payloads;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::fs;
use tempfile::tempdir;

#[test]
fn apply_patch_update_maps_to_edit_payload() -> Result<()> {
    let tmp = tempdir()?;
    let path = tmp.path().join("source.txt");
    fs::write(&path, "old\n")?;

    let argv = vec![
        "apply_patch".to_string(),
        r#"*** Begin Patch
*** Update File: source.txt
@@
-old
+new
*** End Patch"#
            .to_string(),
    ];

    let result = maybe_parse_apply_patch_verified(&argv, tmp.path());
    let action = match result {
        MaybeApplyPatchVerified::Body(action) => action,
        other => panic!("expected verified patch, got {other:?}"),
    };

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
            tool_name: "Edit".to_string(),
            tool_input: json!({
                "file_path": path.to_string_lossy(),
                "old_string": "old\n",
                "new_string": "new\n",
            }),
        }]
    );

    Ok(())
}
