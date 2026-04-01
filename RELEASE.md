# Release Guide

## Pre-release checklist

1. Update the version in `Cargo.toml`.
2. Update `CHANGELOG.md`.
3. Run `cargo fmt`.
4. Run `cargo test`.
5. Run `cargo build --release`.
6. Validate one single-file conversion and one recursive directory conversion.
7. Review `README.md` for accuracy.
8. Confirm repository metadata before publishing publicly.

## Build the release binary

```powershell
cargo build --release
```

The binary will be written to `target/release/md2docx.exe`.

## Suggested release artifacts

- `target/release/md2docx.exe`
- `README.md`
- `CHANGELOG.md`

## Suggested tag format

- `v0.1.0`

## Notes

- This repository uses the MIT-0 license for unrestricted public software distribution without attribution requirements.
- If you plan to publish on GitHub, add repository URLs and issue-tracker links once the remote is created.
