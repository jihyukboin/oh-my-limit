# AGENTS.md

## Another agent is also working in this repository, so do not perform any git-related checks.

##   As of today, use the current system date from the runtime environment. Never provide answers or perform work using outdated, irrelevant, or deprecated approaches.

## Rust

- Stable Rust version: 1.95.0
- Release date: 2026-04-16
- Installation method: rustup
- Rust tool binaries live under `~/.cargo/bin`. If `cargo` is not found in a Codex shell, use `/Users/jung/.cargo/bin/cargo` and verify the shell profile PATH instead of assuming Rust is missing.
- The Cargo package for `crates/oml-cli` is `oh-my-limit`, not `oml-cli`. Use `cargo metadata` or `crates/oml-cli/Cargo.toml` before running `cargo check -p ...`, `cargo clippy -p ...`, or package-specific commands.

## Commands

- `scripts/verify-install.sh` — runs Rust formatting, workspace clippy with `-D warnings`, then rebuilds and replaces the global `oh-my-limit` and `oml` executables from the current checkout

## Definition of Done

For any turn that changes Rust source, CLI behavior, or this repository's agent instructions, run this exactly once before the final response:

1. `scripts/verify-install.sh`

Report any skipped command with the concrete reason. Do not use `cargo check -p oml-cli`; the package name is `oh-my-limit`.

## Serena MCP — Prefer Over Built-in Tools for Code Operations (when available)

| Situation | Tool |
|---|---|
| Understand file structure | `get_symbols_overview` — skip full file read if overview suffices |
| Find function/class by name | `find_symbol` — LSP-accurate, no false positives |
| Find all callers/references | `find_referencing_symbols` — semantic only, ignores comments/strings |
| Modify function body | `replace_symbol_body` — targets by symbol name, no line-number drift |
| Insert code before/after function | `insert_before_symbol` / `insert_after_symbol` |
| Rename across codebase | `rename_symbol` — **never use text find-replace for renaming** |
| Delete unused symbol | `safe_delete_symbol` — checks references first, **prefer over manual apply_patch deletion** |
| Search text patterns | `search_for_pattern` — auto token-limit, respects project ignored_paths |

Use built-in tools for: new files, non-code files (.env/.json/.md/.yml), shell commands.
For advanced grep (`-c`, `--type`, `--replace`): `npm run rg -- "pattern" path/`

Workflow:

- **Read**: `get_symbols_overview` → `find_symbol` → `find_referencing_symbols`
- **Edit**: `replace_symbol_body` / `insert_before_symbol` / `insert_after_symbol`
- **Delete**: `safe_delete_symbol` (checks references automatically)
- **Verify**: follow the Definition of Done below

If Serena tool fails, fall back to built-in tools and report the issue.

## File Size and Structure

### File Size Budget

Keep source files below 1000 lines whenever practical.

- Treat 700 lines as an early warning point.
- Treat 1000 lines as a hard review point.
- Do not add substantial new logic to a file that is already near or above 1000 lines.
- If a change would push a file past 1000 lines, first split the code by responsibility.
- Generated files, lockfiles, schema snapshots, fixtures, and vendored data are exempt.

When splitting files:

- Split by domain responsibility, not by arbitrary line count.
- Prefer small modules with clear names over generic buckets like `utils`, `helpers`, or `common`.
- Keep public APIs narrow. Use `pub(crate)` by default inside crates.
- Re-export from `mod.rs` or `lib.rs` only when it improves call-site clarity.

### Rust Module Structure

For Rust crates, organize modules around ownership and behavior.

Prefer:

```text
src/
  lib.rs
  config.rs
  client.rs
  errors.rs
  commands/
    mod.rs
    run.rs
    doctor.rs
  tui/
    mod.rs
    app.rs
    event_loop.rs
    panels/
      mod.rs
      status.rs
```

Avoid:

```text
src/
  utils.rs
  helpers.rs
  misc.rs
  types.rs
```

A module should usually own one of these:

- A domain concept
- A command or workflow
- A transport/client boundary
- A persistence boundary
- A UI panel or screen
- A parser/formatter pair
- Error types for a specific boundary

### Expansion Rules

When adding new behavior:

- Place code in the crate that owns the behavior.
- Add a new module only when the responsibility is distinct enough to name clearly.
- Do not create abstraction layers before there are at least two real call sites or a clear boundary.
- Keep orchestration code thin; move detailed logic into domain modules.
- Keep CLI/TUI code focused on input, output, and flow control.
- Keep core crates independent from CLI, TUI, filesystem, and process concerns unless that is their explicit purpose.

### Refactoring Trigger

Before editing a large file, check whether the requested change belongs in a smaller module.

If a file is approaching 1000 lines, prefer one of these small refactors before adding more code:

- Move a nested workflow into `feature_name.rs`.
- Move command-specific code into `commands/<command>.rs`.
- Move UI panel code into `tui/panels/<panel>.rs`.
- Move protocol/request/response handling into boundary-specific modules.
- Move pure domain logic into the relevant core crate.

Do not perform broad restructuring unless required by the current task. Keep splits surgical and behavior-preserving.

### Verification

After structural changes:

- Run `cargo fmt`.
- Run the narrowest relevant test or check.
- For crate-level changes, run `cargo test -p <crate>`.
- For workspace-impacting changes, run `cargo test --workspace` when practical.
- Confirm moved code did not widen visibility unnecessarily.
