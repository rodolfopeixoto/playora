# Contributing

Playora is in early MVP. Patches welcome.

## Local dev

```sh
cargo build --release
cargo test          # unit + integration
cargo clippy
cargo fmt --check
```

## Cross-compile for R36S

```sh
sh scripts/build-container.sh    # Apple `container` CLI (preferred)
# fallback to docker:
sh scripts/build-arm64.sh
```

## Conventions

- Edition 2021, MSRV 1.81.
- snake_case crates, kebab-case binaries, PascalCase types.
- No `unwrap` in non-test code.
- No `unsafe` outside documented FFI (only `statvfs` today).
- Errors via `anyhow::Result` in binaries, `thiserror` in libs.
- No comments stating WHAT — only WHY (when non-obvious).
- Tests next to code (`#[cfg(test)] mod tests`).

## PR checklist

- [ ] `cargo test` passes
- [ ] `cargo clippy` no new warnings
- [ ] `cargo fmt`
- [ ] docs updated if surface changes
