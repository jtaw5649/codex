use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    #[serde(default)]
    pub pre_tool_use: Vec<HookCommandConfig>,
    #[serde(default)]
    pub post_tool_use: Vec<HookCommandConfig>,
    #[serde(default)]
    pub session_start: Vec<HookCommandConfig>,
    #[serde(default)]
    pub user_prompt_submit: Vec<HookCommandConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookCommandConfig {
    pub matcher: Option<String>,
    pub command: Vec<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}
