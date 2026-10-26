use std::{
    io::Write,
    process::{Command, Stdio},
};
fn cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_html2pdf"))
}
fn input(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let path = dir.path().join("report.html");
    std::fs::write(&path, "<h1>Report</h1><p>CLI content</p>").unwrap();
    path
}
#[test]
fn help_and_version() {
    for flag in ["--help", "--version"] {
        let o = cmd().arg(flag).output().unwrap();
        assert!(o.status.success());
        assert!(String::from_utf8_lossy(&o.stdout).contains("html2pdf"));
    }
}
#[test]
fn file_to_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = input(&dir);
    let pdf = dir.path().join("out.pdf");
    let o = cmd().arg(path).arg(&pdf).output().unwrap();
    assert!(o.status.success(), "{:?}", o);
    assert!(o.stdout.is_empty());
    lopdf::Document::load(&pdf).unwrap();
}
#[test]
fn stdin_to_stdout_is_clean_pdf() {
    let mut child = cmd()
        .args(["-", "-", "--quiet"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"<p>From stdin</p>")
        .unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success(), "{:?}", o);
    assert!(o.stderr.is_empty(), "{:?}", o);
    assert!(o.stdout.starts_with(b"%PDF-"));
    lopdf::Document::load_mem(&o.stdout).unwrap();
}
#[test]
fn existing_output_preserved_with_safe() {
    let dir = tempfile::tempdir().unwrap();
    let path = input(&dir);
    let pdf = dir.path().join("out.pdf");
    std::fs::write(&pdf, b"original").unwrap();
    let o = cmd().arg(path).arg(&pdf).arg("--safe").output().unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("--safe"));
    assert_eq!(std::fs::read(pdf).unwrap(), b"original");
}
#[test]
fn existing_output_replaced_by_default() {
    for extra in [None, Some("--force")] {
        let dir = tempfile::tempdir().unwrap();
        let path = input(&dir);
        let pdf = dir.path().join("out.pdf");
        std::fs::write(&pdf, b"original").unwrap();
        let o = cmd().arg(path).arg(&pdf).args(extra).output().unwrap();
        assert!(o.status.success(), "{extra:?}: {o:?}");
        lopdf::Document::load(pdf).unwrap();
    }
}
#[test]
fn input_cannot_overwrite_itself() {
    let dir = tempfile::tempdir().unwrap();
    let path = input(&dir);
    let before = std::fs::read(&path).unwrap();
    assert!(
        !cmd()
            .arg(&path)
            .arg(&path)
            .arg("--force")
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(std::fs::read(path).unwrap(), before);
}
#[test]
fn failures_leave_no_partial_output() {
    let dir = tempfile::tempdir().unwrap();
    let path = input(&dir);
    std::fs::write(&path, "<img src='absent.png'>").unwrap();
    let out = dir.path().join("out.pdf");
    assert!(!cmd().arg(path).arg(&out).output().unwrap().status.success());
    assert!(!out.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
#[test]
fn failed_conversion_preserves_existing_output() {
    let dir = tempfile::tempdir().unwrap();
    let path = input(&dir);
    std::fs::write(&path, "<img src='absent.png'>").unwrap();
    let out = dir.path().join("out.pdf");
    std::fs::write(&out, b"original").unwrap();
    assert!(!cmd().arg(path).arg(&out).output().unwrap().status.success());
    assert_eq!(std::fs::read(out).unwrap(), b"original");
}
#[test]
fn missing_input() {
    let o = cmd()
        .args(["/nonexistent/report.html", "-"])
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(o.stdout.is_empty());
}
#[test]
fn invalid_utf8() {
    let dir = tempfile::tempdir().unwrap();
    let path = input(&dir);
    std::fs::write(&path, [255, 254, 253]).unwrap();
    let o = cmd().arg(path).arg("-").output().unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("UTF-8"));
}
#[test]
fn invalid_arguments() {
    for args in [
        vec!["-", "-", "--margin", "NaN"],
        vec!["-", "-", "--asset-timeout", "0"],
        vec!["-", "-", "--font", "missing_equals"],
        vec!["-", "-", "--unknown"],
        vec!["-", "-", "--page-size", "B5"],
        vec!["-", "-", "--zoom", "0"],
        vec!["-", "-", "--margin-left", "10px"],
        vec!["-", "-", "--margin-top", "abc"],
        vec!["-", "-", "--safe", "--force"],
        vec!["-", "-", "--zoom", "abc"],
    ] {
        let mut c = cmd()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let _ = c.stdin.take().unwrap().write_all(b"<p>Hi</p>");
        assert!(!c.wait_with_output().unwrap().status.success());
    }
}
#[test]
fn relative_assets_use_input_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = input(&dir);
    std::fs::write(
        dir.path().join("logo.png"),
        include_bytes!("fixtures/logo.png"),
    )
    .unwrap();
    std::fs::write(&path, "<img src='logo.png'>").unwrap();
    let o = cmd().arg(path).arg("-").output().unwrap();
    assert!(o.status.success(), "{:?}", o);
}
#[test]
fn explicit_base_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = input(&dir);
    std::fs::write(&path, "<img src='logo.png'>").unwrap();
    assert!(
        cmd()
            .arg(path)
            .arg("-")
            .args(["--base-dir", "tests/fixtures"])
            .output()
            .unwrap()
            .status
            .success()
    );
}
#[test]
fn invalid_font_is_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = input(&dir);
    let o = cmd()
        .arg(&path)
        .arg("-")
        .arg("--font")
        .arg(format!("Bad={}", path.display()))
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("invalid font"));
}
#[test]
fn utf8_bom_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let path = input(&dir);
    std::fs::write(&path, "\u{feff}<p>Report</p>").unwrap();
    assert!(cmd().arg(path).arg("-").output().unwrap().status.success());
}
#[test]
fn page_size_accepts_wkhtmltopdf_style_names() {
    let dir = tempfile::tempdir().unwrap();
    let path = input(&dir);
    for (size, width) in [("A4", 595.28), ("Letter", 612.0), ("legal", 612.0)] {
        let o = cmd()
            .arg(&path)
            .arg("-")
            .args(["--quiet", "--page-size", size])
            .output()
            .unwrap();
        assert!(o.status.success(), "{size}: {:?}", o);
        let pdf = lopdf::Document::load_mem(&o.stdout).unwrap();
        let page = pdf.get_pages().values().next().copied().unwrap();
        let bbox = pdf.get_object(page).unwrap().as_dict().unwrap();
        let w = match &bbox.get(b"MediaBox").unwrap().as_array().unwrap()[2] {
            lopdf::Object::Real(n) => *n,
            lopdf::Object::Integer(n) => *n as f32,
            _ => panic!("invalid page dimension"),
        };
        assert!((w - width).abs() < 0.2, "{size}: {w}");
    }
}
#[test]
fn node_wkhtmltopdf_wrapper_arguments() {
    // Argument shape produced by the npm wkhtmltopdf 0.4 wrapper for
    // { pageSize: 'letter', zoom: "1.25", marginLeft: "10mm", marginRight: "10mm", output }.
    let dir = tempfile::tempdir().unwrap();
    let pdf = dir.path().join("out.pdf");
    std::fs::write(&pdf, b"previous report").unwrap();
    let mut c = cmd()
        .args(["--quiet", "--page-size", "letter", "--zoom", "1.25"])
        .args(["--margin-left", "10mm", "--margin-right", "10mm", "-"])
        .arg(&pdf)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin
        .take()
        .unwrap()
        .write_all(b"<p>WRAPPER-MARKER</p>")
        .unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success(), "{o:?}");
    assert!(o.stdout.is_empty() && o.stderr.is_empty(), "{o:?}");
    let doc = lopdf::Document::load(&pdf).unwrap();
    let pages = doc.get_pages();
    assert!(doc.extract_text(&[1]).unwrap().contains("WRAPPER-MARKER"));
    let page = doc.get_object(pages[&1]).unwrap().as_dict().unwrap();
    let bbox = page.get(b"MediaBox").unwrap().as_array().unwrap();
    assert!((number(&bbox[2]) - 612.0).abs() < 0.2);
}
#[test]
fn per_side_margins_accept_units() {
    let dir = tempfile::tempdir().unwrap();
    let path = input(&dir);
    let min_x = |args: &[&str]| {
        let o = cmd().arg(&path).arg("-").args(args).output().unwrap();
        assert!(o.status.success(), "{args:?}: {o:?}");
        let doc = lopdf::Document::load_mem(&o.stdout).unwrap();
        let id = doc.get_pages()[&1];
        doc.get_and_decode_page_content(id)
            .unwrap()
            .operations
            .iter()
            .filter(|op| op.operator == "Tm")
            .map(|op| number(&op.operands[4]))
            .fold(f32::MAX, f32::min)
    };
    let default = min_x(&[]);
    // 60 mm = 170.08 pt; 6 cm and -L are the same left margin.
    for args in [
        &["--margin-left", "60mm"][..],
        &["--margin-left", "6cm"],
        &["-L", "60"],
    ] {
        let x = min_x(args);
        assert!(
            (x - default - (170.08 - 34.02)).abs() < 1.0,
            "{args:?}: {x} vs {default}"
        );
    }
    let x = min_x(&["--margin", "1in", "-R", "5mm"]);
    assert!(
        (x - default - (72.0 - 34.02)).abs() < 1.0,
        "{x} vs {default}"
    );
}
fn number(v: &lopdf::Object) -> f32 {
    match v {
        lopdf::Object::Real(n) => *n,
        lopdf::Object::Integer(n) => *n as f32,
        _ => panic!("expected a number"),
    }
}
