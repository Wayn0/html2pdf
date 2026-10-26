use clap::Parser;
use html2pdf::{MAX_INPUT_BYTES, Margins, Options, PageSize, Result};
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
    about = "Convert a local UTF-8 HTML report to a PDF without a browser"
)]
struct Cli {
    /// Local HTML file, or - for stdin
    input: PathBuf,
    /// PDF file, or - for stdout. Existing files are replaced unless --safe is given
    output: PathBuf,
    /// Resolve assets against this directory (default: input file's directory; CWD for stdin)
    #[arg(long)]
    base_dir: Option<PathBuf>,
    /// Portrait page size (case-insensitive)
    #[arg(long, value_enum, ignore_case = true, default_value_t = PageSize::A4)]
    page_size: PageSize,
    /// Page margin on every side (0–90 mm); accepts mm, cm, in or pt, default unit mm
    #[arg(long, value_name = "LENGTH", default_value = "12mm", value_parser = parse_length_mm)]
    margin: f32,
    /// Top margin, overriding --margin
    #[arg(short = 'T', long, value_name = "LENGTH", value_parser = parse_length_mm)]
    margin_top: Option<f32>,
    /// Right margin, overriding --margin
    #[arg(short = 'R', long, value_name = "LENGTH", value_parser = parse_length_mm)]
    margin_right: Option<f32>,
    /// Bottom margin, overriding --margin
    #[arg(short = 'B', long, value_name = "LENGTH", value_parser = parse_length_mm)]
    margin_bottom: Option<f32>,
    /// Left margin, overriding --margin
    #[arg(short = 'L', long, value_name = "LENGTH", value_parser = parse_length_mm)]
    margin_left: Option<f32>,
    /// Scale page content, like wkhtmltopdf --zoom (0.1–10; margins are not scaled)
    #[arg(long, default_value_t = 1.0)]
    zoom: f32,
    /// Block HTTP and HTTPS assets
    #[arg(long)]
    offline: bool,
    /// Timeout per remote asset, in seconds
    #[arg(long, default_value_t = 15, value_parser = clap::value_parser!(u64).range(1..=300))]
    asset_timeout: u64,
    /// Embed a TTF/OTF font under a CSS family name; repeat as needed
    #[arg(long, value_name = "FAMILY=PATH")]
    font: Vec<String>,
    /// Refuse to replace an existing output file
    #[arg(long)]
    safe: bool,
    /// Accepted for compatibility; replacing existing output is the default
    #[arg(long, hide = true, conflicts_with = "safe")]
    force: bool,
    /// Suppress success messages (warnings and errors remain visible)
    #[arg(short, long)]
    quiet: bool,
}

/// Parse a wkhtmltopdf-style length such as `10`, `10mm`, `1.5cm`, `0.5in` or `36pt` into mm.
fn parse_length_mm(value: &str) -> Result<f32> {
    let value = value.trim();
    let split = value
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let scale = match unit.to_ascii_lowercase().as_str() {
        "" | "mm" => 1.0,
        "cm" => 10.0,
        "in" => 25.4,
        "pt" => 25.4 / 72.0,
        _ => return Err(format!("unsupported unit {unit:?}; use mm, cm, in or pt")),
    };
    let number: f32 = number
        .trim()
        .parse()
        .map_err(|_| format!("invalid length {value:?}"))?;
    if !number.is_finite() {
        return Err(format!("invalid length {value:?}"));
    }
    Ok(number * scale)
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
    if !stdout && cli.output.exists() && cli.safe {
        return Err(format!(
            "output {} already exists; omit --safe to replace it",
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
            page_size: cli.page_size,
            margins: Margins {
                top: cli.margin_top.unwrap_or(cli.margin),
                right: cli.margin_right.unwrap_or(cli.margin),
                bottom: cli.margin_bottom.unwrap_or(cli.margin),
                left: cli.margin_left.unwrap_or(cli.margin),
            },
            zoom: cli.zoom,
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
        if cli.safe {
            temp.persist_noclobber(&cli.output)
        } else {
            temp.persist(&cli.output)
        }
        .map_err(|e| format!("saving {}: {}", cli.output.display(), e.error))?;
    }
    if !cli.quiet {
        eprintln!(
            "Rendered {} {:?} page(s), {} bytes",
            result.pages,
            cli.page_size,
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
