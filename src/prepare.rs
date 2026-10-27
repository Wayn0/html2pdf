use crate::{MAX_ASSET_BYTES, Options, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use printpdf::{Base64OrRaw, RawImage};
use scraper::{ElementRef, Html, Selector};
use std::{collections::BTreeMap, fs::File, io::Read, path::Path};

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
    let Sanitized {
        css,
        stripped,
        sized_selectors,
    } = sanitize_css(&css);
    if stripped {
        out.warnings.push(
            "CSS ::before/::after/::first-line/::first-letter rules are unsupported and were ignored"
                .into(),
        );
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
        out.warnings.push(
            "CSS @page is unsupported; --page-size and margin options control page geometry".into(),
        );
    }
    let mut images = Images {
        sizes: BTreeMap::new(),
        // A selector the matcher cannot parse might match an image, so it counts as one.
        sized_by_css: sized_selectors
            .iter()
            .map(|s| Selector::parse(s).ok())
            .collect(),
    };
    for element in parsed.select(&selector("img")) {
        // Like a browser, an <img> with no source draws nothing rather than failing.
        let Some(src) = image_src(element) else {
            if !out.warnings.iter().any(|w| w.starts_with("skipped <img>")) {
                out.warnings
                    .push("skipped <img> with missing or empty src".into());
            }
            continue;
        };
        if images.sizes.contains_key(src) {
            continue;
        }
        let bytes = load(src, base, options, &agent)?;
        let image = RawImage::decode_from_bytes(&bytes, &mut Vec::new())
            .map_err(|e| format!("cannot decode image {src:?} (PNG/JPEG supported): {e}"))?;
        images
            .sizes
            .insert(src.to_string(), (image.width, image.height));
        out.images.insert(src.to_string(), Base64OrRaw::Raw(bytes));
    }
    out.xml.push_str("<html><head><style>");
    escape(&css, &mut out.xml);
    out.xml.push_str("</style></head>");
    let body = parsed
        .select(&selector("body"))
        .next()
        .ok_or("HTML has no body")?;
    serialize(body, &images, &mut out.xml, 0)?;
    out.xml.push_str("</html>");
    Ok(out)
}

struct Images {
    /// Natural pixel size of each decoded image, by `src`.
    sizes: BTreeMap<String, (usize, usize)>,
    /// Author rules that set a non-`auto` width or height (`None`: unparseable selector).
    sized_by_css: Vec<Option<Selector>>,
}

fn serialize(
    element: ElementRef<'_>,
    images: &Images,
    out: &mut String,
    depth: usize,
) -> Result<()> {
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
    let intrinsic = if name == "img" {
        let Some(src) = image_src(element) else {
            return Ok(());
        };
        intrinsic_size(element, images.sizes.get(src).copied())
    } else {
        None
    };
    // A block-level image (Bootstrap's `img-responsive`/`center-block`) is laid out
    // without its intrinsic size and vanishes, so also state that size as inline CSS
    // unless an author rule or inline style sizes the image.
    let size_style = intrinsic
        .filter(|_| !sized_by_author(element, images))
        .map(|(width, height)| format!("width: {width}px; height: {height}px;"));
    out.push('<');
    out.push_str(name);
    if let Some((width, height)) = intrinsic {
        out.push_str(&format!(" width=\"{width}\" height=\"{height}\""));
    }
    if let Some(size_style) = &size_style {
        out.push_str(" style=\"");
        out.push_str(size_style);
        if let Some(style) = element.value().attr("style") {
            out.push(' ');
            escape(style, out);
        }
        out.push('"');
    }
    for (key, value) in element.value().attrs() {
        if key.starts_with("on")
            || key.contains(':')
            || (intrinsic.is_some() && matches!(key, "width" | "height"))
            || (size_style.is_some() && key == "style")
        {
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
            serialize(e, images, out, depth + 1)?;
        } else if let Some(t) = child.value().as_text() {
            escape(t, out);
        }
    }
    out.push_str("</");
    out.push_str(name);
    out.push('>');
    Ok(())
}

struct Sanitized {
    css: String,
    /// Whether pseudo-element selectors were dropped.
    stripped: bool,
    /// Selectors of rules that set a non-`auto` width or height.
    sized_selectors: Vec<String>,
}

