# Sample configuration

For a sample configuration file, see [this documentation](https://developers.openai.com/codex/config-sample).

For hook configuration details, see `docs/hooks.md`.

```toml
[hooks]
  [[hooks.pre_tool_use]]
  command = ["/path/to/hook.sh"]
  matcher = "Write"
  timeout_ms = 30000
```
