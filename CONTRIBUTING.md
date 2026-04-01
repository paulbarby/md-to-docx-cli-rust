# Contributing

## Development setup

1. Install a current stable Rust toolchain.
2. Clone the repository.
3. Build the project with `cargo build`.
4. Run the test suite with `cargo test`.

## Local verification

Before opening a pull request, run:

```powershell
cargo fmt
cargo test
cargo build --release
```

If you change conversion behavior, also run one or more real sample conversions and verify the generated `.docx` output.

## Scope guidelines

- Keep the binary fast to start and small to distribute.
- Prefer minimal dependencies.
- Preserve Markdown behavior unless there is a deliberate compatibility decision.
- Keep generated artifacts out of commits unless they are intentionally tracked examples.

## Pull requests

- Keep changes focused.
- Update `README.md` when user-facing behavior changes.
- Update `CHANGELOG.md` for notable release-facing changes.
