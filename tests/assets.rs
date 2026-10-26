use base64::{Engine, engine::general_purpose::STANDARD};
use html2pdf::{Options, convert};
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    thread,
    time::Duration,
};
const PNG: &[u8] = include_bytes!("fixtures/logo.png");
fn has_painted_image(bytes: &[u8]) {
    let doc = lopdf::Document::load_mem(bytes).unwrap();
    assert!(doc.objects.values().any(|o| o.as_stream().is_ok_and(|s| {
        s.dict
            .get(b"Subtype")
            .is_ok_and(|v| v.as_name().is_ok_and(|n| n == b"Image"))
    })));
    let mut painted = false;
    for page in doc.get_pages().values() {
        let content = doc.get_page_content(*page);
        let ops = lopdf::content::Content::decode(&content).unwrap();
        painted |= ops.operations.iter().any(|op| op.operator == "Do");
    }
    assert!(painted, "image embedded but not painted");
}
fn server(status: &str, body: Vec<u8>, delay: Duration) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/asset", listener.local_addr().unwrap());
    let status = status.to_string();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = [0; 4096];
        let n = stream.read(&mut request).unwrap_or(0);
        thread::sleep(delay);
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(&body);
        String::from_utf8_lossy(&request[..n]).into_owned()
    });
    (url, handle)
}
#[test]
fn local_image_is_embedded_and_painted() {
    let c = convert(
        "<p>Logo</p><img src='logo.png' style='width:64px;height:64px'>",
        Path::new("tests/fixtures"),
        &Options::default(),
    )
    .unwrap();
    has_painted_image(&c.bytes);
}
#[test]
fn base64_image_is_embedded_and_painted() {
    let c = convert(
        &format!(
            "<img src='data:image/png;base64,{}' style='width:64px;height:64px'>",
            STANDARD.encode(PNG)
        ),
        Path::new("."),
        &Options::default(),
    )
    .unwrap();
    has_painted_image(&c.bytes);
}
#[test]
fn remote_image_is_embedded_and_painted() {
    let (url, handle) = server("200 OK", PNG.to_vec(), Duration::ZERO);
    let result = convert(
        &format!("<img src='{url}' style='width:64px;height:64px'>"),
        Path::new("."),
        &Options::default(),
    );
    handle.join().unwrap();
    has_painted_image(&result.unwrap().bytes);
}
#[test]
fn remote_url_with_spaces_is_percent_encoded() {
    let (url, handle) = server("200 OK", PNG.to_vec(), Duration::ZERO);
    let result = convert(
        &format!("<img src='{url}/logo _70 .png' style='width:64px;height:64px'>"),
        Path::new("."),
        &Options::default(),
    );
    let request = handle.join().unwrap();
    has_painted_image(&result.unwrap().bytes);
    assert!(
        request.starts_with("GET /asset/logo%20_70%20.png HTTP/1.1\r\n"),
        "{request}"
    );
}
#[test]
fn http_404_is_error() {
    let (url, handle) = server("404 Not Found", b"missing".to_vec(), Duration::ZERO);
    let result = convert(
        &format!("<img src='{url}'>"),
        Path::new("."),
        &Options::default(),
    );
    handle.join().unwrap();
    assert!(result.err().unwrap().contains("404"));
}
#[test]
fn remote_asset_timeout_is_error() {
    let (url, handle) = server("200 OK", PNG.to_vec(), Duration::from_millis(200));
    let result = convert(
        &format!("<img src='{url}'>"),
        Path::new("."),
        &Options {
            asset_timeout: Duration::from_millis(40),
            ..Options::default()
        },
    );
    handle.join().unwrap();
    assert!(result.is_err());
}
#[test]
fn local_stylesheet_affects_pdf_colour() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("report.css"), "p { color: #ff0000; }").unwrap();
    let c = convert(
        "<link rel='stylesheet' href='report.css'><p>Red text</p>",
        dir.path(),
        &Options::default(),
    )
    .unwrap();
    let doc = lopdf::Document::load_mem(&c.bytes).unwrap();
    let stream = doc.get_page_content(*doc.get_pages().get(&1).unwrap());
    let ops = lopdf::content::Content::decode(&stream).unwrap();
    assert!(
        ops.operations
            .iter()
            .any(|op| op.operator == "rg" && format!("{:?}", op.operands) == "[1, 0, 0]"),
        "{:?}",
        ops.operations
    );
}
#[test]
fn remote_stylesheet() {
    let (url, handle) = server("200 OK", b"p { color: #ff0000; }".to_vec(), Duration::ZERO);
    let result = convert(
        &format!("<link rel='stylesheet' href='{url}'><p>Remote CSS</p>"),
        Path::new("."),
        &Options::default(),
    );
    handle.join().unwrap();
    assert!(result.is_ok());
}
#[test]
fn missing_stylesheet_errors() {
    assert!(
        convert(
            "<link rel='stylesheet' href='absent.css'><p>Hi</p>",
            Path::new("."),
            &Options::default()
        )
        .is_err()
    );
}
#[test]
fn repeated_image_source_only_embeds_once() {
    let c = convert(
        "<img src='logo.png'><img src='logo.png'>",
        Path::new("tests/fixtures"),
        &Options::default(),
    )
    .unwrap();
    let doc = lopdf::Document::load_mem(&c.bytes).unwrap();
    let count = doc
        .objects
        .values()
        .filter(|o| {
            o.as_stream().is_ok_and(|s| {
                s.dict
                    .get(b"Subtype")
                    .is_ok_and(|v| v.as_name().is_ok_and(|n| n == b"Image"))
            })
        })
        .count();
    assert_eq!(count, 1);
}