/// Rewrite author CSS into what the renderer handles like a browser would:
/// - The renderer has no pseudo-elements and would apply `.row:after { display: table }`
///   to `.row` itself, so drop those selectors before they hide real content.
/// - A malformed declaration (e.g. a stray `class="x"` in a rule) makes the renderer
///   lose the rest of the stylesheet, where a browser drops just that declaration.
fn sanitize_css(css: &str) -> Sanitized {
    let css = strip_comments(css);
    let mut out = String::with_capacity(css.len());
    let mut stripped = false;
    let mut sized_selectors = Vec::new();
    let mut rest = css.as_str();
    while let Some(open) = find_unquoted(rest, &['{', '}']) {
        let (prelude, tail) = rest.split_at(open);
        if let Some(after) = tail.strip_prefix('}') {
            out.push_str(prelude);
            out.push('}');
            rest = after;
            continue;
        }
        // Statements such as `@charset ...;` end before the block's selector.
        let start = prelude.rfind(';').map_or(0, |i| i + 1);
        out.push_str(&prelude[..start]);
        let selectors = &prelude[start..];
        let at_rule = selectors.trim_start().starts_with('@');
        if at_rule && is_group_rule(selectors) {
            out.push_str(selectors);
            out.push('{');
            rest = &tail[1..];
            continue;
        }
        let end = block_end(tail);
        let raw_block = &tail[..end];
        rest = &tail[end..];
        if at_rule {
            out.push_str(selectors);
            out.push_str(raw_block);
            continue;
        }
        let block = &clean_declarations(raw_block);
        let parts = split_selectors(selectors);
        let kept: Vec<&str> = parts
            .iter()
            .map(|s| s.trim())
            .filter(|s| !is_pseudo_element(s))
            .collect();
        stripped |= kept.len() != parts.len();
        if declares_size(block) {
            sized_selectors.extend(kept.iter().map(|s| s.to_string()));
        }
        if !kept.is_empty() {
            out.push('\n');
            out.push_str(&kept.join(", "));
            out.push(' ');
            out.push_str(block);
        }
        // A clearfix (`.row:after { clear: both }`) makes its host contain floats.
        let hosts: Vec<&str> = parts
            .iter()
            .filter_map(|s| clearfix_host(s.trim(), block))
            .collect();
        if !hosts.is_empty() {
            out.push('\n');
            out.push_str(&hosts.join(", "));
            out.push_str(" { display: flow-root; }");
        }
    }
    out.push_str(rest);
    Sanitized {
        css: out,
        stripped,
        sized_selectors,
    }
}

/// Keep only well-formed `name: value` declarations of a `{ ... }` block.
fn clean_declarations(block: &str) -> String {
    let Some(inner) = block
        .strip_prefix('{')
        .and_then(|b| b.strip_suffix('}'))
        .filter(|b| find_unquoted(b, &['{', '}']).is_none())
    else {
        return block.to_string();
    };
    let kept: Vec<&str> = split_unquoted(inner, ';')
        .into_iter()
        .map(str::trim)
        .filter(|d| declaration(d).is_some())
        .collect();
    if kept.is_empty() {
        return "{ }".into();
    }
    format!("{{ {}; }}", kept.join("; "))
}

/// The property name and value of a well-formed declaration.
fn declaration(decl: &str) -> Option<(&str, &str)> {
    let (name, value) = decl.split_once(':')?;
    let name = name.trim();
    let ident = name.strip_prefix('-').unwrap_or(name);
    let valid = ident
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '-')
        && ident
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    (valid && !value.trim().is_empty()).then_some((name, value.trim()))
}

/// Whether declarations set a `width` or `height` other than `auto`.
fn declares_size(declarations: &str) -> bool {
    let inner = declarations.trim();
    let inner = inner
        .strip_prefix('{')
        .and_then(|b| b.strip_suffix('}'))
        .unwrap_or(inner);
    split_unquoted(inner, ';')
        .into_iter()
        .filter_map(|d| declaration(d.trim()))
        .any(|(name, value)| {
            let value = value.trim_end_matches("!important").trim();
            matches!(name.to_ascii_lowercase().as_str(), "width" | "height")
                && !value.eq_ignore_ascii_case("auto")
        })
}

fn sized_by_author(element: ElementRef<'_>, images: &Images) -> bool {
    element.value().attr("style").is_some_and(declares_size)
        || images
            .sized_by_css
            .iter()
            .any(|s| s.as_ref().is_none_or(|s| s.matches(&element)))
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut chars = css.chars().peekable();
    let mut quote = None;
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                out.push(c);
                if c == '\\' {
                    out.extend(chars.next());
                } else if c == q {
                    quote = None;
                }
            }
            None if c == '/' && chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for c in chars.by_ref() {
                    if prev == '*' && c == '/' {
                        break;
                    }
                    prev = c;
                }
                out.push(' ');
            }
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                }
                out.push(c);
            }
        }
    }
    out
}

