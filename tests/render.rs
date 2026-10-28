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
                    margins: html2pdf::Margins::uniform(m),
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
                margins: html2pdf::Margins::uniform(margin),
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
fn zoom_scales_content_but_keeps_page_size() {
    let rows = (0..200)
        .map(|i| format!("<tr><td>row-{i}</td></tr>"))
        .collect::<String>();
    let html = format!("<table>{rows}</table><p>END-MARKER</p>");
    let render = |zoom| {
        convert(
            &html,
            Path::new("."),
            &Options {
                zoom,
                ..Options::default()
            },
        )
        .unwrap()
    };
    let normal = render(1.0);
    let zoomed = render(2.0);
    assert!(
        zoomed.pages > normal.pages,
        "{} vs {}",
        zoomed.pages,
        normal.pages
    );
    let pdf = lopdf::Document::load_mem(&zoomed.bytes).unwrap();
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
        let ops = pdf.get_and_decode_page_content(*id).unwrap().operations;
        let first = ops.iter().find(|op| op.operator == "cm").unwrap();
        let matrix = first.operands.iter().map(number).collect::<Vec<_>>();
        assert_eq!(matrix, [2.0, 0.0, 0.0, 2.0, 0.0, 0.0]);
    }
    let text = text(&pdf);
    for i in 0..200 {
        assert_eq!(text.matches(&format!("row-{i} ")).count(), 1);
    }
    assert!(text.ends_with("END-MARKER"));
    for zoom in [0.0, f32::NAN, 11.0] {
        assert!(
            convert(
                "<p>x</p>",
                Path::new("."),
                &Options {
                    zoom,
                    ..Options::default()
                }
            )
            .is_err()
        );
    }
}
#[test]
fn margin_must_leave_printable_area() {
    let result = convert(
        "<p>x</p>",
        Path::new("."),
        &Options {
            page_size: html2pdf::PageSize::A5,
            margins: html2pdf::Margins::uniform(80.),
            ..Options::default()
        },
    );
    assert!(result.is_err());
    let lopsided = html2pdf::Margins {
        left: 90.,
        right: 90.,
        ..html2pdf::Margins::uniform(10.)
    };
    let result = convert(
        "<p>x</p>",
        Path::new("."),
        &Options {
            page_size: html2pdf::PageSize::A5,
            margins: lopsided,
            ..Options::default()
        },
    );
    assert!(result.is_err());
}
#[test]
fn bootstrap_clearfix_keeps_content_and_contains_floats() {
    // Bootstrap's clearfix used to apply `display: table` to `.row` itself and drop its content.
    let (result, pdf) = render(
        "<style>.row:before, .row:after { display: table; content: \" \"; }\
         .row:after { clear: both; }</style>\
         <div class='row'><div style='width:50%;float:left'>\
         <img src='logo.png' style='width:180px;height:150px'></div>\
         <div style='width:50%;float:left'><p>RIGHT-COLUMN</p></div></div>\
         <div class='row'><p>INSIDE-ROW</p></div><h3>AFTER-FLOATS</h3>",
    );
    assert!(result.warnings.iter().any(|w| w.contains("::after")));
    let all = text(&pdf);
    for marker in ["RIGHT-COLUMN", "INSIDE-ROW", "AFTER-FLOATS"] {
        assert!(all.contains(marker), "{marker} missing from {all:?}");
    }
    let page = *pdf.get_pages().values().next().unwrap();
    let ops = lopdf::content::Content::decode(&pdf.get_page_content(page))
        .unwrap()
        .operations;
    let y = |op: &lopdf::content::Operation| op.operands[5].as_float().unwrap();
    let image_bottom = ops.iter().find(|op| op.operator == "cm").map(y).unwrap();
    let last_text = ops
        .iter()
        .filter(|op| op.operator == "Tm")
        .map(y)
        .next_back();
    assert!(
        last_text.unwrap() < image_bottom,
        "text after the floated row overlaps the image"
    );
}
#[test]
fn bootstrap_report_keeps_logos_chart_and_styles_in_place() {
    // Sanitized from a production report converted with
    // `--page-size Letter --zoom 1.25 -L 10mm -R 10mm`. Its stylesheet has a stray
    // `class="..."` inside a rule, which used to discard every later rule; the logos
    // overlapped the heading and the block-level chart was drawn over the next one.
    let options = Options {
        page_size: html2pdf::PageSize::Letter,
        zoom: 1.25,
        margins: html2pdf::Margins {
            left: 10.,
            right: 10.,
            ..html2pdf::Margins::uniform(12.)
        },
        ..Options::default()
    };
    let result = convert(
        include_str!("fixtures/bootstrap-report.html"),
        Path::new("tests/fixtures"),
        &options,
    )
    .unwrap();
    let pdf = lopdf::Document::load_mem(&result.bytes).unwrap();
    let all = text(&pdf);
    for marker in [
        "Wobbleton Snorkel Summary",
        "Breakdown of florps",
        "Snorkel Tally Below Chart",
        "Day Quin 42 43 44 45 46 47",
        "Wibble Reasons",
        "Krandle's Nook",
        "Tarnwhistle 5 6 7 8 9 10 11",
        "REASONS-END",
    ] {
        assert!(all.contains(marker), "{marker} missing from {all:?}");
    }

    let page = *pdf.get_pages().values().next().unwrap();
    let ops = lopdf::content::Content::decode(&pdf.get_page_content(page))
        .unwrap()
        .operations;
    let num = |o: &lopdf::Object| o.as_float().unwrap();
    // Image placements [width, height, x, y] by name, from the `cm` before each `Do`.
    let mut images = std::collections::BTreeMap::new();
    let mut matrix = vec![];
    // Heading baselines: text drawn at the page's largest font size.
    let mut headings: Vec<(f32, f32, f32)> = vec![];
    let mut size = 0.;
    for op in &ops {
        match op.operator.as_str() {
            "cm" => matrix = op.operands.iter().map(num).collect(),
            "Do" => {
                let name = op.operands[0].as_name().unwrap();
                let m = &matrix;
                images.insert(
                    String::from_utf8_lossy(name).into_owned(),
                    [m[0], m[3], m[4], m[5]],
                );
            }
            "Tf" => size = num(&op.operands[1]),
            "Tm" => headings.push((size, num(&op.operands[4]), num(&op.operands[5]))),
            _ => {}
        }
    }
    let largest = headings.iter().map(|h| h.0).fold(0., f32::max);
    headings.retain(|h| h.0 == largest);
    headings.dedup_by(|a, b| (a.2 - b.2).abs() < 1.);
    let [title, tally] = headings[..] else {
        panic!("expected two headings on page 1, got {headings:?}");
    };

    let image = |name: &str| images[&format!("HtmlImg_report_{name}_png")];
    let aspect = |[w, h, ..]: [f32; 4]| w / h;
    for (name, natural) in [("wide_logo", 1080. / 282.), ("site_logo", 350. / 150.)] {
        let logo = image(name);
        assert!(
            (aspect(logo) - natural).abs() < 0.03,
            "{name} distorted: {logo:?}"
        );
        assert!(
            logo[3] > title.2 + title.0,
            "{name} {logo:?} overlaps heading {title:?}"
        );
    }
    // `.pull-left`/`.pull-right` are `!important`, which the renderer used to discard,
    // leaving the site logo at the left edge of its half of the page.
    let [wide, site] = [image("wide_logo"), image("site_logo")];
    assert!(wide[2] < 60., "wide logo is not at the left: {wide:?}");
    // Placements are in unzoomed units; an outer transform applies `--zoom 1.25`.
    let right_margin = (612. - 10. * 72. / 25.4) / 1.25;
    assert!(
        (site[2] + site[0] - right_margin).abs() < 15.,
        "site logo does not float right: {site:?}"
    );
    let chart = image("chart");
    assert!(
        (aspect(chart) - 175. / 174.).abs() < 0.03,
        "chart distorted: {chart:?}"
    );
    assert!(
        chart[3] > tally.2 + tally.0,
        "chart {chart:?} overlaps heading {tally:?}"
    );
    assert!(
        chart[2] > 150.,
        "center-block chart is not centered: {chart:?}"
    );
    // `.text-center` comes after the malformed rule, so it proves the cascade survived.
    assert!(title.1 > 120., "heading is not centered: {title:?}");
}
#[test]
fn wide_sized_tables_wrap_inside_the_page() {
    // Production reports use `--page-size Letter --zoom 1.25 -L 10mm -R 10mm`; their
    // 7- and 8-column tables ran past the right margin because the renderer never
    // wraps auto-layout cells. The first-row labels here are wider than the page.
    let options = Options {
        page_size: html2pdf::PageSize::Letter,
        zoom: 1.25,
        margins: html2pdf::Margins {
            left: 10.,
            right: 10.,
            ..html2pdf::Margins::uniform(12.)
        },
        ..Options::default()
    };
    let header: String = (1..=8)
        .map(|i| format!("<th>Quorp Wibble Total {i}</th>"))
        .collect();
    let row = |label: &str| -> String {
        let cells: String = (1..=7).map(|i| format!("<td>{i}</td>")).collect();
        format!("<tr><td>{label}</td>{cells}</tr>")
    };
    let rows: String = ["Florp Glim", "Snargle Wex", "TABLE-END"].map(row).concat();
    let html = format!(
        "<style>body {{ margin-left: 20px }} table {{ width: 100%; border-collapse: collapse }}\
         th, td {{ border: 1px solid #ddd; padding: 4px; font-size: 10px }}</style>\
         <table><thead><tr>{header}</tr></thead><tbody>{rows}</tbody></table><p>AFTER-TABLE</p>"
    );
    let result = convert(&html, Path::new("tests/fixtures"), &options).unwrap();
    let pdf = lopdf::Document::load_mem(&result.bytes).unwrap();
    assert_eq!(result.pages, 1);
    let all = text(&pdf).replace(' ', "");
    for marker in ["Total8", "TABLE-END1234567", "AFTER-TABLE"] {
        assert!(all.contains(marker), "{marker} missing from {all:?}");
    }

    let num = |o: &lopdf::Object| o.as_float().unwrap();
    // Placements are in unzoomed units; an outer transform applies `--zoom 1.25`.
    let right_margin = (612. - 10. * 72. / 25.4) / 1.25;
    let page = *pdf.get_pages().values().next().unwrap();
    let ops = lopdf::content::Content::decode(&pdf.get_page_content(page))
        .unwrap()
        .operations;
    // Table borders are filled paths; their right edge must stay inside the margin.
    let border_right = ops
        .iter()
        .filter(|op| op.operator == "l")
        .map(|op| num(&op.operands[0]))
        .fold(0., f32::max);
    assert!(
        border_right > right_margin - 20. && border_right <= right_margin + 0.5,
        "table right edge {border_right} vs margin {right_margin}"
    );
    let text_right = ops
        .iter()
        .filter(|op| op.operator == "Tm")
        .map(|op| num(&op.operands[4]))
        .fold(0., f32::max);
    assert!(text_right < border_right, "text starts past the table edge");
}
