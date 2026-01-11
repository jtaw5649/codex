# Hooks

Codex can run synchronous hooks on session lifecycle events, user prompts, and tool use.
Hooks receive JSON on stdin and should print JSON to stdout (even `{}`) to acknowledge.

## Configuration

Add hook configuration to `~/.codex/config.toml`:

```toml
[hooks]
  [[hooks.session_start]]
  command = ["/path/to/hook.sh"]
  matcher = "startup|resume|clear" # optional (regex against `source`)

  [[hooks.user_prompt_submit]]
  command = ["/path/to/hook.sh"]
  matcher = ".*" # optional (regex against prompt text)

  [[hooks.pre_tool_use]]
  command = ["/path/to/hook.sh"]
  matcher = "Write|Edit|MultiEdit|TodoWrite" # regex against `tool_name`
  timeout_ms = 30000 # optional per-hook timeout in milliseconds

  [[hooks.post_tool_use]]
  command = ["/path/to/hook.sh"]
  matcher = "Write|Edit|MultiEdit|TodoWrite" # regex against `tool_name`
```

## Payloads

All hook payloads include:

- `session_id`
- `transcript_path`
- `hook_event_name`

### SessionStart

```json
{
  "hook_event_name": "SessionStart",
  "source": "startup" | "resume" | "clear"
}
```

### UserPromptSubmit

```json
{
  "hook_event_name": "UserPromptSubmit",
  "prompt": "...",
  "cwd": "/abs/path"
}
```

### PreToolUse / PostToolUse

```json
{
  "hook_event_name": "PreToolUse" | "PostToolUse",
  "tool_name": "Write",
  "tool_input": { "file_path": "...", "content": "..." }
}
```

## Responses

- `PreToolUse` can block tool execution by returning:
  - `{ "decision": "block", "reason": "..." }`
  - `{ "decision": "allow" }`
- Other hooks ignore the response, but still expect valid JSON (use `{}` if you have nothing to return).

## Security

Hooks run locally with the same privileges as Codex. Only run scripts you trust,
validate and escape inputs carefully, and avoid shell injection when forwarding data.