/// Byte offset of the first of `targets` outside quotes, parentheses and brackets.
fn find_unquoted(s: &str, targets: &[char]) -> Option<usize> {
    let mut quote = None;
    let mut depth = 0usize;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
        } else if c == '(' || c == '[' {
            depth += 1;
        } else if (c == ')' || c == ']') && depth > 0 {
            depth -= 1;
        } else if depth == 0 && targets.contains(&c) {
            return Some(i);
        }
    }
    None
}

/// Length of the `{ ... }` block at the start of `s`, including any nested blocks.
fn block_end(s: &str) -> usize {
    let mut depth = 0usize;
    let mut offset = 0;
    while let Some(i) = find_unquoted(&s[offset..], &['{', '}']) {
        offset += i;
        if s[offset..].starts_with('{') {
            depth += 1;
        } else {
            depth -= 1;
        }
        offset += 1;
        if depth == 0 {
            return offset;
        }
    }
    s.len()
}

fn is_group_rule(prelude: &str) -> bool {
    let name = prelude.trim_start()[1..]
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "media" | "supports" | "document" | "layer" | "container"
    )
}

fn split_selectors(list: &str) -> Vec<&str> {
    split_unquoted(list, ',')
}

fn split_unquoted(list: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut rest = list;
    while let Some(i) = find_unquoted(rest, &[separator]) {
        parts.push(&rest[..i]);
        rest = &rest[i + 1..];
    }
    parts.push(rest);
    parts
}

fn clearfix_host<'a>(selector: &'a str, block: &str) -> Option<&'a str> {
    let block = block.to_ascii_lowercase().replace(char::is_whitespace, "");
    if !["clear:both", "clear:left", "clear:right"]
        .iter()
        .any(|c| block.contains(c))
    {
        return None;
    }
    let lower = selector.to_ascii_lowercase();
    let host = if lower.ends_with("::after") {
        &selector[..selector.len() - 7]
    } else if lower.ends_with(":after") {
        &selector[..selector.len() - 6]
    } else {
        return None;
    };
    (!host.is_empty() && !is_pseudo_element(host) && !host.ends_with(char::is_whitespace))
        .then_some(host)
}

fn is_pseudo_element(selector: &str) -> bool {
    let s = selector.to_ascii_lowercase();
    s.contains("::")
        || [":before", ":after", ":first-line", ":first-letter"]
            .iter()
            .any(|p| s.contains(p))
}

/// The renderer sizes an image only from its `width`/`height` attributes and treats a
/// missing one as zero, so fill them in like a browser: the natural pixel size, with a
/// single stated dimension scaled by the image's aspect ratio. Non-pixel attribute
/// values (such as percentages) are left to the renderer.
fn intrinsic_size(
    element: ElementRef<'_>,
    natural: Option<(usize, usize)>,
) -> Option<(usize, usize)> {
    let (natural_w, natural_h) = natural.filter(|&(w, h)| w > 0 && h > 0)?;
    let attr = |key| -> Option<Option<f64>> {
        let Some(value) = element
            .value()
            .attr(key)
            .map(str::trim)
            .filter(|v| !v.is_empty())
        else {
            return Some(None);
        };
        let px = value.strip_suffix("px").unwrap_or(value).trim();
        px.parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && *v >= 0.0)
            .map(Some)
    };
    let ratio = natural_h as f64 / natural_w as f64;
    let (width, height) = match (attr("width")?, attr("height")?) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, w * ratio),
        (None, Some(h)) => (h / ratio, h),
        (None, None) => (natural_w as f64, natural_h as f64),
    };
    Some((width.round() as usize, height.round() as usize))
}

fn image_src<'a>(element: ElementRef<'a>) -> Option<&'a str> {
    element.value().attr("src").filter(|s| !s.trim().is_empty())
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
            .get(&encode_url(src))
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

