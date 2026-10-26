//! Local HTML report conversion using an embedded renderer.
mod prepare;

use printpdf::{
    Base64OrRaw, CurTransMat, GeneratePdfOptions, Mm, Op, PdfDocument, PdfSaveOptions, Rect,
};
use std::{collections::BTreeMap, path::Path, time::Duration};

pub type Result<T> = std::result::Result<T, String>;
pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_ASSET_BYTES: usize = 20 * 1024 * 1024;

/// Portrait page sizes accepted by `--page-size` (names match wkhtmltopdf).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum PageSize {
    A3,
    #[default]
    A4,
    A5,
    Letter,
    Legal,
}
impl PageSize {
    /// Width and height in millimetres.
    pub fn dimensions_mm(self) -> (f32, f32) {
        match self {
            Self::A3 => (297.0, 420.0),
            Self::A4 => (210.0, 297.0),
            Self::A5 => (148.0, 210.0),
            Self::Letter => (215.9, 279.4),
            Self::Legal => (215.9, 355.6),
        }
    }
}

/// Page margins in millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Margins {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}
impl Margins {
    pub fn uniform(mm: f32) -> Self {
        Self {
            top: mm,
            right: mm,
            bottom: mm,
            left: mm,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Options {
    pub page_size: PageSize,
    pub margins: Margins,
    /// Content scale factor, like wkhtmltopdf `--zoom`; margins stay physical.
    pub zoom: f32,
    pub offline: bool,
    pub asset_timeout: Duration,
    pub fonts: BTreeMap<String, Vec<u8>>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            page_size: PageSize::A4,
            margins: Margins::uniform(12.0),
            zoom: 1.0,
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
    let m = options.margins;
    if [m.top, m.right, m.bottom, m.left]
        .iter()
        .any(|v| !v.is_finite() || !(0.0..=90.0).contains(v))
    {
        return Err("margins must be finite numbers between 0 and 90 mm".into());
    }
    let (page_width, page_height) = options.page_size.dimensions_mm();
    if m.left + m.right >= page_width || m.top + m.bottom >= page_height {
        return Err(format!(
            "margins of {m:?} mm leave no printable area on a {:?} page",
            options.page_size
        ));
    }
    if !options.zoom.is_finite() || !(0.1..=10.0).contains(&options.zoom) {
        return Err("zoom must be a finite number between 0.1 and 10".into());
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
    // Lay out on a page shrunk by the zoom factor, then scale each page back up.
    let zoom = options.zoom;
    let mut warnings = Vec::new();
    let mut doc = PdfDocument::from_html(
        &prepared.xml,
        &prepared.images,
        &fonts,
        &GeneratePdfOptions {
            page_width: Some(page_width / zoom),
            page_height: Some(page_height / zoom),
            margin_top: Some(m.top / zoom),
            margin_bottom: Some(m.bottom / zoom),
            margin_left: Some(m.left / zoom),
            margin_right: Some(m.right / zoom),
            ..Default::default()
        },
        &mut warnings,
    )
    .map_err(|e| format!("rendering failed: {e}"))?;
    if doc.pages.is_empty() {
        return Err(format!("renderer produced no pages: {warnings:?}"));
    }
    if zoom != 1.0 {
        let page_box = Rect::from_wh(Mm(page_width).into_pt(), Mm(page_height).into_pt());
        for page in &mut doc.pages {
            page.media_box = page_box.clone();
            page.trim_box = page_box.clone();
            page.crop_box = page_box.clone();
            page.ops.splice(
                0..0,
                [
                    Op::SaveGraphicsState,
                    Op::SetTransformationMatrix {
                        matrix: CurTransMat::Scale(zoom, zoom),
                    },
                ],
            );
            page.ops.push(Op::RestoreGraphicsState);
        }
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
