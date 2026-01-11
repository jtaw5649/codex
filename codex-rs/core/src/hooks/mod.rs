use serde::Deserialize;
use serde_json::Value;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;

mod config;
mod manager;
mod payload;

pub use config::HookCommandConfig;
pub use config::HooksConfig;
pub use manager::HooksManager;
pub use payload::HookContext;
pub use payload::SessionStartHookPayload;
pub use payload::ToolHookPayload;
pub use payload::UserPromptSubmitHookPayload;
pub use payload::build_apply_patch_hook_payloads;
pub use payload::build_session_start_hook_payload;
pub use payload::build_user_prompt_submit_hook_payload;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HookDecision {
    #[serde(default)]
    pub decision: Option<HookDecisionKind>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HookDecisionWithContext {
    pub decision: HookDecision,
    pub hook: HookCommandConfig,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HookDecisionKind {
    Block,
    Allow,
}

#[derive(Debug, thiserror::Error)]
pub enum HookError {
    #[error("hook command is empty")]
    EmptyCommand,
    #[error("failed to spawn hook command: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("failed to serialize hook payload: {0}")]
    SerializePayload(serde_json::Error),
    #[error("failed to parse hook response: {0}")]
    InvalidResponse(serde_json::Error),
    #[error("hook command timed out")]
    Timeout,
    #[error("hook command failed: {0}")]
    CommandFailed(String),
    #[error("hook response was not valid UTF-8: {0}")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),
}

pub struct HookRunner {
    timeout: Duration,
}

impl HookRunner {
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }

    pub async fn run(
        &self,
        command: Vec<String>,
        payload: Value,
        timeout_override: Option<Duration>,
    ) -> Result<HookDecision, HookError> {
        let (program, args) = command.split_first().ok_or(HookError::EmptyCommand)?;
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            let input = serde_json::to_vec(&payload).map_err(HookError::SerializePayload)?;
            stdin.write_all(&input).await?;
        }

        let timeout_duration = timeout_override.unwrap_or(self.timeout);
        let output = timeout(timeout_duration, child.wait_with_output())
            .await
            .map_err(|_| HookError::Timeout)??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(HookError::CommandFailed(stderr));
        }

        let stdout = String::from_utf8(output.stdout)?;
        let trimmed = stdout.trim();
        let decision =
            serde_json::from_str::<HookDecision>(trimmed).map_err(HookError::InvalidResponse)?;
        Ok(decision)
    }
}
