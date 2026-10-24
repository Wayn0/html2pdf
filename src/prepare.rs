use crate::{MAX_ASSET_BYTES, Options, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use printpdf::{Base64OrRaw, RawImage};
use scraper::{ElementRef, Html, Selector};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::Path,
};

pub(crate) struct Prepared {
    pub xml: String,
    pub title: Option<String>,
    pub images: BTreeMap<String, Base64OrRaw>,
    pub warnings: Vec<String>,
}

// A small report-oriented baseline; author styles appear later in the cascade.
const DEFAULT_CSS: &str = r#"
body { font-family: Helvetica; font-size: 12px; color: #222222; }
pre, code, kbd, samp { font-family: Courier; }
pre { white-space: pre-wrap; }
img { max-width: 100%; }
table { border-collapse: collapse; }
"#;

pub(crate) fn prepare(html: &str, base: &Path, options: &Options) -> Result<Prepared> {
    let parsed = Html::parse_document(html);
    let selector = |s: &str| Selector::parse(s).expect("constant selector");
    let title = parsed
        .select(&selector("title"))
        .next()
        .map(|e| e.text().collect::<String>());
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(options.asset_timeout))
        .max_redirects(5)
        .build()
        .into();
    let mut out = Prepared {
        xml: String::new(),
        images: BTreeMap::new(),
        title,
        warnings: vec![],
    };
    if parsed.select(&selector("script")).next().is_some() {
        out.warnings
            .push("JavaScript is not executed; script elements were omitted".into());
    }
    if parsed
        .select(&selector(
            "svg, math, iframe, canvas, input, select, textarea, button",
        ))
        .next()
        .is_some()
    {
        out.warnings.push("SVG, MathML, embedded documents, canvas, and interactive form rendering are unsupported".into());
    }
    if parsed.select(&selector("base")).next().is_some() {
        out.warnings
            .push("<base> is ignored; use --base-dir and absolute remote URLs".into());
    }
    let mut css = DEFAULT_CSS.to_string();
    for element in parsed.select(&selector("style, link")) {
        let media = element.value().attr("media").unwrap_or("all");
        if !matches!(media.trim(), "" | "all" | "print") {
            continue;
        }
        if element.value().name() == "style" {
            css.push('\n');
            css.push_str(&element.text().collect::<String>());
        } else if element
            .value()
            .attr("rel")
            .unwrap_or("")
            .split_whitespace()
            .any(|s| s.eq_ignore_ascii_case("stylesheet"))
        {
            let href = element
                .value()
                .attr("href")
                .ok_or("stylesheet link has no href")?;
            let bytes = load(href, base, options, &agent)?;
            css.push('\n');
            css.push_str(
                std::str::from_utf8(&bytes)
                    .map_err(|_| format!("stylesheet {href:?} is not UTF-8"))?,
            );
        }
    }
    if css.contains("@import") {
        return Err("CSS @import is not supported; use <link rel=\"stylesheet\"> instead".into());
    }
    if css.contains("url(") {
        out.warnings
            .push("CSS url() assets and @font-face are unsupported; use <img> and --font".into());
    }
    if css.contains("@media") {
        out.warnings.push("CSS @media blocks have limited renderer support; use a print stylesheet with media=\"print\"".into());
    }
    if css.contains("@page") {
        out.warnings
            .push("CSS @page is unsupported; A4 and --margin control page geometry".into());
    }
    let mut seen = BTreeSet::new();
    for element in parsed.select(&selector("img")) {
        let src = element
            .value()
            .attr("src")
            .filter(|s| !s.is_empty())
            .ok_or("image has no src")?;
        if !seen.insert(src.to_string()) {
            continue;
        }
        let bytes = load(src, base, options, &agent)?;
        RawImage::decode_from_bytes(&bytes, &mut Vec::new())
            .map_err(|e| format!("cannot decode image {src:?} (PNG/JPEG supported): {e}"))?;
        out.images.insert(src.to_string(), Base64OrRaw::Raw(bytes));
    }
    out.xml.push_str("<html><head><style>");
    escape(&css, &mut out.xml);
    out.xml.push_str("</style></head>");
    let body = parsed
        .select(&selector("body"))
        .next()
        .ok_or("HTML has no body")?;
    serialize(body, &mut out.xml, 0)?;
    out.xml.push_str("</html>");
    Ok(out)
}

