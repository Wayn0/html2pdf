use clap::Parser;
use html2pdf::{MAX_INPUT_BYTES, Options, Result};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

#[derive(Parser)]
#[command(
    version,
    about = "Convert a local UTF-8 HTML report to an A4 PDF without a browser"
)]
struct Cli {
    /// Local HTML file, or - for stdin
    input: PathBuf,
    /// PDF file, or - for stdout. Existing files require --force
    output: PathBuf,
    /// Resolve assets against this directory (default: input file's directory; CWD for stdin)
    #[arg(long)]
    base_dir: Option<PathBuf>,
    /// Page margin on every side, in millimetres (0–90)
    #[arg(long, default_value_t = 12.0)]
    margin: f32,
    /// Block HTTP and HTTPS assets
    #[arg(long)]
    offline: bool,
    /// Timeout per remote asset, in seconds
    #[arg(long, default_value_t = 15, value_parser = clap::value_parser!(u64).range(1..=300))]
    asset_timeout: u64,
    /// Embed a TTF/OTF font under a CSS family name; repeat as needed
    #[arg(long, value_name = "FAMILY=PATH")]
    font: Vec<String>,
    /// Replace an existing output file atomically
    #[arg(long)]
    force: bool,
    /// Suppress success messages (warnings and errors remain visible)
    #[arg(short, long)]
    quiet: bool,
}

fn read_limited(reader: impl Read, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("reading input: {e}"))?;
    if bytes.len() > limit {
        return Err(format!("input exceeds {} MiB limit", limit / 1024 / 1024));
    }
    Ok(bytes)
}

fn run(cli: Cli) -> Result<()> {
    let stdout = cli.output == Path::new("-");
    if !stdout && cli.output.exists() && !cli.force {
        return Err(format!(
            "output {} already exists; use --force to replace it",
            cli.output.display()
        ));
    }
    if !stdout
        && cli.input != Path::new("-")
        && cli.input.canonicalize().ok() == cli.output.canonicalize().ok()
        && cli.input.exists()
        && cli.output.exists()
    {
        return Err("input and output must be different files".into());
    }
    let bytes = if cli.input == Path::new("-") {
        read_limited(io::stdin().lock(), MAX_INPUT_BYTES)?
    } else {
        read_limited(
            File::open(&cli.input)
                .map_err(|e| format!("opening input {}: {e}", cli.input.display()))?,
            MAX_INPUT_BYTES,
        )?
    };
    let html = std::str::from_utf8(&bytes)
        .map_err(|e| format!("input must be UTF-8: {e}"))?
        .trim_start_matches('\u{feff}');
    let base = cli.base_dir.unwrap_or_else(|| {
        cli.input
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_path_buf()
    });
    if !base.is_dir() {
        return Err(format!(
            "base directory {} does not exist or is not a directory",
            base.display()
        ));
    }
    let mut fonts = BTreeMap::new();
    for spec in cli.font {
        let (name, path) = spec
            .split_once('=')
            .filter(|(n, p)| !n.is_empty() && !p.is_empty())
            .ok_or("--font expects FAMILY=PATH")?;
        let bytes = read_limited(
            File::open(path).map_err(|e| format!("opening font {path:?}: {e}"))?,
            html2pdf::MAX_ASSET_BYTES,
        )?;
        if printpdf::ParsedFont::from_bytes(&bytes, 0, &mut Vec::new()).is_none() {
            return Err(format!("invalid font {path:?}; expected a TTF/OTF font"));
        }
        fonts.insert(name.to_string(), bytes);
    }
    let result = html2pdf::convert(
        html,
        &base,
        &Options {
            margin_mm: cli.margin,
            offline: cli.offline,
            asset_timeout: Duration::from_secs(cli.asset_timeout),
            fonts,
        },
    )?;
    for warning in result.warnings {
        eprintln!("warning: {warning}");
    }
    if stdout {
        io::stdout()
            .lock()
            .write_all(&result.bytes)
            .map_err(|e| format!("writing PDF to stdout: {e}"))?;
    } else {
        let parent = cli
            .output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(parent)
            .map_err(|e| format!("creating output in {}: {e}", parent.display()))?;
        temp.write_all(&result.bytes)
            .and_then(|_| temp.as_file().sync_all())
            .map_err(|e| format!("writing PDF: {e}"))?;
        if cli.force {
            temp.persist(&cli.output)
        } else {
            temp.persist_noclobber(&cli.output)
        }
        .map_err(|e| format!("saving {}: {}", cli.output.display(), e.error))?;
    }
    if !cli.quiet {
        eprintln!(
            "Rendered {} A4 page(s), {} bytes",
            result.pages,
            result.bytes.len()
        );
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("html2pdf: {error}");
            ExitCode::FAILURE
        }
    }
}
