# Translation Pipeline

Korean input is masked, translated to compact English, validated, unmasked,
and then sent to Codex when the usage gate decides bridging is worthwhile.

The TUI keeps provider selection separate from provider execution.

```text
TUI /translator commands
  -> config.toml
  -> TranslationService
      -> noop
      -> ollama
      -> local-openai-compatible
      -> openai
  -> Codex turn/start input
```

Provider settings are changed from the TUI:

```text
/translator
/translator opens an interactive picker:
  Local
    Ollama
    OpenAI-compatible local server
    Off
  Remote
    OpenAI API

/ starts the Codex-style command hint popup. Use Up/Down to move, Tab to
complete, Enter to run the selected command, and Esc to dismiss.

Direct commands remain available:
/translator provider <noop|ollama|local-openai-compatible|openai>
/translator model <model>
/translator base-url <url|default>
/translator api-key-env <ENV>
/translator remote <on|off>
/translator test
```

Remote OpenAI translation is opt-in. The config stores the environment variable
name, not the raw API key. When the key is entered through the picker, it is
validated with one OpenAI API call and then saved to `~/.oh-my-limit/.env` as
`OPENAI_API_KEY`. The OpenAI provider refuses to run until remote translation is
explicitly enabled:

```toml
[translation]
provider = "openai"
model = "gpt-5.4-mini"
base_url = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"
fail_closed = true

[privacy]
remote_translation_allowed = true
```

```text
# ~/.oh-my-limit/.env
OPENAI_API_KEY=sk-...
```
