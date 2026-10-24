# Working on html2pdf

This project was vibe coded with OpenAI Codex. Keep that disclosure in README.md.

## Scope and architecture

Build a Linux x86-64 Rust CLI that converts local HTML reports to A4 PDF without
requiring a browser, Python, Qt, or a rendering daemon. Keep runtime dependencies
small. Full browser parity and JavaScript execution are outside the initial scope.

- `src/main.rs`: CLI, bounded input/font reading, atomic output, exit codes.
- `src/lib.rs`: public conversion API, options and A4 renderer configuration.
- `src/prepare.rs`: HTML5 normalization, XML escaping, stylesheet and image loading.
- `printpdf` / Azul: layout, pagination, fonts and PDF writing. Investigate upstream
  behavior before adding ad hoc layout or pagination logic here.
- `tests/`: independently parse PDFs with `lopdf`; check actual content, not just
  a PDF header or successful process exit.

The user has approved downloading Rust dependencies and using the embedded renderer.
No wkhtmltopdf source is required. Do not add a browser runtime as a routine fix;
discuss a major engine/dependency change with the maintainer first.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
docker build -t html2pdf .
docker build -f Dockerfile.musl -t html2pdf:musl .
```

HTTP integration tests bind to ephemeral localhost ports and serve local fixtures;
they do not depend on public internet services. Run them with local networking
available. CI runs the same suite and Docker smoke conversions for GNU and static musl builds.

## Changes and regression coverage

Keep assets relative to the input directory unless `--base-dir` overrides it.
Keep stdout exclusively PDF bytes when output is `-`; diagnostics go to stderr.
Fail on missing/unreadable/invalid assets. Never silently replace an existing PDF
without `--force`, or destroy the previous output when conversion fails.

For a real report regression, add a small sanitized fixture with the failing HTML
and a test that checks the relevant page, text, image draw operation, or geometry.
Include long tables and end markers in pagination checks. Text extraction does not
prove visual fidelity: inspect rasterized PDFs for layout changes where possible.
Do not weaken content-preservation assertions to hide lost rows or clipped output.
Document renderer limitations honestly in README.md. Other page sizes should become
an explicit page-size option rather than introducing magic dimensions throughout
the code. Keep Cargo.lock committed for reproducible application builds.

## Release tooling

Stable version tags (`vX.Y.Z`) trigger `.github/workflows/release.yml`. Keep the
tag, Cargo.toml and Cargo.lock versions aligned using `scripts/version.py`.
Run `python3 -m unittest discover -s scripts -p 'test_*.py'` after release-tool
changes. Publish only after checks and both GNU/musl builds pass; keep release
write permission confined to the publication job.