fn serialize(element: ElementRef<'_>, out: &mut String, depth: usize) -> Result<()> {
    if depth > 256 {
        return Err("HTML nesting exceeds the 256-level limit".into());
    }
    let name = element.value().name();
    if matches!(
        name,
        "script"
            | "style"
            | "link"
            | "meta"
            | "base"
            | "template"
            | "iframe"
            | "object"
            | "embed"
            | "canvas"
            | "audio"
            | "video"
            | "svg"
            | "math"
    ) {
        return Ok(());
    }
    out.push('<');
    out.push_str(name);
    for (key, value) in element.value().attrs() {
        if key.starts_with("on") || key.contains(':') {
            continue;
        }
        out.push(' ');
        out.push_str(key);
        out.push_str("=\"");
        escape(value, out);
        out.push('"');
    }
    if element.children().next().is_none() {
        out.push_str(" />");
        return Ok(());
    }
    out.push('>');
    for child in element.children() {
        if let Some(e) = ElementRef::wrap(child) {
            serialize(e, out, depth + 1)?;
        } else if let Some(t) = child.value().as_text() {
            escape(t, out);
        }
    }
    out.push_str("</");
    out.push_str(name);
    out.push('>');
    Ok(())
}

fn escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c == '\t' || c == '\n' || c == '\r' || c >= ' ' => out.push(c),
            _ => {}
        }
    }
}

fn load(src: &str, base: &Path, options: &Options, agent: &ureq::Agent) -> Result<Vec<u8>> {
    let src = src.trim();
    let bytes = if src.starts_with("data:") {
        let (header, data) = src.split_once(',').ok_or("invalid data URI")?;
        if !header.ends_with(";base64") {
            return Err("only base64 data URIs are supported".into());
        }
        if data.len() > MAX_ASSET_BYTES * 4 / 3 + 4 {
            return Err("data URI exceeds 20 MiB limit".into());
        }
        STANDARD
            .decode(data)
            .map_err(|e| format!("invalid base64 data URI: {e}"))?
    } else if src.starts_with("https://") || src.starts_with("http://") {
        if options.offline {
            return Err(format!("remote asset blocked by --offline: {src}"));
        }
        agent
            .get(src)
            .call()
            .map_err(|e| format!("fetching {src:?}: {e}"))?
            .body_mut()
            .with_config()
            .limit(MAX_ASSET_BYTES as u64)
            .read_to_vec()
            .map_err(|e| format!("reading {src:?}: {e}"))?
    } else {
        if src.contains("://") || src.starts_with("//") || src.starts_with("file:") {
            return Err(format!(
                "unsupported asset URL {src:?}; use a file path or http(s) URL"
            ));
        }
        let path = base.join(src);
        let file =
            File::open(&path).map_err(|e| format!("opening asset {}: {e}", path.display()))?;
        let mut bytes = Vec::new();
        file.take(MAX_ASSET_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("reading asset {}: {e}", path.display()))?;
        bytes
    };
    if bytes.len() > MAX_ASSET_BYTES {
        return Err(format!("asset {src:?} exceeds 20 MiB limit"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalization() {
        let p = prepare(
            "<!doctype html><TITLE>A &amp; B</TITLE><p>one&nbsp;&copy;<br>two<p>three",
            Path::new("."),
            &Options::default(),
        )
        .unwrap();
        assert!(p.xml.contains("<br />"));
        assert!(p.xml.contains("three</p>"));
        assert!(p.xml.contains('©'));
        assert_eq!(p.title.as_deref(), Some("A & B"));
    }
    #[test]
    fn escapes_attributes_and_text() {
        let p = prepare(
            "<p title='a &quot; b'>a &amp; b &lt; c</p>",
            Path::new("."),
            &Options::default(),
        )
        .unwrap();
        assert!(p.xml.contains("a &amp; b &lt; c"));
        assert!(p.xml.contains("title=\"a &quot; b\""));
    }
    #[test]
    fn strips_scripts() {
        let p = prepare(
            "<script>throw Error('bad')</script><p onclick='bad()'>Hello</p>",
            Path::new("."),
            &Options::default(),
        )
        .unwrap();
        assert!(!p.xml.contains("script"));
        assert!(!p.xml.contains("onclick"));
        assert_eq!(p.warnings.len(), 1);
    }
    #[test]
    fn screen_styles_omitted() {
        let p = prepare(
            "<style media='screen'>SCREEN</style><style media='print'>PRINT</style><p>Hi</p>",
            Path::new("."),
            &Options::default(),
        )
        .unwrap();
        assert!(!p.xml.contains("SCREEN"));
        assert!(p.xml.contains("PRINT"));
    }
}
