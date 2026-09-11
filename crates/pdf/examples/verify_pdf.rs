//! Print the signature report for a PDF: `cargo run -p pdf --example verify_pdf -- file.pdf`.

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: verify_pdf <file.pdf>");
    let bytes = std::fs::read(&path).expect("read pdf");
    match pdf::verify_signatures(&bytes) {
        Ok(reports) if reports.is_empty() => println!("no signatures"),
        Ok(reports) => {
            for (i, r) in reports.iter().enumerate() {
                println!(
                    "#{i} field={:?} subfilter={}\n   signer={:?}\n   digest_valid={} signature_valid={} covers_whole_doc={} is_valid={}",
                    r.field_name, r.sub_filter, r.signer,
                    r.digest_valid, r.signature_valid, r.covers_whole_document, r.is_valid()
                );
            }
        }
        Err(e) => println!("verify error: {e}"),
    }
}
