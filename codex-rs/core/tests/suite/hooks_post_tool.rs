#![cfg(not(target_os = "windows"))]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use anyhow::Result;
use codex_core::hooks::HookCommandConfig;
use codex_core::hooks::HooksConfig;
use core_test_support::fs_wait;
use core_test_support::responses::ev_apply_patch_call;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::ApplyPatchModelOutput;
use core_test_support::test_codex::TestCodexHarness;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_tool_use_invokes_hook() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let hook_dir = TempDir::new()?;
    let output_path = hook_dir.path().join("payload.json");
    let script = hook_dir.path().join("hook.sh");
    std::fs::write(
        &script,
        format!("#!/bin/bash\nset -e\ncat > {}\n", output_path.display()),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let script_str = script.to_string_lossy().to_string();

    let builder = test_codex().with_config(move |config| {
        config.include_apply_patch_tool = true;
        config.hooks = Some(HooksConfig {
            post_tool_use: vec![HookCommandConfig {
                matcher: Some("Edit".to_string()),
                command: vec![script_str],
                timeout_ms: None,
            }],
            ..Default::default()
        });
    });

    let harness = TestCodexHarness::with_builder(builder).await?;

    let target = harness.path("file.txt");
    fs::write(&target, "original\n")?;

    let patch =
        "*** Begin Patch\n*** Update File: file.txt\n@@\n-original\n+updated\n*** End Patch";
    let call_id = "post-tool-apply-patch";

    mount_sse_sequence(
        harness.server(),
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_apply_patch_call(call_id, patch, ApplyPatchModelOutput::Function),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    harness.submit("apply patch").await?;

    fs_wait::wait_for_path_exists(&output_path, Duration::from_secs(5)).await?;
    let payload_raw = fs::read_to_string(&output_path)?;
    let payload: Value = serde_json::from_str(&payload_raw)?;

    assert_eq!(payload["hook_event_name"], "PostToolUse");
    assert_eq!(payload["tool_name"], "Edit");

    Ok(())
}
