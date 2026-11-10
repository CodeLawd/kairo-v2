//! The join card's QR code against Electron's `qrcode` package
//! (`tools/ts-oracle/modules/audience-qr.ts`): the same module grid and the
//! same pixels, so phones scan it exactly as before.

use base64::Engine;
use kairo_audience::qr::{qr_data_url, qr_matrix, qr_pixels};
use serde_json::json;

fn urls() -> Vec<String> {
    let mut urls = vec![
        "https://kairopresenter.com/q/75XNMW".to_owned(),
        "http://localhost:3001/q/ABCDEF".to_owned(),
        "https://kairopresenter.com/q/A2B3C4".to_owned(),
        "https://staging.kairopresenter.com/q/ZZZZZZ".to_owned(),
        "https://example.org/a/very/long/path/that/pushes/the/code/to/a/larger/version?with=query&and=more#fragment".to_owned(),
        // Mode switches the optimiser has to weigh.
        "https://kairopresenter.com/q/234567".to_owned(),
        "https://kairopresenter.com/q/2A3B4C?ref=1234567890123".to_owned(),
        "12345678901234567890".to_owned(),
        "HELLO WORLD 123 $%*+-./:".to_owned(),
        "https://kairopresenter.com/q/ÉGLISE-✓".to_owned(),
    ];
    // Codes across the whole alphabet, as rooms are created.
    let alphabet = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    for i in 0..40usize {
        let code: String = (0..6)
            .map(|k| alphabet[(i * 7 + k * 13) % alphabet.len()] as char)
            .collect();
        urls.push(format!("https://kairopresenter.com/q/{code}"));
    }
    urls
}

fn decode_png(data_url: &str) -> (u32, Vec<u8>) {
    let b64 = data_url
        .strip_prefix("data:image/png;base64,")
        .expect("a PNG data URL");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap();
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().unwrap();
    let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buffer).unwrap();
    buffer.truncate(info.buffer_size());
    // Normalise to RGBA.
    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        other => panic!("unexpected colour type {other:?}"),
    };
    (info.width, rgba)
}

#[test]
fn module_grid_matches_electron() {
    let urls = urls();
    let inputs: Vec<_> = urls.iter().map(|u| json!([u])).collect();
    let expected = kairo_oracle::oracle_values("oracle:audience-qr.matrix", &inputs);
    for (url, expected) in urls.iter().zip(expected) {
        let (size, dark) = qr_matrix(url).unwrap();
        let bits: String = dark.iter().map(|d| if *d { '1' } else { '0' }).collect();
        assert_eq!(size as u64, expected["size"].as_u64().unwrap(), "{url}");
        assert_eq!(bits, expected["bits"].as_str().unwrap(), "{url}");
    }
}

#[test]
fn pixels_match_electrons_data_url() {
    let urls = urls();
    let inputs: Vec<_> = urls.iter().map(|u| json!([u])).collect();
    let expected = kairo_oracle::oracle_values("oracle:audience-qr.dataUrl", &inputs);
    for (url, expected) in urls.iter().zip(expected) {
        let (width, electron) = decode_png(expected.as_str().unwrap());
        let (size, dark) = qr_matrix(url).unwrap();
        let (side, rust) = qr_pixels(size, &dark);
        assert_eq!(side as u32, width, "{url}");
        assert!(rust == electron, "{url}: pixels differ");
        // The data URL itself decodes to the same pixels.
        assert_eq!(
            decode_png(&qr_data_url(url).unwrap()),
            (width, electron),
            "{url}"
        );
    }
}
