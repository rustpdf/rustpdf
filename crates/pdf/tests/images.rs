//! Fase 4 end-to-end tests: JPEG verbatim embedding, PNG re-encode, palette →
//! Indexed, alpha → SMask (verified by rendering and sampling pixels), 16-bit.

use image::{ImageEncoder, Rgb, RgbImage, Rgba, RgbaImage};
use pdf::Document;
use testkit::{validate_with, Validator, ValidatorStatus};

fn qpdf_ok(path: &std::path::Path) {
    for r in validate_with(path, &[Validator::Qpdf, Validator::MutoolClean]) {
        if let ValidatorStatus::Fail { code, output } = r.status {
            panic!("{:?} failed (code {code:?}):\n{output}", r.validator);
        }
    }
}

fn jpeg_bytes(w: u32, h: u32) -> Vec<u8> {
    let img = RgbImage::from_fn(w, h, |x, _| Rgb([(x % 256) as u8, 100, 200]));
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
        .write_image(&img, w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn jpeg_is_embedded_verbatim_with_dctdecode() {
    let jpeg = jpeg_bytes(64, 48);
    let mut doc = Document::new();
    let id = doc.add_image_jpeg(jpeg.clone()).unwrap();
    doc.add_page().draw_image(id, 72.0, 600.0, 128.0, 96.0);
    let bytes = doc.to_bytes().unwrap();

    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Filter /DCTDecode"));
    assert!(text.contains("/Subtype /Image"));
    // Verbatim: the original JPEG byte stream appears unchanged (no recompress).
    assert!(contains(&bytes, &jpeg), "JPEG bytes were modified");

    let path = std::env::temp_dir().join("rustpdf_img_jpeg.pdf");
    std::fs::write(&path, &bytes).unwrap();
    qpdf_ok(&path);
}

#[test]
fn png_rgb_opaque_roundtrips() {
    let img = RgbImage::from_fn(40, 30, |x, y| Rgb([x as u8 * 6, y as u8 * 8, 90]));
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&img, 40, 30, image::ExtendedColorType::Rgb8)
        .unwrap();

    let mut doc = Document::new();
    let id = doc.add_image_png(&png).unwrap();
    doc.add_page().draw_image(id, 72.0, 600.0, 160.0, 120.0);
    let bytes = doc.to_bytes().unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("/Filter /FlateDecode"));

    let path = std::env::temp_dir().join("rustpdf_img_png_rgb.pdf");
    std::fs::write(&path, &bytes).unwrap();
    qpdf_ok(&path);
}

#[test]
fn png_alpha_becomes_smask_and_shows_through() {
    // Opaque red square on the left half, fully transparent on the right half.
    let png_img = RgbaImage::from_fn(40, 20, |x, _| {
        if x < 20 {
            Rgba([255, 0, 0, 255])
        } else {
            Rgba([255, 0, 0, 0])
        }
    });
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&png_img, 40, 20, image::ExtendedColorType::Rgba8)
        .unwrap();

    let mut doc = Document::new();
    let id = doc.add_image_png(&png).unwrap();
    let page = doc.add_page_sized(200.0, 200.0);
    // Blue background, then the image spanning the whole page.
    page.content()
        .set_fill_rgb(0.0, 0.0, 1.0)
        .rect(0.0, 0.0, 200.0, 200.0)
        .fill();
    page.draw_image(id, 0.0, 0.0, 200.0, 200.0);

    let bytes = doc.to_bytes().unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("/SMask"));
    let path = std::env::temp_dir().join("rustpdf_img_alpha.pdf");
    std::fs::write(&path, &bytes).unwrap();
    qpdf_ok(&path);

    // Render and sample: left half should be red-ish, right half blue (bg shows).
    let png_out = std::env::temp_dir().join("rustpdf_img_alpha.png");
    if testkit::render_to_png(&path, &png_out, 72).is_ok() {
        let rendered = image::open(&png_out).unwrap().to_rgb8();
        let (w, h) = rendered.dimensions();
        let left = rendered.get_pixel(w / 4, h / 2).0;
        let right = rendered.get_pixel(3 * w / 4, h / 2).0;
        assert!(
            left[0] > 150 && left[2] < 100,
            "left should be red: {left:?}"
        );
        assert!(
            right[2] > 150 && right[0] < 100,
            "right should be blue bg: {right:?}"
        );
    }
}

#[test]
fn png_palette_and_16bit_embed() {
    // Palette PNG via the `png` crate (image crate has no indexed encoder).
    let mut palette_png = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut palette_png, 2, 2);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_palette(vec![255, 0, 0, 0, 255, 0, 0, 0, 255]);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[0, 1, 2, 0]).unwrap();
    }
    let mut doc = Document::new();
    let id = doc.add_image_png(&palette_png).unwrap();
    doc.add_page().draw_image(id, 72.0, 600.0, 100.0, 100.0);
    let bytes = doc.to_bytes().unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("/Indexed"));
    let path = std::env::temp_dir().join("rustpdf_img_palette.pdf");
    std::fs::write(&path, &bytes).unwrap();
    qpdf_ok(&path);

    // 16-bit grayscale.
    let mut g16 = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut g16, 2, 1);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Sixteen);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[0x12, 0x34, 0xAB, 0xCD]).unwrap();
    }
    let mut doc = Document::new();
    let id = doc.add_image_png(&g16).unwrap();
    doc.add_page().draw_image(id, 72.0, 600.0, 100.0, 50.0);
    let bytes = doc.to_bytes().unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("/BitsPerComponent 16"));
    let path = std::env::temp_dir().join("rustpdf_img_g16.pdf");
    std::fs::write(&path, &bytes).unwrap();
    qpdf_ok(&path);
}
