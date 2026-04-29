# AGENTS.md

## Operating Rules

- Another agent may be working in this repository. Do not run git commands or git-related checks, including `git status`, `git diff`, `git log`, `git branch`, `git checkout`, `git reset`, `git stash`, `git commit`, or equivalent tooling.
- Use the current runtime date for all date-sensitive work. Do not rely on stale model knowledge for versions, APIs, tooling behavior, or release status.
- Keep changes scoped to the user request. Avoid broad restructuring unless it is required to complete the task safely.
- Prefer the smallest correct change. Do not introduce abstractions until there are at least two real call sites or a clear boundary.

## Rust Environment

- Stable Rust version: `1.95.0`
- Release date: `2026-04-16`
- Installation method: `rustup`
- Rust tool binaries are expected under `~/.cargo/bin`.
- If `cargo` is not found in a Codex shell, try `/Users/jung/.cargo/bin/cargo` and inspect the shell profile `PATH` before assuming Rust is missing.
- The Cargo package for `crates/oml-cli` is `oh-my-limit`, not `oml-cli`.
- Before package-specific Cargo commands, confirm package names with `cargo metadata` or the relevant `Cargo.toml`.

## Required Commands

- `scripts/verify-install.sh`
  - Runs Rust formatting.
  - Runs workspace Clippy with `-D warnings`.
  - Rebuilds and replaces the global `oh-my-limit` and `oml` executables from the current checkout.

## Definition of Done

For any turn that changes Rust source, CLI behavior, or repository agent instructions, run this command exactly once before the final response:

```sh
scripts/verify-install.sh
```

- Report the result in the final response.
- If the command is skipped, report the concrete reason.
- Do not run `cargo check -p oml-cli`; the package name is `oh-my-limit`.
- Do not run duplicate formatting or Clippy commands when `scripts/verify-install.sh` already covers them, unless a narrower diagnostic is needed before the final verification.
- Run additional targeted tests only when the change affects behavior not covered by the required verification command.

## Code Navigation and Edits

Prefer Serena MCP over built-in code tools when available and reliable.

| Task | Preferred Serena tool |
|---|---|
| Understand file structure | `get_symbols_overview` |
| Find a function, type, class, or method | `find_symbol` |
| Find callers or references | `find_referencing_symbols` |
| Replace a function or method body | `replace_symbol_body` |
| Insert code near a symbol | `insert_before_symbol` / `insert_after_symbol` |
| Rename a symbol across code | `rename_symbol` |
| Delete an unused symbol | `safe_delete_symbol` |
| Search text patterns | `search_for_pattern` |

Use built-in tools for:

- New files
- Non-code files such as `.env`, `.json`, `.md`, `.yml`, and `.toml`
- Shell commands
- Cases where Serena is unavailable, stale, or fails

If Serena fails, fall back to built-in tools and report the failure briefly.

Preferred workflow:

1. Read: `get_symbols_overview` → `find_symbol` → `find_referencing_symbols`
2. Edit: symbol-aware Serena edit tools when possible
3. Delete: `safe_delete_symbol` before manual deletion
4. Verify: follow the Definition of Done

For advanced grep features such as counts, type filters, or replacement previews, use:

```sh
npm run rg -- "pattern" path/
```

## File Size and Structure

Keep source files below 1000 lines whenever practical.

- Treat 700 lines as an early warning point.
- Treat 1000 lines as a hard review point.
- Do not add substantial new logic to a file near or above 1000 lines.
- If a change would push a file past 1000 lines, first split code by responsibility.
- Generated files, lockfiles, schema snapshots, fixtures, and vendored data are exempt.

When splitting files:

- Split by domain responsibility, not arbitrary line count.
- Prefer clear module names over generic names such as `utils`, `helpers`, or `common`.
- Use `pub(crate)` by default inside crates.
- Re-export from `mod.rs` or `lib.rs` only when it improves call-site clarity.
- Keep splits surgical and behavior-preserving.

## Rust Module Guidelines

Organize Rust modules around ownership and behavior.

Prefer structures like:

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

Avoid generic buckets like:

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
- A transport or client boundary
- A persistence boundary
- A UI panel or screen
- A parser/formatter pair
- Boundary-specific error types

## Expansion Rules

When adding behavior:

- Put code in the crate that owns the behavior.
- Add a new module only when the responsibility is distinct and nameable.
- Keep orchestration code thin.
- Move detailed logic into domain modules.
- Keep CLI and TUI code focused on input, output, and flow control.
- Keep core crates independent from CLI, TUI, filesystem, and process concerns unless that is their explicit purpose.

Before editing a large file, check whether the requested change belongs in a smaller module.

If a file is approaching 1000 lines, prefer one of these narrow refactors before adding more logic:

- Move a nested workflow into `feature_name.rs`.
- Move command-specific code into `commands/<command>.rs`.
- Move UI panel code into `tui/panels/<panel>.rs`.
- Move protocol, request, or response handling into boundary-specific modules.
- Move pure domain logic into the relevant core crate.

## Final Response Requirements

When finishing a task:

- Summarize what changed.
- Report verification commands run and their results.
- Report skipped required commands with concrete reasons.
- Mention any Serena fallback only if it affected the workflow.
