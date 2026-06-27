# Smoke test for the RustPdf Ruby binding. Exercises the whole surface,
# including licensing gating. Exits non-zero on any failed assertion.

$LOAD_PATH.unshift(File.expand_path("../lib", __dir__))
require "rustpdf"

def repo_root
  dir = __dir__
  12.times do
    return dir if File.file?(File.join(dir, "Cargo.toml"))

    parent = File.dirname(dir)
    break if parent == dir

    dir = parent
  end
  warn "could not locate repo root"
  exit 2
end

def check(cond, msg)
  return if cond

  warn "ASSERT FAILED: #{msg}"
  exit 1
end

root = repo_root
font = File.join(root, "assets", "fonts", "Roboto-Regular.ttf")
dev_license = File.read(File.join(root, "crates", "license", "fixtures", "dev_license.txt")).strip

puts "rustpdf version: #{RustPdf.version}"

# 1. Corporate features blocked without a license.
ENV.delete("RUSTPDF_LICENSE")
blocked = false
begin
  d = RustPdf::Document.new
  d.pdfa
  d.add_page
  d.to_bytes
rescue RustPdf::Error
  blocked = true
end
check(blocked, "PDF/A must be blocked without a license")

RustPdf.activate_license(dev_license)
puts "license activated"

# 2. Tagged PDF/A-2a with a font, heading and justified paragraph.
doc = RustPdf::Document.new
doc.pdfa(RustPdf::Pdfa::A2A).info(title: "Olá", author: "rustpdf")
f = doc.add_font_file(font)
doc.add_page
   .show_text(f, 20, 72, 760, "Título", heading_level: 1)
   .paragraph(f, 12, 72, 720, 450, "Um parágrafo. " * 8, align: RustPdf::Align::JUSTIFY)
pdfa = doc.to_bytes
check(!pdfa.empty?, "pdfa bytes")
text = RustPdf.extract_text(pdfa)
check(text.include?("Título"), "extracted text: #{text}")
puts "built PDF/A-2a (#{pdfa.bytesize} bytes); extracted ok"

# 3. Incremental update preserves the original prefix.
ed = RustPdf::EditableDoc.load(pdfa)
check(ed.page_count == 1, "page count")
ed.set_info("Subject", "via FFI")
check(ed.get_info("Subject") == "via FFI", "get_info")
incr = ed.to_bytes_incremental(pdfa)
check(incr.start_with?(pdfa), "incremental preserves original")
puts "incremental update ok (#{incr.bytesize} bytes)"

# 4. Merge + optimize.
a = RustPdf::EditableDoc.load(pdfa)
b = RustPdf::EditableDoc.load(pdfa)
a.merge(b).optimize
merged = RustPdf::EditableDoc.load(a.to_bytes)
check(merged.page_count == 2, "merged page count")
puts "merge + optimize ok"

# 5. AcroForm with every field type.
form = RustPdf::Document.new
form.add_page
    .text_field("city", 0, [120, 700, 300, 720], value: "SP", size: 12)
    .checkbox("ok", 0, [120, 670, 138, 688], true)
    .radio_group("plan", 0, [[[120, 640, 138, 658], "a"], [[160, 640, 178, 658], "b"]], selected: 1)
    .dropdown("country", 0, [120, 610, 300, 630], %w[BR PT], selected: 0, size: 12)
fb = form.to_bytes
check(fb.include?("/AcroForm"), "AcroForm present")
puts "forms ok"

# 6. Encryption (AES-256) round-trips.
plain_doc = RustPdf::Document.new
pf = plain_doc.add_font_file(font)
plain = plain_doc.add_page.show_text(pf, 14, 72, 700, "segredo").to_bytes
enc_ed = RustPdf::EditableDoc.load(plain)
enc_ed.encrypt(method: RustPdf::Cipher::AES256, owner: "owner")
enc = enc_ed.to_bytes
check(enc.include?("/AESV3"), "AES-256 marker")
check(RustPdf.extract_text(enc).include?("segredo"), "decrypted text")
puts "encryption ok"

# 7. Digital signature (PKCS#7 / PAdES) with the committed test key.
fx = File.join(root, "crates", "pdf", "tests", "fixtures")
key = File.binread(File.join(fx, "signer_key.pk8"))
cert = File.binread(File.join(fx, "signer_cert.der"))
signed = RustPdf.sign(plain, key, cert, reason: "Aprovado", pades: true)
check(signed.include?("/ByteRange"), "signature ByteRange")
puts "signed ok (#{signed.bytesize} bytes)"

puts "OK: full Ruby binding surface exercised"
