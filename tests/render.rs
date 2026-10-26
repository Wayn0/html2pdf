use html2pdf::{Options, convert};
use std::path::Path;

fn render(html: &str) -> (html2pdf::Conversion, lopdf::Document) {
    let result = convert(html, Path::new("tests/fixtures"), &Options::default()).unwrap();
    assert!(result.bytes.starts_with(b"%PDF-"));
    let pdf = lopdf::Document::load_mem(&result.bytes).expect("independent PDF parser");
    assert_eq!(pdf.get_pages().len(), result.pages);
    for id in pdf.get_pages().values() {
        let page = pdf.get_object(*id).unwrap().as_dict().unwrap();
        let bbox = page.get(b"MediaBox").unwrap().as_array().unwrap();
        let number = |v: &lopdf::Object| match v {
            lopdf::Object::Real(n) => *n,
            lopdf::Object::Integer(n) => *n as f32,
            _ => panic!("invalid page dimension"),
        };
        assert!((number(&bbox[2]) - 595.28).abs() < 0.2);
        assert!((number(&bbox[3]) - 841.89).abs() < 0.2);
    }
    (result, pdf)
}
fn text(pdf: &lopdf::Document) -> String {
    pdf.extract_text(&pdf.get_pages().keys().copied().collect::<Vec<_>>())
        .unwrap()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn simple_a4_pdf_has_extractable_text() {
    let (result, pdf) = render("<h1>Report title</h1><p>Hello PDF</p>");
    assert_eq!(result.pages, 1);
    assert!(text(&pdf).contains("Hello PDF"));
}
#[test]
fn broad_html_element_coverage() {
    let (_, pdf) = render(include_str!("fixtures/elements.html"));
    let actual = text(&pdf);
    let markers = [
        "HEADERMARK",
        "NAVMARK",
        "ANCHORMARK",
        "HEADINGONE",
        "HEADINGTWO",
        "HEADINGTHREE",
        "HEADINGFOUR",
        "HEADINGFIVE",
        "HEADINGSIX",
        "PARAGRAPHMARK",
        "STRONGMARK",
        "BOLDMARK",
        "EMPHASISMARK",
        "ITALICMARK",
        "UNDERLINEMARK",
        "STRIKEMARK",
        "SMALLMARK",
        "HIGHLIGHTMARK",
        "BREAKMARK",
        "ARTICLEMARK",
        "SPANMARK",
        "DIVMARK",
        "ASIDEMARK",
        "QUOTEMARK",
        "CITEMARK",
        "SHORTQUOTEMARK",
        "ABBRMARK",
        "DEFINITIONMARK",
        "TIMEMARK",
        "DATAMARK",
        "PREFORMATTEDMARK",
        "CODEMARK",
        "KEYBOARDMARK",
        "SAMPLEMARK",
        "VARIABLEMARK",
        "SUPMARK",
        "SUBMARK",
        "DELETEDMARK",
        "INSERTEDMARK",
        "UNORDEREDMARK",
        "ORDEREDMARK",
        "NESTEDMARK",
        "TERMMARK",
        "DESCRIPTIONMARK",
        "CAPTIONMARK",
        "TABLECAPTIONMARK",
        "TABLEHEADERMARK",
        "TABLECELLMARK",
        "COLSPANMARK",
        "ROWSPANMARK",
        "ROWSPANFIRST",
        "ROWSPANSECOND",
        "TABLEFOOTMARK",
        "ADDRESSMARK",
        "ENDMARK",
        "FOOTERMARK",
    ];
    let missing = markers
        .iter()
        .filter(|m| !actual.contains(**m))
        .collect::<Vec<_>>();
    assert!(missing.is_empty(), "missing {missing:?}\n{actual}");
}
#[test]
fn thousand_row_table_preserves_every_cell_once_and_in_order() {
    let rows = (0..1000)
        .map(|i| {
            format!(
                "<tr><td>ROW{i:04}END</td><td>VALUE{i:04}END</td><td>{}.25</td></tr>",
                i
            )
        })
        .collect::<String>();
    let (result, pdf) = render(&format!(
        "<style>table {{width:100%;}} td,th {{padding:3px;border:1px solid black;}}</style><h1>Large report</h1><table><thead><tr><th>Identifier</th><th>Value</th><th>Amount</th></tr></thead><tbody>{rows}</tbody></table><p>FINALREPORTMARK</p>"
    ));
    assert!(
        result.pages > 10 && result.pages < 100,
        "unexpected pages {}",
        result.pages
    );
    let actual = text(&pdf);
    let mut last = 0;
    for i in 0..1000 {
        for marker in [format!("ROW{i:04}END"), format!("VALUE{i:04}END")] {
            assert_eq!(
                actual.matches(&marker).count(),
                1,
                "missing/duplicated {marker}"
            );
            let pos = actual.find(&marker).unwrap();
            assert!(pos >= last, "out of order {marker}");
            last = pos;
        }
    }
    assert!(actual.find("FINALREPORTMARK").unwrap() > last);
}
#[test]
fn html5_recovery_entities_and_void_elements() {
    let (_, pdf) = render(
        "<!doctype html><p>FIRST&nbsp;&copy;<br>SECOND<hr><ul><li>THIRD<li>FOURTH</ul><p>A &amp; B &lt; C",
    );
    let t = text(&pdf).split_whitespace().collect::<Vec<_>>().join(" ");
    for m in ["FIRST", "SECOND", "THIRD", "FOURTH", "A & B < C"] {
        assert!(t.contains(m), "{m}: {t}");
    }
}
#[test]
fn explicit_page_break() {
    let (result, pdf) =
        render("<p>FIRSTPAGE</p><div style='page-break-before: always'>SECONDPAGE</div>");
    assert_eq!(result.pages, 2);
    assert!(pdf.extract_text(&[1]).unwrap().contains("FIRSTPAGE"));
    assert!(pdf.extract_text(&[2]).unwrap().contains("SECONDPAGE"));
}
#[test]
fn long_paragraph_preserves_tail() {
    let (_, pdf) = render(&format!(
        "<p>{}</p><p>TAILMARK</p>",
        "Wrapping content for paragraphs. ".repeat(150)
    ));
    assert!(text(&pdf).contains("TAILMARK"));
}
#[test]
fn title_metadata() {
    let (_, pdf) = render("<title>Quarterly report</title><p>Report</p>");
    let info = pdf.trailer.get(b"Info").unwrap().as_reference().unwrap();
    let title = pdf
        .get_object(info)
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Title")
        .unwrap();
    assert_eq!(
        lopdf::decode_text_string(title).unwrap(),
        "Quarterly report"
    );
}
#[test]
fn invalid_options_and_empty_input() {
    for h in ["", " \n\t"] {
        assert!(convert(h, Path::new("."), &Options::default()).is_err());
    }
    for m in [-1., 91., f32::NAN, f32::INFINITY] {
        assert!(
            convert(
                "<p>x</p>",
                Path::new("."),
                &Options {
                    margin_mm: m,
                    ..Options::default()
                }
            )
            .is_err()
        );
    }
}
#[test]
fn missing_image_is_an_error() {
    let e = convert(
        "<img src='missing.png'>",
        Path::new("tests/fixtures"),
        &Options::default(),
    )
    .err()
    .unwrap();
    assert!(e.contains("missing.png"));
}
#[test]
fn offline_rejects_remote_images_and_stylesheets() {
    for html in [
        "<img src='https://example.invalid/logo.png'>",
        "<link rel='stylesheet' href='https://example.invalid/a.css'><p>Hi</p>",
    ] {
        assert!(
            convert(
                html,
                Path::new("."),
                &Options {
                    offline: true,
                    ..Options::default()
                }
            )
            .err()
            .unwrap()
            .contains("offline")
        );
    }
}
#[test]
fn invalid_image_is_an_error() {
    assert!(
        convert(
            "<img src='elements.html'>",
            Path::new("tests/fixtures"),
            &Options::default()
        )
        .err()
        .unwrap()
        .contains("decode image")
    );
}
#[test]
fn unsupported_asset_schemes_are_errors() {
    for src in [
        "ftp://example.com/a.png",
        "file:///tmp/a.png",
        "//example.com/a.png",
    ] {
        assert!(
            convert(
                &format!("<img src='{src}'>"),
                Path::new("."),
                &Options::default()
            )
            .is_err()
        );
    }
}
#[test]
fn css_import_is_actionable_error() {
    assert!(
        convert(
            "<style>@import 'missing.css';</style><p>Hi</p>",
            Path::new("."),
            &Options::default()
        )
        .err()
        .unwrap()
        .contains("@import")
    );
}
#[test]
fn malformed_data_image_is_error() {
    for src in [
        "data:image/png;base64,!!!",
        "data:image/png,nope",
        "data:broken",
    ] {
        assert!(
            convert(
                &format!("<img src='{src}'>"),
                Path::new("."),
                &Options::default()
            )
            .is_err()
        );
    }
}

#[test]
fn hidden_content_is_not_in_pdf() {
    let (_, pdf) = render(
        "<style>.secret {display:none}</style><p>VISIBLEMARK</p><p class='secret'>HIDDENMARK</p>",
    );
    let t = text(&pdf);
    assert!(t.contains("VISIBLEMARK"));
    assert!(!t.contains("HIDDENMARK"));
}
#[test]
fn excessive_nesting_is_an_error() {
    let html = format!("{}deep{}", "<div>".repeat(260), "</div>".repeat(260));
    assert!(
        convert(&html, Path::new("."), &Options::default())
            .err()
            .unwrap()
            .contains("nesting")
    );
}
#[test]
fn oversized_input_is_an_error() {
    let html = "a".repeat(html2pdf::MAX_INPUT_BYTES + 1);
    assert!(
        convert(&html, Path::new("."), &Options::default())
            .err()
            .unwrap()
            .contains("32 MiB")
    );
}
#[test]
fn empty_image_source_is_an_error() {
    for html in ["<img>", "<img src=''>"] {
        assert!(convert(html, Path::new("."), &Options::default()).is_err());
    }
}
#[test]
fn zero_remote_timeout_is_an_error() {
    assert!(
        convert(
            "<p>Hi</p>",
            Path::new("."),
            &Options {
                asset_timeout: std::time::Duration::ZERO,
                ..Options::default()
            }
        )
        .is_err()
    );
}
#[test]
fn margin_options_keep_a4_page_size() {
    for margin in [0., 20., 90.] {
        let result = convert(
            "<p>Margin test</p>",
            Path::new("."),
            &Options {
                margin_mm: margin,
                ..Options::default()
            },
        )
        .unwrap();
        assert_eq!(result.pages, 1);
        let doc = lopdf::Document::load_mem(&result.bytes).unwrap();
        assert!(text(&doc).contains("Margin test"));
    }
}
#[test]
fn tall_table_rows_preserve_content_across_pages() {
    let rows = (0..120)
        .map(|i| {
            format!(
                "<tr><td>START{i:03}</td><td>{}<br>END{i:03}</td></tr>",
                "Long wrapped description with several words. ".repeat(4)
            )
        })
        .collect::<String>();
    let (_, pdf) = render(&format!(
        "<style>table {{width:100%}} td {{width:50%;padding:6px}}</style><table>{rows}</table>"
    ));
    let t = text(&pdf);
    for i in 0..120 {
        for m in [format!("START{i:03}"), format!("END{i:03}")] {
            assert_eq!(t.matches(&m).count(), 1, "{m}");
        }
    }
}
#[test]
fn script_content_is_not_rendered() {
    let (c, pdf) = render("<script>SECRETJSMARK</script><p>VISIBLEMARK</p>");
    assert!(!text(&pdf).contains("SECRETJSMARK"));
    assert!(c.warnings.iter().any(|s| s.contains("JavaScript")));
}
#[test]
fn page_size_option_sets_media_box() {
    for (size, width, height) in [
        (html2pdf::PageSize::A3, 841.89, 1190.55),
        (html2pdf::PageSize::A4, 595.28, 841.89),
        (html2pdf::PageSize::A5, 419.53, 595.28),
        (html2pdf::PageSize::Letter, 612.0, 792.0),
        (html2pdf::PageSize::Legal, 612.0, 1008.0),
    ] {
        let rows = (0..200)
            .map(|i| format!("<tr><td>row-{i}</td></tr>"))
            .collect::<String>();
        let result = convert(
            &format!("<table>{rows}</table><p>END-MARKER</p>"),
            Path::new("."),
            &Options {
                page_size: size,
                ..Options::default()
            },
        )
        .unwrap();
        let pdf = lopdf::Document::load_mem(&result.bytes).unwrap();
        assert!(result.pages > 1, "{size:?}");
        for id in pdf.get_pages().values() {
            let page = pdf.get_object(*id).unwrap().as_dict().unwrap();
            let bbox = page.get(b"MediaBox").unwrap().as_array().unwrap();
            let number = |v: &lopdf::Object| match v {
                lopdf::Object::Real(n) => *n,
                lopdf::Object::Integer(n) => *n as f32,
                _ => panic!("invalid page dimension"),
            };
            assert!((number(&bbox[2]) - width).abs() < 0.2, "{size:?}");
            assert!((number(&bbox[3]) - height).abs() < 0.2, "{size:?}");
        }
        let text = text(&pdf);
        for i in 0..200 {
            assert_eq!(text.matches(&format!("row-{i} ")).count(), 1, "{size:?}");
        }
        assert!(text.ends_with("END-MARKER"), "{size:?}");
    }
}
#[test]
fn margin_must_leave_printable_area() {
    let result = convert(
        "<p>x</p>",
        Path::new("."),
        &Options {
            page_size: html2pdf::PageSize::A5,
            margin_mm: 80.,
            ..Options::default()
        },
    );
    assert!(result.is_err());
}
