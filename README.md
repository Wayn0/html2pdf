# html2pdf

A small Rust command-line tool that converts local HTML reports into PDFs (A4 by default),
ready to attach to email. It embeds a renderer; **no Chrome, Qt, Python, or
wkhtmltopdf installation is required**. Initial target: Linux x86-64.

**This project was vibe coded with OpenAI Codex**, with requirements and review
from its human maintainer. It is an early version intended for iterative testing
against real reports. Passing tests establish specific behavior, not full HTML/CSS
conformance or browser-quality rendering. Contributor instructions are in
[AGENTS.md](AGENTS.md).

## Download binaries

Download Linux x86-64 binaries from the
[GitHub Releases page](https://github.com/Wayn0/html2pdf/releases/latest).
Releases will appear after the workflows are pushed and a version tag is pushed.
Each release contains:

- `html2pdf-vVERSION-x86_64-unknown-linux-gnu.tar.gz`: GNU/glibc build from Debian bookworm.
- `html2pdf-vVERSION-x86_64-unknown-linux-musl.tar.gz`: static build, including for Alpine.
- `SHA256SUMS`: archive checksums.

Extract the archive, then run `./html2pdf report.html report.pdf`. Use the musl
archive when you want a binary with no shared-library requirements. To verify a
download, keep the archives and `SHA256SUMS` in the same directory and run
`sha256sum --check --ignore-missing SHA256SUMS`.

## Build and use

Build with stable Rust (minimum declared version: 1.88) and a C compiler for the
TLS dependency. Use the committed lockfile:

```sh
cargo build --release --locked
./target/release/html2pdf report.html report.pdf
./target/release/html2pdf examples/report.html /tmp/example.pdf
./target/release/html2pdf --help
```

The executable is `target/release/html2pdf`. You can copy it into a compatible
Linux container. A binary built on a newer glibc distribution may not run on an
older one; use the supplied Docker build for a consistent Debian runtime.

```sh
# Pipe HTML in; resolve images and stylesheets from a chosen directory.
cat report.html | html2pdf - report.pdf --base-dir ./assets

# Pipe PDF bytes out; messages go to stderr.
html2pdf report.html - > report.pdf

# Prevent network requests, adjust margins, replace an existing PDF.
html2pdf report.html report.pdf --offline --margin 15 --force

# Use US Letter instead of the default A4.
html2pdf report.html report.pdf --page-size Letter

# Supply a custom TTF/OTF font and use font-family: "Report Sans" in CSS.
html2pdf report.html report.pdf --font 'Report Sans=/fonts/ReportSans.ttf'
```

For stdout redirection, your shell opens/truncates the destination before the
converter runs. Use the file-output form when you want atomic output and protection
for an existing PDF.

| Option | Behavior |
| --- | --- |
| `INPUT OUTPUT` | Local UTF-8 HTML and PDF paths; `-` means stdin/stdout |
| `--base-dir DIR` | Asset directory; defaults to the input file's parent, or CWD for stdin |
| `--page-size SIZE` | Portrait `A3`, `A4` (default), `A5`, `Letter` or `Legal`; case-insensitive |
| `--margin MM` | All four page margins; default 12 mm, allowed range 0–90, must leave a printable area |
| `--offline` | Reject HTTP(S) assets, while allowing local and data assets |
| `--asset-timeout SECONDS` | Timeout per remote asset, default 15, range 1–300 |
| `--font FAMILY=PATH` | Embed a TTF/OTF font; repeat for additional font files |
| `--force` | Replace an existing output file after successful conversion |
| `--quiet` | Suppress the success message; warnings/errors remain visible |

A successful conversion exits 0. Conversion/I/O errors exit 1; invalid CLI usage
exits 2. Missing or invalid assets fail the conversion rather than silently leaving
holes in the report. Output files are written through a temporary file in the
same directory and atomically installed. Failed conversion leaves an existing
output intact, including with `--force`. Input and output cannot be the same file.

## Docker

```sh
docker build -t html2pdf .
docker run --rm --user "$(id -u):$(id -g)" \
  -v "$PWD:/work" html2pdf report.html report.pdf

# With only local assets, disable networking as well.
docker run --rm --network none --user "$(id -u):$(id -g)" \
  -v "$PWD:/work" html2pdf examples/report.html report.pdf --offline
```

The multi-stage build keeps Rust, Cargo and the C compiler out of the runtime
image. Runtime uses Debian bookworm-slim and the executable. No font package or
system OpenSSL is required: the renderer includes default font subsets and HTTPS
uses Rustls with bundled public certificate roots. Private certificate authorities
are not configurable in this version.

For an existing Debian-compatible container, copy the executable from the build
stage into your own image. The GNU build links to glibc, libm, libgcc_s and the
Linux loader. For Alpine or a fully static executable, use the musl build below.

### Static musl build

`Dockerfile.musl` builds for `x86_64-unknown-linux-musl` using Alpine and places
only the static executable in a `scratch` runtime image:

```sh
docker build -f Dockerfile.musl -t html2pdf:musl .
docker run --rm --network none --user "$(id -u):$(id -g)" \
  -v "$PWD:/work" html2pdf:musl examples/report.html report.pdf --offline
```

To extract the executable for use on Linux x86-64, including Alpine:

```sh
docker create --name html2pdf-musl-export html2pdf:musl
docker cp html2pdf-musl-export:/html2pdf ./html2pdf-musl
docker rm html2pdf-musl-export
./html2pdf-musl report.html report.pdf
```

Alternatively, with a musl toolchain installed on the build host:

```sh
rustup target add x86_64-unknown-linux-musl
# Debian/Ubuntu: install musl-tools to provide musl-gcc.
CC_x86_64_unknown_linux_musl=musl-gcc \
CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc \
  cargo build --release --locked --target x86_64-unknown-linux-musl
```

The result is `target/x86_64-unknown-linux-musl/release/html2pdf`. The target's
standard library and musl C compiler are build-time requirements. No shared
libraries, browser, or installed font package is required at runtime. The scratch
image has no shell; HTTPS certificate roots are bundled through Rustls.

## HTML, assets and pagination

The CLI uses an HTML5 parser to recover normal HTML, including omitted closing
tags, named entities and void elements, then normalizes it for the embedded
`printpdf`/Azul renderer. No JavaScript executes.

- Portrait pages, A4 (210 × 297 mm) by default. `--page-size` selects A3, A5, US
  Letter or US Legal. Landscape orientation and custom dimensions are not supported
  yet; HTML metadata does not override the CLI's page geometry.
- Headings, paragraphs, semantic sections, lists, inline text, links, preformatted
  text, tables, cell spans, borders, padding and basic colors/styles.
- Inline CSS, `<style>` blocks and UTF-8 `<link rel="stylesheet" href="…">` files.
  Stylesheets with `media="print"`, `media="all"` or no media are loaded;
  screen-only stylesheets are skipped. Prefer a separate print stylesheet over
  complex `@media` rules.
- Local PNG/JPEG images, absolute HTTP(S) image URLs, and base64 data images.
  Duplicate image sources are loaded once. Use explicit image dimensions in CSS,
  for example `<img src="logo.png" style="width:120px;height:40px">`.
- Relative asset paths resolve against the HTML file's directory, independent of
  the process working directory. `--base-dir` overrides this. Local asset paths
  are filesystem paths, not URL-encoded paths; use absolute HTTP(S) URLs for remote
  assets. `<base>` is ignored with a warning.
- Automatic page flow and `page-break-before: always` are tested. Long tables are
  tested with 1,000 rows and unique markers in every row and data cell. Wrapped
  table rows and long paragraphs have separate content-preservation tests.
- Default Helvetica/Times/Courier-style subsets are available without installed
  fonts. Coverage is limited; supply appropriate fonts for other scripts. Font
  choice, shaping and missing-glyph behavior still need real-report validation.

HTML is much more than a list of tags: layout, typography, CSS and pagination are
where most compatibility work lives. This version deliberately uses an existing
engine rather than implementing a new browser.

### Known rendering limits

- **Repeated `<thead>` headers did not appear on continuation pages** in the
  initial large-table probe, despite the renderer enabling repetition. Do not rely
  on repeated headers yet.
- A nested list in the element fixture duplicates its preceding text. Basic text
  checks do not catch every visual/duplication defect; inspect the PDF.
- Text preservation tests for tags do not establish correct styling for every
  tag. Italics, strike-through, superscripts/subscripts and other details may
  differ from a browser. Complex layouts, flex/grid, positioned content and very
  tall individual elements are not compatibility guarantees.
- CSS `@page`, CSS `url()` assets, `@font-face`, and JavaScript-generated content
  are unsupported. `@import` is rejected with an actionable error. `@media`
  blocks have limited support. Use `--font`, `<img>` and linked print CSS instead.
- Inline SVG/MathML and embedded documents/media are omitted with a warning.
  Interactive forms and canvas are unsupported. Rasterize charts into PNG/JPEG
  before conversion. Only PNG and JPEG decoders are enabled initially.
- No browser-level fidelity promise, clickable-link guarantee, PDF/A, tagged PDF,
  accessibility conformance or full wkhtmltopdf option compatibility. Wrappers that
  call a `wkhtmltopdf` executable can use this binary only with the options listed
  above (for example `--quiet` and `--page-size`); other wkhtmltopdf flags are
  rejected as usage errors.

Use trusted report templates. The converter can read local assets and fetch
remote assets referenced by a report; `--base-dir` is a resolution directory, not
a filesystem sandbox. For isolation, use a container with narrowly scoped mounts
and `--network none`/`--offline` as appropriate. Input is limited to 32 MiB, each
asset to 20 MiB, and HTML nesting to 256 levels; these are not a total memory or
CPU budget. Remote assets have individual timeouts and at most five redirects.

## Dependencies and design

| Direct dependency | Purpose |
| --- | --- |
| `printpdf` with HTML, PNG and JPEG features | Azul layout/pagination and PDF generation |
| `scraper` | HTML5 parsing and normalization |
| `ureq` with Rustls | Blocking HTTP(S) asset fetching without system OpenSSL |
| `clap` | CLI parsing and generated help |
| `base64` | Embedded data image decoding |
| `tempfile` | Safe temporary files and atomic output installation |
| `lopdf` (test only) | Parse generated PDFs independently of the rendering API |

These crates bring transitive Rust dependencies; “no browser” does not mean zero
dependencies. Rendering uses upstream code, not copied wkhtmltopdf source. See
[printpdf](https://docs.rs/printpdf/0.12.8/printpdf/) and
[Azul](https://azul.rs/). The application is MIT licensed; dependency/font licenses
remain their respective upstream licenses.

## Tests and iteration

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Tests cover a broad HTML fixture, PDF parsing and page dimensions for each size, content order and
counts in large tables, page breaks, text wrapping, metadata, local/data/HTTP
images (including actual image draw operations), stylesheet color operators,
HTTP errors/timeouts, offline mode, input errors, clean stdout and safe file
replacement. HTTP tests serve fixtures from ephemeral localhost ports and need
local networking; they do not contact public servers. GitHub Actions runs checks on pull requests. Version tags run the checks,
release builds and Docker smoke conversions.

For a manual check, generate the [sample report](examples/report.html) and the
[element fixture](tests/fixtures/elements.html). Optional Poppler tools can help:

```sh
html2pdf tests/fixtures/elements.html /tmp/elements.pdf
pdftotext -layout /tmp/elements.pdf -
pdftoppm -scale-to 1400 -png /tmp/elements.pdf /tmp/elements
```

When a real report fails, reduce it to a sanitized fixture and add a test for the
specific lost content, pagination issue or drawing behavior. This is the intended
path to improving rendering without adding a heavyweight browser runtime.

## Publishing a release (GitHub Actions)

GitHub Actions workflows live in `.github/workflows/`, similar to GitLab CI jobs.
Pull requests run formatting, lint and test checks (which compile test binaries).
Branch pushes, including pushes to `main`, do not trigger a workflow. Release
binaries and Docker images are built only when pushing a stable version tag such as
`v0.1.1` runs the release workflow: validate the version, run tests, build GNU and
musl binaries, smoke-test both container runtimes, package them, and publish a
GitHub Release with the two archives and checksums. Tests and both builds must
succeed before publication. Only the publication job gets `contents: write`.
GitHub supplies `GITHUB_TOKEN` automatically; no personal access token is needed.

For the first release, commit/push the project and workflows, then tag its current
version (`0.1.0`):

```sh
git tag -a v0.1.0 -m "Release 0.1.0"
git push origin v0.1.0
```

For subsequent releases, use the version helper (Python 3.11+; release tooling
only, not a dependency of the converter):

```sh
python3 scripts/version.py bump 0.1.1
git add Cargo.toml Cargo.lock
git commit -m "Release 0.1.1"
git push
git tag -a v0.1.1 -m "Release 0.1.1"
git push origin v0.1.1
```

The helper updates both Cargo files and refuses invalid versions or downgrades.
A tag must exactly match the Cargo version; mismatches stop the release. Changing
`Cargo.toml` alone does not trigger a release; the tag is the explicit release
trigger. Stable `X.Y.Z` versions are supported initially, without prerelease tags.
Do not move published tags; release a new version instead.

Watch progress under the repository's **Actions** tab. Finished downloads appear
under **Releases** at `https://github.com/Wayn0/html2pdf/releases`. Repository or
organization policies must allow Actions and the workflow's release write
permission. No GitHub Pages setup is required. Local configuration is not a
published release until committed/pushed and triggered on GitHub.
