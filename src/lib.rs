//! Local HTML report conversion using an embedded renderer.
mod prepare;

use printpdf::{Base64OrRaw, GeneratePdfOptions, PdfDocument, PdfSaveOptions};
use std::{collections::BTreeMap, path::Path, time::Duration};

pub type Result<T> = std::result::Result<T, String>;
pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_ASSET_BYTES: usize = 20 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Options {
    pub margin_mm: f32,
    pub offline: bool,
    pub asset_timeout: Duration,
    pub fonts: BTreeMap<String, Vec<u8>>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            margin_mm: 12.0,
            offline: false,
            asset_timeout: Duration::from_secs(15),
            fonts: BTreeMap::new(),
        }
    }
}

pub struct Conversion {
    pub bytes: Vec<u8>,
    pub pages: usize,
    pub warnings: Vec<String>,
}

/// Convert UTF-8 HTML. Relative assets resolve against `base_dir`, never the CWD.
/// JavaScript is not executed. Asset failures are errors rather than missing logos.
pub fn convert(html: &str, base_dir: &Path, options: &Options) -> Result<Conversion> {
    if html.trim().is_empty() {
        return Err("input HTML is empty".into());
    }
    if html.len() > MAX_INPUT_BYTES {
        return Err("input HTML exceeds the 32 MiB limit".into());
    }
    if !options.margin_mm.is_finite() || !(0.0..=90.0).contains(&options.margin_mm) {
        return Err("margin must be a finite number between 0 and 90 mm".into());
    }
    if options.asset_timeout.is_zero() {
        return Err("asset timeout must be greater than zero".into());
    }
    let prepared = prepare::prepare(html, base_dir, options)?;
    let fonts = options
        .fonts
        .iter()
        .map(|(name, bytes)| (name.clone(), Base64OrRaw::Raw(bytes.clone())))
        .collect();
    let mut warnings = Vec::new();
    let mut doc = PdfDocument::from_html(
        &prepared.xml,
        &prepared.images,
        &fonts,
        &GeneratePdfOptions {
            page_width: Some(210.0),
            page_height: Some(297.0),
            margin_top: Some(options.margin_mm),
            margin_bottom: Some(options.margin_mm),
            margin_left: Some(options.margin_mm),
            margin_right: Some(options.margin_mm),
            ..Default::default()
        },
        &mut warnings,
    )
    .map_err(|e| format!("rendering failed: {e}"))?;
    if doc.pages.is_empty() {
        return Err(format!("renderer produced no pages: {warnings:?}"));
    }
    if let Some(title) = prepared.title {
        doc.metadata.info.document_title = title;
    }
    let pages = doc.pages.len();
    let bytes = doc.save(&PdfSaveOptions::default(), &mut warnings);
    let mut messages = prepared.warnings;
    messages.extend(warnings.iter().map(|w| format!("{w:?}")));
    Ok(Conversion {
        bytes,
        pages,
        warnings: messages,
    })
}
