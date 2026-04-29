# Contributing

This project uses a Rust Cargo workspace.

## Local checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Scope

Keep changes surgical. Put shared behavior in the owning library crate rather
than in `oml-cli`.

