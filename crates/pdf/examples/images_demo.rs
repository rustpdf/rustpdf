//! Fase 4 milestone: embed a JPEG (DCTDecode, verbatim) and a transparent PNG
//! (FlateDecode + SMask) and paint them over a colored background.
//!
//! Run: `cargo run -p pdf --example images_demo -- out.pdf`

use image::{ImageEncoder, Rgb, RgbImage, Rgba, RgbaImage};

fn make_jpeg() -> Vec<u8> {
    // A 160x100 RGB gradient.
    let img = RgbImage::from_fn(160, 100, |x, y| {
        Rgb([(x * 255 / 160) as u8, (y * 255 / 100) as u8, 128])
    });
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
        .write_image(&img, 160, 100, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

fn make_png_rgba() -> Vec<u8> {
    // A 120x120 red disc on transparent background (alpha -> SMask).
    let img = RgbaImage::from_fn(120, 120, |x, y| {
        let (dx, dy) = (x as f32 - 60.0, y as f32 - 60.0);
        let r = (dx * dx + dy * dy).sqrt();
        if r < 55.0 {
            Rgba([220, 40, 40, 255])
        } else {
            Rgba([220, 40, 40, 0])
        }
    });
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&img, 120, 120, image::ExtendedColorType::Rgba8)
        .unwrap();
    out
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "images.pdf".to_string());

    let mut doc = pdf::Document::new();
    let jpeg = doc.add_image_jpeg(make_jpeg()).unwrap();
    let disc = doc.add_image_png(make_png_rgba()).unwrap();

    let page = doc.add_page();
    // Light-gray background so the PNG transparency is visible.
    page.content()
        .set_fill_rgb(0.9, 0.92, 0.95)
        .rect(0.0, 0.0, 595.0, 842.0)
        .fill();

    // JPEG at 320x200 pt.
    page.draw_image(jpeg, 72.0, 560.0, 320.0, 200.0);
    // Transparent PNG disc overlapping a dark rectangle to show the alpha.
    page.content()
        .set_fill_rgb(0.1, 0.5, 0.2)
        .rect(72.0, 380.0, 160.0, 160.0)
        .fill();
    page.draw_image(disc, 120.0, 360.0, 160.0, 160.0);

    doc.save(&path).expect("save");
    println!("wrote {path}");
}
