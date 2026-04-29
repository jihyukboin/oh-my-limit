# Oh My Limit

Oh My Limit is an app-server-first Rust TUI for Korean developers using Codex
through a ChatGPT Plus or Pro subscription.

The project is intentionally structured as a Cargo workspace. The CLI remains
thin, while Codex app-server access, masking, translation, storage, and config
ownership live in separate crates.

## Status

This repository is in the initial workspace layout stage.

## Workspace

- `crates/oml-cli`: user-facing `oh-my-limit` and `oml` command surface
- `crates/oml-core`: language detection, masking, usage gate, and reports
- `crates/oml-codex-appserver`: `codex app-server` JSONL client
- `crates/oml-codex-exec`: emergency and benchmark fallback runner
- `crates/oml-translation`: local and opt-in remote translation providers
- `crates/oml-storage`: SQLite-backed sessions, cache, and reports
- `crates/oml-config`: config loading, defaults, and paths

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.

