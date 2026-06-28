//! Image *extraction* round-trips (reverse of Fase 4): embed JPEG/PNG images
//! into a document, then pull them back out with `pdf::extract_images` and check
//! that JPEG comes out verbatim and PNG decodes back to the original pixels.

use image::{ImageEncoder, Rgb, RgbImage, Rgba, RgbaImage};
use pdf::{extract_images, Document, ImageFormat};

fn jpeg_bytes(w: u32, h: u32) -> Vec<u8> {
    let img = RgbImage::from_fn(w, h, |x, _| Rgb([(x % 256) as u8, 100, 200]));
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
        .write_image(&img, w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

fn png_rgb(w: u32, h: u32) -> (RgbImage, Vec<u8>) {
    let img = RgbImage::from_fn(w, h, |x, y| Rgb([x as u8 * 6, y as u8 * 8, 90]));
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&img, w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    (img, png)
}

#[test]
fn jpeg_extracts_verbatim() {
    let jpeg = jpeg_bytes(64, 48);
    let mut doc = Document::new();
    let id = doc.add_image_jpeg(jpeg.clone()).unwrap();
    doc.add_page().draw_image(id, 72.0, 600.0, 128.0, 96.0);
    let bytes = doc.to_bytes().unwrap();

    let images = extract_images(&bytes).unwrap();
    assert_eq!(images.len(), 1);
    let img = &images[0];
    assert_eq!(img.page, 0);
    assert_eq!(img.format, ImageFormat::Jpeg);
    assert_eq!((img.width, img.height), (64, 48));
    // DCTDecode is passed through untouched — byte-identical to the source JPEG.
    assert_eq!(img.data, jpeg);
    assert_eq!(img.file_name(), "page1_Im0.jpg");
}

#[test]
fn png_extracts_and_decodes_to_original_pixels() {
    let (src, png) = png_rgb(40, 30);
    let mut doc = Document::new();
    let id = doc.add_image_png(&png).unwrap();
    doc.add_page().draw_image(id, 72.0, 600.0, 160.0, 120.0);
    let bytes = doc.to_bytes().unwrap();

    let images = extract_images(&bytes).unwrap();
    assert_eq!(images.len(), 1);
    let img = &images[0];
    assert_eq!(img.format, ImageFormat::Png);
    assert_eq!((img.width, img.height), (40, 30));

    // Re-decode the extracted PNG and compare to the original pixels.
    let decoded = image::load_from_memory(&img.data).unwrap().to_rgb8();
    assert_eq!(decoded.dimensions(), (40, 30));
    assert_eq!(decoded.as_raw(), src.as_raw());
}

#[test]
fn rgba_png_extracts_with_alpha() {
    // Left half opaque red, right half transparent.
    let src = RgbaImage::from_fn(20, 10, |x, _| {
        if x < 10 {
            Rgba([255, 0, 0, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&src, 20, 10, image::ExtendedColorType::Rgba8)
        .unwrap();

    let mut doc = Document::new();
    let id = doc.add_image_png(&png).unwrap();
    doc.add_page().draw_image(id, 72.0, 600.0, 100.0, 50.0);
    let bytes = doc.to_bytes().unwrap();

    let images = extract_images(&bytes).unwrap();
    assert_eq!(images.len(), 1);
    let decoded = image::load_from_memory(&images[0].data).unwrap().to_rgba8();
    assert_eq!(decoded.dimensions(), (20, 10));
    // Alpha survived the SMask round-trip.
    assert_eq!(decoded.get_pixel(0, 0)[3], 255);
    assert_eq!(decoded.get_pixel(19, 0)[3], 0);
}

#[test]
fn multiple_pages_and_save_in() {
    let jpeg = jpeg_bytes(32, 32);
    let (_, png) = png_rgb(16, 16);
    let mut doc = Document::new();
    let j = doc.add_image_jpeg(jpeg).unwrap();
    let p = doc.add_image_png(&png).unwrap();
    doc.add_page().draw_image(j, 72.0, 600.0, 64.0, 64.0);
    doc.add_page().draw_image(p, 72.0, 600.0, 64.0, 64.0);
    let bytes = doc.to_bytes().unwrap();

    let images = extract_images(&bytes).unwrap();
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].page, 0);
    assert_eq!(images[1].page, 1);

    let dir = std::env::temp_dir().join("rustpdf_extract_images_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = images[1].save_in(&dir).unwrap();
    assert!(path.exists());
    assert_eq!(path.file_name().unwrap(), images[1].file_name().as_str());
}