// Percent-encode characters browsers tolerate in src attributes but HTTP URIs reject
// (e.g. spaces in uploaded file names). Existing %XX escapes are kept; the fragment
// is never sent to the server.
fn encode_url(src: &str) -> String {
    let src = src.split_once('#').map_or(src, |(url, _)| url);
    let mut out = String::with_capacity(src.len());
    for &b in src.as_bytes() {
        if b <= b' ' || b >= 0x7f || b"\"<>\\^`{|}".contains(&b) {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push(b as char);
        }
    }
    out
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
    fn pseudo_element_rules_are_stripped() {
        let Sanitized { css, stripped, .. } = sanitize_css(
            "/* a{} */ *, *:before, *::after { box-sizing: border-box; }\n\
             .row:before, .row:after { display: table; content: \" { \"; }\n\
             @media print { a[href]:after { content: \" (\" attr(href) \")\"; } p { color: red; } }\n\
             a[href^=\"javascript:\"], :not(.a, .b) { color: blue; }\n\
             @font-face { font-family: x; } .row { margin: 0; }",
        );
        assert!(stripped);
        assert!(!css.contains("before") && !css.contains("after") && !css.contains("table"));
        assert!(css.contains("* { box-sizing: border-box; }"));
        assert!(css.contains("@media print {\np { color: red; } }"));
        assert!(css.contains("a[href^=\"javascript:\"], :not(.a, .b) { color: blue; }"));
        assert!(css.contains("@font-face { font-family: x; }"));
        assert!(css.contains(".row { margin: 0; }"));
        assert!(!css.contains("flow-root"));
        let css = sanitize_css(
            ".clearfix:after, .row::after { clear: both; } .x:after { content: \"\"; }",
        )
        .css;
        assert!(css.contains(".clearfix, .row { display: flow-root; }"));
        assert!(!css.contains(".x"));
        let Sanitized { css, stripped, .. } = sanitize_css("p:hover { x: y }");
        assert_eq!((css.trim(), stripped), ("p:hover { x: y; }", false));
    }
    #[test]
    fn malformed_declarations_are_dropped() {
        // A stray attribute pasted into a rule must not take the rest of the stylesheet with it.
        let Sanitized { css, .. } = sanitize_css(
            "@media (min-width: 1px) { .a {class=\"x\"\n width: 1%; } .b { ; } }\n\
             p { color: red;; background: url(a;b) } .c { -webkit-x: 1; --y: 2; 3d: no; }",
        );
        assert!(css.contains(".a { }") && css.contains(".b { }"));
        assert!(css.contains("p { color: red; background: url(a;b); }"));
        assert!(css.contains(".c { -webkit-x: 1; --y: 2; }"));
        assert!(!css.contains("class") && !css.contains(";;") && !css.contains("{ ;"));
    }
    #[test]
    fn image_sizes_follow_attributes_and_author_css() {
        let sized = sanitize_css(
            "img { max-width: 100%; height: auto !important; } .logo, p { width: 9px; }",
        )
        .sized_selectors;
        assert_eq!(sized, [".logo", "p"]);
        let html = Html::parse_fragment(
            "<img src=a width=180px><img src=a height=50><img src=a><img src=a width=50%>\
             <img src=a class=logo><img src=a style='height: 9px'><img src=b>",
        );
        let images = Images {
            sizes: BTreeMap::from([("a".to_string(), (360, 100))]),
            sized_by_css: sized.iter().map(|s| Selector::parse(s).ok()).collect(),
        };
        let found: Vec<_> = html
            .select(&Selector::parse("img").unwrap())
            .map(|e| {
                let size = intrinsic_size(e, images.sizes.get(image_src(e).unwrap()).copied());
                (size, sized_by_author(e, &images))
            })
            .collect();
        assert_eq!(
            found,
            [
                (Some((180, 50)), false),
                (Some((180, 50)), false),
                (Some((360, 100)), false),
                (None, false),
                (Some((360, 100)), true),
                (Some((360, 100)), true),
                (None, false),
            ]
        );
        let mut xml = String::new();
        serialize(
            html.select(&Selector::parse("img").unwrap())
                .next()
                .unwrap(),
            &images,
            &mut xml,
            0,
        )
        .unwrap();
        assert_eq!(
            xml,
            "<img width=\"180\" height=\"50\" style=\"width: 180px; height: 50px;\" src=\"a\" />"
        );
    }
    #[test]
    fn url_encoding() {
        assert_eq!(
            encode_url("https://h/a/logo _70 .png"),
            "https://h/a/logo%20_70%20.png"
        );
        assert_eq!(
            encode_url("http://h/%20é?q=a b#x"),
            "http://h/%20%C3%A9?q=a%20b"
        );
        assert_eq!(
            encode_url("http://h/a.png?x=1&y=2"),
            "http://h/a.png?x=1&y=2"
        );
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
