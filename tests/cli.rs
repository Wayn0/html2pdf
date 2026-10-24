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
fn existing_output_preserved_without_force() {
    let dir = tempfile::tempdir().unwrap();
    let path = input(&dir);
    let pdf = dir.path().join("out.pdf");
    std::fs::write(&pdf, b"original").unwrap();
    let o = cmd().arg(path).arg(&pdf).output().unwrap();
    assert!(!o.status.success());
    assert_eq!(std::fs::read(pdf).unwrap(), b"original");
}
#[test]
fn force_replaces_output() {
    let dir = tempfile::tempdir().unwrap();
    let path = input(&dir);
    let pdf = dir.path().join("out.pdf");
    std::fs::write(&pdf, b"original").unwrap();
    assert!(
        cmd()
            .arg(path)
            .arg(&pdf)
            .arg("--force")
            .output()
            .unwrap()
            .status
            .success()
    );
    lopdf::Document::load(pdf).unwrap();
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
fn failed_conversion_preserves_existing_output_even_with_force() {
    let dir = tempfile::tempdir().unwrap();
    let path = input(&dir);
    std::fs::write(&path, "<img src='absent.png'>").unwrap();
    let out = dir.path().join("out.pdf");
    std::fs::write(&out, b"original").unwrap();
    assert!(
        !cmd()
            .arg(path)
            .arg(&out)
            .arg("--force")
            .output()
            .unwrap()
            .status
            .success()
    );
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
