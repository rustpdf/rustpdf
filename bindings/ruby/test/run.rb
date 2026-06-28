# Smoke test for the RustPdf Ruby binding. Exercises the whole surface,
# including licensing gating. Exits non-zero on any failed assertion.

$LOAD_PATH.unshift(File.expand_path("../lib", __dir__))
require "rustpdf"
require "tmpdir"
require "zlib"

# A minimal valid 1x1 red RGB PNG, built with the stdlib only.
def tiny_png
  chunk = lambda do |tag, data|
    [data.bytesize].pack("N") + tag + data + [Zlib.crc32(tag + data)].pack("N")
  end
  ihdr = [1, 1, 8, 2, 0, 0, 0].pack("NNCCCCC") # 1x1, 8-bit, RGB
  idat = Zlib::Deflate.deflate("\x00\xff\x00\x00".b) # filter byte 0 + one red pixel
  "\x89PNG\r\n\x1a\n".b + chunk.call("IHDR", ihdr) + chunk.call("IDAT", idat) +
    chunk.call("IEND", "".b)
end

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

# 8. Image extraction: embed a PNG, then pull every raster image back out.
img_doc = RustPdf::Document.new
img_doc.add_page
im = img_doc.add_image_png(tiny_png)
img_doc.draw_image(im, 72, 600, 64, 64)
with_img = img_doc.to_bytes
out_dir = Dir.mktmpdir("rustpdf_images_")
n_images = RustPdf.extract_images_to_dir(with_img, out_dir)
check(n_images >= 1, "expected >=1 extracted image, got #{n_images}")
written = Dir.children(out_dir)
check(written.size == n_images, "count #{n_images} != files #{written}")
puts "image extraction ok (#{n_images} image(s) -> #{out_dir})"

# 9. Hyperlinks + bookmarks (Document, Tier 1).
nav = RustPdf::Document.new
nf = nav.add_font_file(font)
nav.add_page.show_text(nf, 20, 72, 760, "Page 1")
nav.add_page.show_text(nf, 20, 72, 760, "Page 2")
nav.link_uri([72, 700, 300, 720], "https://example.com")
nav.link_to_page([72, 670, 300, 690], 1, top: 760.0)
root_bm = RustPdf::Bookmark.new("Cover", 0, top: 800.0)
root_bm.child(RustPdf::Bookmark.new("Details", 1))
nav.add_bookmark(root_bm)
nav_bytes = nav.to_bytes
check(nav_bytes.include?("/Link"), "link annotation present")
check(nav_bytes.include?("/Outlines"), "outline present")
puts "links + bookmarks ok (#{nav_bytes.bytesize} bytes)"

# 10. Factur-X / ZUGFeRD (Document, Tier 2).
fx_xml = "<?xml version=\"1.0\"?><CrossIndustryInvoice/>".b
fx_doc = RustPdf::Document.new
fx_doc.add_page.show_text(fx_doc.add_font_file(font), 12, 72, 700, "Invoice")
fx_doc.facturx(fx_xml, profile: RustPdf::FacturxProfile::EN16931)
fx_bytes = fx_doc.to_bytes
check(fx_bytes.include?("factur-x.xml") || fx_bytes.include?("Factur"), "facturx attachment present")
puts "facturx ok (#{fx_bytes.bytesize} bytes)"

# 11. Form fill / checkbox / radio / choice / flatten / field_names (EditableDoc).
form2 = RustPdf::Document.new
form2.add_page
     .text_field("name", 0, [120, 700, 300, 720], value: "", size: 12)
     .checkbox("agree", 0, [120, 670, 138, 688], false)
     .radio_group("plan", 0, [[[120, 640, 138, 658], "a"], [[160, 640, 178, 658], "b"]])
     .dropdown("country", 0, [120, 610, 300, 630], %w[BR PT])
fed = RustPdf::EditableDoc.load(form2.to_bytes)
names = fed.field_names
check(names.include?("name"), "field_names includes name: #{names}")
check(fed.fill_text_field("name", "Ada"), "fill_text_field found")
check(fed.set_checkbox("agree", true), "set_checkbox found")
check(fed.set_radio("plan", "b"), "set_radio found")
check(fed.set_choice("country", "PT"), "set_choice found")
check(!fed.set_checkbox("nope"), "missing checkbox not found")
fed.flatten_forms
flat = fed.to_bytes
check(!flat.empty?, "flattened bytes")
puts "form fill + flatten + field_names ok (#{names.size} fields)"

# 12. Watermark + redact (EditableDoc, Tier 1/2).
wm_doc = RustPdf::Document.new
wdf = wm_doc.add_font_file(font)
wm_doc.add_page.show_text(wdf, 14, 72, 700, "confidential body text")
wm_ed = RustPdf::EditableDoc.load(wm_doc.to_bytes)
wm_ed.watermark_text("DRAFT", size: 60.0, color: [0.8, 0.1, 0.1], opacity: 0.2, rotation_deg: 45.0)
check(wm_ed.redact(0, [[72, 695, 300, 712]]), "redact page existed")
check(!wm_ed.redact(99, [[0, 0, 10, 10]]), "redact missing page")
wm_bytes = wm_ed.to_bytes
check(!wm_bytes.empty?, "watermarked+redacted bytes")
puts "watermark + redact ok (#{wm_bytes.bytesize} bytes)"

# 13. Convert an existing PDF to PDF/A (EditableDoc, Tier 2).
conv_doc = RustPdf::Document.new
conv_doc.add_page.show_text(conv_doc.add_font_file(font), 12, 72, 700, "plain")
conv_ed = RustPdf::EditableDoc.load(conv_doc.to_bytes)
conv_ed.convert_to_pdfa(RustPdf::Pdfa::A2B)
conv_bytes = conv_ed.to_bytes
check(conv_bytes.include?("pdfaid"), "converted PDF/A metadata present")
puts "convert_to_pdfa ok (#{conv_bytes.bytesize} bytes)"

# 14. Verify signatures on a freshly-signed doc (module-level, Tier 2).
sigs = RustPdf.verify_signatures(signed)
check(sigs.is_a?(Array) && sigs.size >= 1, "expected >=1 signature, got #{sigs.inspect}")
sig = sigs.first
check(sig.key?("sub_filter"), "signature record has sub_filter: #{sig.inspect}")
check(sig.key?("byte_range") && sig["byte_range"].is_a?(Array), "byte_range array")
check(RustPdf.verify_signatures(pdfa).empty?, "unsigned doc has no signatures")
puts "verify_signatures ok (#{sigs.size} signature(s))"

puts "OK: full Ruby binding surface exercised"
