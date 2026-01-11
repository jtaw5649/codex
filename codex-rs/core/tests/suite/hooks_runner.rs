#![cfg(not(target_os = "windows"))]

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use anyhow::Result;
use codex_core::hooks::HookDecision;
use codex_core::hooks::HookDecisionKind;
use codex_core::hooks::HookError;
use codex_core::hooks::HookRunner;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hook_runner_parses_decision_payload() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let script = temp_dir.path().join("hook.sh");
    std::fs::write(
        &script,
        r#"#!/bin/bash
set -e
cat >/dev/null
echo '{"decision":"block","reason":"policy"}'
"#,
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;

    let runner = HookRunner::new(Duration::from_secs(2));
    let result = runner
        .run(
            vec![
                "/bin/bash".to_string(),
                script.to_string_lossy().to_string(),
            ],
            json!({ "hook_event_name": "PreToolUse" }),
            None,
        )
        .await?;

    assert_eq!(
        result,
        HookDecision {
            decision: Some(HookDecisionKind::Block),
            reason: Some("policy".to_string()),
        }
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hook_runner_rejects_invalid_json() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let script = temp_dir.path().join("hook.sh");
    std::fs::write(
        &script,
        r#"#!/bin/bash
set -e
cat >/dev/null
echo 'not json'
"#,
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;

    let runner = HookRunner::new(Duration::from_secs(2));
    let result = runner
        .run(
            vec![
                "/bin/bash".to_string(),
                script.to_string_lossy().to_string(),
            ],
            json!({ "hook_event_name": "PreToolUse" }),
            None,
        )
        .await;

    assert!(matches!(result, Err(HookError::InvalidResponse(_))));
    Ok(())
}
