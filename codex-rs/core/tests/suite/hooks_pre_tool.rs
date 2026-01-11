#![cfg(not(target_os = "windows"))]

use std::fs;
use std::os::unix::fs::PermissionsExt;

use anyhow::Result;
use codex_core::hooks::HookCommandConfig;
use codex_core::hooks::HooksConfig;
use codex_core::protocol::AskForApproval;
use core_test_support::responses::ev_apply_patch_call;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::ApplyPatchModelOutput;
use core_test_support::test_codex::TestCodexHarness;
use core_test_support::test_codex::test_codex;
use codex_core::protocol::EventMsg;
use codex_core::protocol::HookActivityHook;
use codex_core::protocol::HookActivityStatus;
use codex_core::protocol::HookActivityTool;
use codex_core::protocol::WarningEvent;
use codex_core::protocol::Op;
use codex_core::protocol::SandboxPolicy;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::user_input::UserInput;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use tokio::time::Duration;
use tokio::time::Instant;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_tool_use_blocks_apply_patch() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let hook_dir = TempDir::new()?;
    let script = hook_dir.path().join("hook.sh");
    std::fs::write(
        &script,
        r#"#!/bin/bash
set -e
cat >/dev/null
echo '{"decision":"block","reason":"blocked by hook"}'
"#,
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let script_str = script.to_string_lossy().to_string();

    let hook_command = script_str.clone();
    let builder = test_codex().with_config(move |config| {
        config.include_apply_patch_tool = true;
        config.hooks = Some(HooksConfig {
            pre_tool_use: vec![HookCommandConfig {
                matcher: Some("Edit".to_string()),
                command: vec![hook_command],
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
    let call_id = "blocked-apply-patch";

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

    let output = harness
        .apply_patch_output(call_id, ApplyPatchModelOutput::Function)
        .await;
    assert!(output.contains("blocked by hook"));
    assert_eq!(fs::read_to_string(&target)?, "original\n");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_tool_use_blocks_shell_command() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let hook_dir = TempDir::new()?;
    let script = hook_dir.path().join("hook.sh");
    std::fs::write(
        &script,
        r#"#!/bin/bash
set -e
cat >/dev/null
echo '{"decision":"block","reason":"blocked by hook"}'
"#,
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let script_str = script.to_string_lossy().to_string();

    let builder = test_codex().with_config(move |config| {
        config.hooks = Some(HooksConfig {
            pre_tool_use: vec![HookCommandConfig {
                matcher: Some("shell_command".to_string()),
                command: vec![script_str],
                timeout_ms: None,
            }],
            ..Default::default()
        });
    });

    let harness = TestCodexHarness::with_builder(builder).await?;
    let call_id = "blocked-shell-command";
    let args = json!({
        "command": "echo blocked",
        "timeout_ms": 1_000,
    });

    mount_sse_sequence(
        harness.server(),
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(call_id, "shell_command", &serde_json::to_string(&args)?),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    harness.submit("run command").await?;

    let output = harness.function_call_stdout(call_id).await;
    assert!(output.contains("blocked by hook"));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_tool_use_block_injects_user_prompt() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let hook_dir = TempDir::new()?;
    let script = hook_dir.path().join("hook.sh");
    let reason = "tdd-guard: write the failing test first";
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/bash
set -e
cat >/dev/null
echo '{{"decision":"block","reason":"{reason}"}}'
"#
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let script_str = script.to_string_lossy().to_string();

    let builder = test_codex().with_config(move |config| {
        config.hooks = Some(HooksConfig {
            pre_tool_use: vec![HookCommandConfig {
                matcher: Some("shell_command".to_string()),
                command: vec![script_str],
                timeout_ms: None,
            }],
            ..Default::default()
        });
    });

    let harness = TestCodexHarness::with_builder(builder).await?;
    let call_id = "blocked-shell-command-prompt";
    let args = json!({
        "command": "echo blocked",
        "timeout_ms": 1_000,
    });

    let mock = mount_sse_sequence(
        harness.server(),
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(call_id, "shell_command", &serde_json::to_string(&args)?),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    harness.submit("run command").await?;

    let output = harness.function_call_stdout(call_id).await;
    assert!(output.contains("blocked by hook"));

    let requests = mock.requests();
    assert!(requests.len() >= 2, "expected follow-up request");
    let follow_up = requests.last().expect("follow-up request");
    let user_texts = follow_up.message_input_texts("user");
    assert!(
        user_texts.iter().any(|text| text == reason),
        "expected hook reason to be injected as a user prompt"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_tool_use_emits_hook_activity_event() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let hook_dir = TempDir::new()?;
    let script = hook_dir.path().join("hook.sh");
    let reason = "tdd-guard: write the failing test first";
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/bash
set -e
cat >/dev/null
echo '{{"decision":"block","reason":"{reason}"}}'
"#
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let script_str = script.to_string_lossy().to_string();
    let hook_command = script_str.clone();

    let builder = test_codex().with_config(move |config| {
        config.include_apply_patch_tool = true;
        config.hooks = Some(HooksConfig {
            pre_tool_use: vec![HookCommandConfig {
                matcher: Some("Edit".to_string()),
                command: vec![hook_command],
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
    let call_id = "blocked-apply-patch-hook-activity";

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

    let test = harness.test();
    let session_model = test.session_configured.model.clone();
    test.codex
        .submit(Op::UserTurn {
            items: vec![UserInput::Text {
                text: "apply patch".into(),
            }],
            final_output_json_schema: None,
            cwd: test.cwd_path().to_path_buf(),
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::DangerFullAccess,
            model: session_model,
            effort: None,
            summary: ReasoningSummary::Auto,
        })
        .await?;

    let mut hook_event = None;
    let mut warning_message = None;
    let mut saw_turn_complete = false;
    let deadline = Instant::now() + Duration::from_secs(10);

    while Instant::now() < deadline
        && (!saw_turn_complete || hook_event.is_none() || warning_message.is_none())
    {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let event = tokio::time::timeout(remaining, test.codex.next_event())
            .await
            .expect("timeout waiting for event")?;
        match event.msg {
            EventMsg::HookActivity(ev) => hook_event = Some(ev),
            EventMsg::Warning(WarningEvent { message }) => warning_message = Some(message),
            EventMsg::TurnComplete(_) => saw_turn_complete = true,
            _ => {}
        }
    }

    let hook_event = hook_event.expect("expected HookActivity event");
    assert_eq!(hook_event.status, HookActivityStatus::Blocked);
    assert_eq!(hook_event.reason.as_deref(), Some(reason));
    assert_eq!(
        hook_event.tool,
        Some(HookActivityTool {
            name: "Edit".into(),
            past_tense: "Edited".into(),
        })
    );
    assert_eq!(
        hook_event.hooks,
        vec![HookActivityHook {
            name: script_str.clone(),
            decision: "block".into(),
        }]
    );
    let expected_warning = format!("blocked by hook: {reason}");
    assert_eq!(warning_message.as_deref(), Some(expected_warning.as_str()));

    Ok(())
}
