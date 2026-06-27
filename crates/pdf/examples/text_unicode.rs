//! Fase 3A.5 / 3B milestone: embedded, subsetted Unicode text that extracts
//! correctly. Run: `cargo run -p pdf --example text_unicode -- out.pdf`

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "text.pdf".to_string());

    let font_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/Roboto-Regular.ttf"
    );

    let mut doc = pdf::Document::new();
    let roboto = doc.add_font_file(font_path).expect("load font");

    let page = doc.add_page();
    let t = page.text(roboto, 24.0);
    t.fill(0.1, 0.1, 0.1).at(72.0, 740.0).show("Hello, World!");
    // Accented text exercises the ToUnicode CMap (Fase 3B.4).
    t.at(72.0, 700.0).show("Olá, açúcar — café, ñandú");
    // Kerning pairs (AV, To, Wa) exercise GPOS shaping (Fase 3D.1).
    t.at(72.0, 660.0).show("AVATAR To Wave");

    doc.save(&path).expect("save");
    println!("wrote {path}");
}
