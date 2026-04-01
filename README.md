# md2docx

`md2docx` is a native Rust command-line utility that converts Markdown files into formatted Word `.docx` documents.

This repository is structured for a small native release build and a simple public distribution workflow.

Why Rust for this tool:

- Fast cold start with no VM or interpreter.
- Single-file release binary.
- Good release-size tuning with LTO, symbol stripping, and abort-on-panic.

## Build

```powershell
cargo build --release
```

The executable will be written to `target/release/md2docx.exe`.

## Usage

```powershell
.\target\release\md2docx.exe .\README.md --output .\README.docx --title "md2docx" --author "Paul"
```

You can also let the tool pick the output name automatically:

```powershell
.\target\release\md2docx.exe .\notes.md --overwrite
```

That writes `notes.docx` next to `notes.md`.

Directory input is supported and runs recursively through subfolders:

```powershell
.\target\release\md2docx.exe .\technical_specs --overwrite
```

That writes each `.docx` next to its source Markdown file anywhere under `technical_specs`.

If you want to keep the output in a separate tree, pass an output directory:

```powershell
.\target\release\md2docx.exe .\technical_specs --output .\technical_specs_docx --overwrite
```

That mirrors the folder structure under `technical_specs_docx`.

## Repository Files

- `CHANGELOG.md`: release history and notable user-facing changes.
- `CONTRIBUTING.md`: development and contribution workflow.
- `LICENSE`: MIT-0 license for unrestricted public use without attribution requirements.
- `RELEASE.md`: release checklist and packaging notes.

## Formatting Coverage

Current support is intentionally focused on the common Markdown structures that convert cleanly to Word:

- Headings
- Paragraphs
- Bold, italic, strikethrough, inline code
- Ordered and unordered lists
- Block quotes
- Fenced code blocks
- Links
- Images as clickable placeholders
- Basic table flattening into readable text

## Notes

- Local and remote images are currently preserved as clickable placeholders instead of embedded media to keep the dependency set lean.
- The CLI parser is implemented without a framework dependency to protect binary size and startup time.
- `target/` and generated `.docx` files are ignored by git.
- This project is licensed under MIT-0, which allows public use, modification, redistribution, and commercial use without attribution requirements.
