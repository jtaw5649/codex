use regex::Regex;
use serde_json::to_value;
use std::time::Duration;

use crate::hooks::HookDecisionKind;
use crate::hooks::HookDecisionWithContext;
use crate::hooks::HookError;
use crate::hooks::HookRunner;
use crate::hooks::HooksConfig;
use crate::hooks::SessionStartHookPayload;
use crate::hooks::ToolHookPayload;
use crate::hooks::UserPromptSubmitHookPayload;

const DEFAULT_HOOK_TIMEOUT: Duration = Duration::from_secs(10);

pub struct HooksManager {
    config: HooksConfig,
    runner: HookRunner,
}

impl HooksManager {
    pub fn new(config: HooksConfig) -> Self {
        Self {
            config,
            runner: HookRunner::new(DEFAULT_HOOK_TIMEOUT),
        }
    }

    pub async fn run_pre_tool_use(
        &self,
        payloads: &[ToolHookPayload],
    ) -> Result<Option<HookDecisionWithContext>, HookError> {
        if self.config.pre_tool_use.is_empty() || payloads.is_empty() {
            return Ok(None);
        }

        for payload in payloads {
            for hook in &self.config.pre_tool_use {
                if !matcher_matches(hook.matcher.as_deref(), &payload.tool_name) {
                    continue;
                }

                let input = to_value(payload).map_err(HookError::SerializePayload)?;
                let timeout_override = hook.timeout_ms.map(Duration::from_millis);
                let decision = self
                    .runner
                    .run(hook.command.clone(), input, timeout_override)
                    .await?;
                if matches!(decision.decision, Some(HookDecisionKind::Block)) {
                    return Ok(Some(HookDecisionWithContext {
                        decision,
                        hook: hook.clone(),
                    }));
                }
            }
        }

        Ok(None)
    }

    pub async fn run_post_tool_use(&self, payloads: &[ToolHookPayload]) -> Result<(), HookError> {
        if self.config.post_tool_use.is_empty() || payloads.is_empty() {
            return Ok(());
        }

        for payload in payloads {
            for hook in &self.config.post_tool_use {
                if !matcher_matches(hook.matcher.as_deref(), &payload.tool_name) {
                    continue;
                }

                let input = to_value(payload).map_err(HookError::SerializePayload)?;
                let timeout_override = hook.timeout_ms.map(Duration::from_millis);
                let _ = self
                    .runner
                    .run(hook.command.clone(), input, timeout_override)
                    .await?;
            }
        }

        Ok(())
    }

    pub async fn run_session_start(
        &self,
        payload: &SessionStartHookPayload,
    ) -> Result<(), HookError> {
        if self.config.session_start.is_empty() {
            return Ok(());
        }

        for hook in &self.config.session_start {
            if !matcher_matches(hook.matcher.as_deref(), &payload.source) {
                continue;
            }

            let input = to_value(payload).map_err(HookError::SerializePayload)?;
            let timeout_override = hook.timeout_ms.map(Duration::from_millis);
            let _ = self
                .runner
                .run(hook.command.clone(), input, timeout_override)
                .await?;
        }

        Ok(())
    }

    pub async fn run_user_prompt_submit(
        &self,
        payload: &UserPromptSubmitHookPayload,
    ) -> Result<(), HookError> {
        if self.config.user_prompt_submit.is_empty() {
            return Ok(());
        }

        for hook in &self.config.user_prompt_submit {
            if !matcher_matches(hook.matcher.as_deref(), &payload.prompt) {
                continue;
            }

            let input = to_value(payload).map_err(HookError::SerializePayload)?;
            let timeout_override = hook.timeout_ms.map(Duration::from_millis);
            let _ = self
                .runner
                .run(hook.command.clone(), input, timeout_override)
                .await?;
        }

        Ok(())
    }
}

fn matcher_matches(matcher: Option<&str>, tool_name: &str) -> bool {
    match matcher {
        None => true,
        Some(pattern) => Regex::new(pattern)
            .map(|re| re.is_match(tool_name))
            .unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "windows"))]
    use std::fs;
    #[cfg(not(target_os = "windows"))]
    use std::os::unix::fs::PermissionsExt;

    use anyhow::Result;
    use serde_json::json;
    use tempfile::TempDir;

    use crate::hooks::HookCommandConfig;

    use super::*;

    #[test]
    fn default_hook_timeout_is_10s() {
        assert_eq!(DEFAULT_HOOK_TIMEOUT, Duration::from_secs(10));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[cfg(not(target_os = "windows"))]
    async fn pre_tool_use_honors_timeout_override() -> Result<()> {
        let hook_dir = TempDir::new()?;
        let script = hook_dir.path().join("hook.sh");
        std::fs::write(
            &script,
            r#"#!/bin/bash
set -e
cat >/dev/null
sleep 1
echo '{"decision":"allow"}'
"#,
        )?;
        std::fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
        let script_str = script.to_string_lossy().to_string();

        let manager = HooksManager::new(HooksConfig {
            pre_tool_use: vec![HookCommandConfig {
                matcher: Some("Write".to_string()),
                command: vec![script_str],
                timeout_ms: Some(50),
            }],
            ..Default::default()
        });
        let payloads = vec![ToolHookPayload {
            session_id: "sess-1".to_string(),
            transcript_path: "/tmp/rollout.jsonl".to_string(),
            hook_event_name: "PreToolUse".to_string(),
            tool_name: "Write".to_string(),
            tool_input: json!({
                "file_path": "/tmp/file.txt",
                "content": "data",
            }),
        }];

        let result = manager.run_pre_tool_use(&payloads).await;
        assert!(matches!(result, Err(HookError::Timeout)));
        Ok(())
    }
}
