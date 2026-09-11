# Smoke test for release CI: exercises the whole surface (vector graphics
# → bytes) against the staged cdylib. The full surface is covered by test/run.rb.
# Exits non-zero on any failed assertion. Set RUSTPDF_LIB to the staged cdylib.

$LOAD_PATH.unshift(File.expand_path("../lib", __dir__))
require "rustpdf"

def check(cond, msg)
  return if cond

  warn "ASSERT FAILED: #{msg}"
  exit 1
end

puts "rustpdf version: #{RustPdf.version}"

doc = RustPdf::Document.new
doc.add_page
   .fill_rgb(0.1, 0.2, 0.8)
   .rect(72, 700, 200, 80)
   .fill
bytes = doc.to_bytes

check(!bytes.empty?, "produced bytes")
check(bytes[0, 5] == "%PDF-".b || bytes[0, 5] == "%PDF-", "starts with %PDF-")
check(doc.page_count == 1, "page count")

puts "OK: free Ruby surface exercised (#{bytes.bytesize} bytes)"
