#![cfg(not(target_os = "windows"))]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use anyhow::Result;
use codex_core::hooks::HookCommandConfig;
use codex_core::hooks::HooksConfig;
use core_test_support::fs_wait;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::TestCodexHarness;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_start_invokes_hook() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let hook_dir = TempDir::new()?;
    let output_path = hook_dir.path().join("payload.json");
    let script = hook_dir.path().join("hook.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/bash\nset -e\ncat > {}\necho '{{}}'\n",
            output_path.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let script_str = script.to_string_lossy().to_string();

    let builder = test_codex().with_config(move |config| {
        config.hooks = Some(HooksConfig {
            session_start: vec![HookCommandConfig {
                matcher: None,
                command: vec![script_str],
                timeout_ms: None,
            }],
            ..Default::default()
        });
    });

    let _harness = TestCodexHarness::with_builder(builder).await?;

    fs_wait::wait_for_path_exists(&output_path, Duration::from_secs(5)).await?;
    let payload_raw = fs::read_to_string(&output_path)?;
    let payload: Value = serde_json::from_str(&payload_raw)?;

    assert_eq!(payload["hook_event_name"], "SessionStart");
    assert_eq!(payload["source"], "startup");
    assert!(payload["session_id"].as_str().is_some());
    assert!(payload["transcript_path"].as_str().is_some());

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_prompt_submit_invokes_hook() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let hook_dir = TempDir::new()?;
    let output_path = hook_dir.path().join("payload.json");
    let script = hook_dir.path().join("hook.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/bash\nset -e\ncat > {}\necho '{{}}'\n",
            output_path.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let script_str = script.to_string_lossy().to_string();

    let builder = test_codex().with_config(move |config| {
        config.hooks = Some(HooksConfig {
            user_prompt_submit: vec![HookCommandConfig {
                matcher: None,
                command: vec![script_str],
                timeout_ms: None,
            }],
            ..Default::default()
        });
    });

    let harness = TestCodexHarness::with_builder(builder).await?;

    core_test_support::responses::mount_sse_sequence(
        harness.server(),
        vec![core_test_support::responses::sse(vec![
            core_test_support::responses::ev_response_created("resp-1"),
            core_test_support::responses::ev_assistant_message("msg-1", "done"),
            core_test_support::responses::ev_completed("resp-1"),
        ])],
    )
    .await;

    let prompt = "hello hooks";
    harness.submit(prompt).await?;

    fs_wait::wait_for_path_exists(&output_path, Duration::from_secs(5)).await?;
    let payload_raw = fs::read_to_string(&output_path)?;
    let payload: Value = serde_json::from_str(&payload_raw)?;

    assert_eq!(payload["hook_event_name"], "UserPromptSubmit");
    assert_eq!(payload["prompt"], prompt);
    assert_eq!(payload["cwd"], harness.cwd().display().to_string());
    assert!(payload["session_id"].as_str().is_some());
    assert!(payload["transcript_path"].as_str().is_some());

    Ok(())
}
