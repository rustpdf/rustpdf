require "fiddle"
require "json"
require_relative "rustpdf/native"

# Idiomatic Ruby binding for the rust-pdf core over its C ABI (libpdf_ffi),
# using the built-in Fiddle stdlib. Covers the whole product surface: vector
# graphics, fonts/text, paragraphs, images, PDF/A (1b-3a), tagged/accessible
# output, attachments, AcroForm fields, manipulation, text extraction,
# encryption and digital signatures, plus feature licensing.
module RustPdf
  # Raised when a native call returns a non-zero PdfStatus.
  class Error < StandardError
    attr_reader :status

    def initialize(message, status = 0)
      @status = status
      super(status.zero? ? message : "PdfStatus=#{status}: #{message}")
    end
  end

  # PDF/A conformance levels.
  module Pdfa
    A1B = 0
    A2B = 1
    A2A = 2
    A3B = 3
    A3A = 4
    # PDF/A-4 (ISO 19005-4), based on PDF 2.0.
    A4 = 5
    A4E = 6
    A4F = 7
  end

  # Paragraph alignment.
  module Align
    LEFT = 0
    RIGHT = 1
    CENTER = 2
    JUSTIFY = 3
  end

  # Embedded-file relationship (PDF/A-3).
  module Relationship
    SOURCE = 0
    DATA = 1
    ALTERNATIVE = 2
    SUPPLEMENT = 3
    UNSPECIFIED = 4
  end

  # Encryption ciphers.
  module Cipher
    RC4 = 0
    AES128 = 1
    AES256 = 2
  end

  # ZUGFeRD / Factur-X conformance profiles.
  module FacturxProfile
    MINIMUM = 0
    BASIC_WL = 1
    BASIC = 2
    EN16931 = 3
    EXTENDED = 4
  end

  # A document outline (bookmark) entry. Nest with #child to build a tree.
  # Each #add_bookmark call on a Document appends one root tree (pre-order
  # flattened into parallel arrays).
  class Bookmark
    attr_accessor :title, :page, :top, :children

    def initialize(title, page, top: nil, children: nil)
      @title = title
      @page = page
      @top = top
      @children = children || []
    end

    # Append a child bookmark; returns self for chaining.
    def child(bookmark)
      @children << bookmark
      self
    end

    # Pre-order flatten into +out+ as [level, title, page, top] tuples.
    def flatten_into(level, out)
      out << [level, title, page, top]
      children.each { |c| c.flatten_into(level + 1, out) }
      out
    end
  end

  module_function

  # Native library version string.
  def version
    Native.call("pdf_version").to_s
  end

  # Activate a license token (unlocks PDF/A, signing, encryption,
  # accessibility). Tokens may also be supplied via the RUSTPDF_LICENSE /
  # RUSTPDF_LICENSE_FILE environment variables (auto-activated).
  def activate_license(token)
    check(Native.call("pdf_activate_license", token))
  end

  # Extract a document's text (Unicode via ToUnicode).
  def extract_text(pdf)
    take_bytes { |pp, pn| Native.call("pdf_extract_text", pdf, pdf.bytesize, pp, pn) }
      .force_encoding(Encoding::UTF_8)
  end

  # Extract every raster image into +dir+ (JPEG verbatim as .jpg, others as
  # .png; files named page{N}_{name}.{ext}). Returns the number written.
  def extract_images_to_dir(pdf, dir)
    count = Fiddle::Pointer.malloc(Native::SIZEOF_SZ, Fiddle::RUBY_FREE)
    check(Native.call("pdf_extract_images_to_dir", pdf, pdf.bytesize, dir, count))
    count[0, Native::SIZEOF_SZ].unpack1("J")
  end

  # Render page +page+ (0-based) of +pdf+ to a PNG image at +dpi+
  # dots-per-inch. Page rendering is a licensed Pro feature: raises unless a
  # license granting it is active.
  def render_page_to_png(pdf, page = 0, dpi = 150.0)
    take_bytes { |pp, pn| Native.call("pdf_render_page_to_png", pdf, pdf.bytesize, page, dpi.to_f, pp, pn) }
  end

  # Number of pages in +pdf+ (free — no license required).
  def page_count(pdf)
    count = Fiddle::Pointer.malloc(Native::SIZEOF_SZ, Fiddle::RUBY_FREE)
    check(Native.call("pdf_page_count", pdf, pdf.bytesize, count))
    count[0, Native::SIZEOF_SZ].unpack1("J")
  end

  # Validate every signature in +pdf+. Returns one Hash per signature with keys
  # "field_name", "sub_filter", "signer", "covers_whole_document",
  # "digest_valid", "signature_valid", "is_valid" and "byte_range". An empty
  # array means the document is unsigned.
  def verify_signatures(pdf)
    js = take_bytes { |pp, pn| Native.call("pdf_verify_signatures_json", pdf, pdf.bytesize, pp, pn) }
         .force_encoding(Encoding::UTF_8)
    js.empty? ? [] : JSON.parse(js)
  end

  # Sign a PDF (PKCS#7 detached, incremental update). Requires a license.
  def sign(pdf, key_der, cert_der, reason: nil, location: nil, name: nil, pades: false)
    take_bytes do |pp, pn|
      Native.call("pdf_sign", pdf, pdf.bytesize, key_der, key_der.bytesize,
                  cert_der, cert_der.bytesize, reason, location, name, pades ? 1 : 0, pp, pn)
    end
  end

  # Append a document timestamp (/DocTimeStamp, PAdES-B-LTA).
  def timestamp(pdf, tsa_key_der, tsa_cert_der, date: nil)
    take_bytes do |pp, pn|
      Native.call("pdf_timestamp", pdf, pdf.bytesize, tsa_key_der, tsa_key_der.bytesize,
                  tsa_cert_der, tsa_cert_der.bytesize, date, pp, pn)
    end
  end

  # Append a Document Security Store (/DSS, PAdES-B-LT).
  def add_dss(pdf, certs: [], crls: [])
    cptrs = certs.map { |c| Fiddle::Pointer[c] }
    rptrs = crls.map { |c| Fiddle::Pointer[c] }
    cptr_buf = cptrs.map(&:to_i).pack("J*")
    clen_buf = certs.map(&:bytesize).pack("J*")
    rptr_buf = rptrs.map(&:to_i).pack("J*")
    rlen_buf = crls.map(&:bytesize).pack("J*")
    result = take_bytes do |pp, pn|
      Native.call("pdf_add_dss", pdf, pdf.bytesize, cptr_buf, clen_buf, certs.size,
                  rptr_buf, rlen_buf, crls.size, pp, pn)
    end
    # keep the per-item pointers alive until the call has returned
    cptrs.clear
    rptrs.clear
    result
  end

  # ---- internal helpers (used by Document/EditableDoc) ----------------------

  def last_error
    p = Native.call("pdf_last_error_message")
    p.null? ? "unknown error" : p.to_s
  end

  def check(status)
    raise Error.new(last_error, status) unless status.zero?
  end

  # Run an out-buffer producer { |out_ptr, out_len| status } and return the
  # produced bytes, always freeing the native buffer.
  def take_bytes
    pp = Fiddle::Pointer.malloc(Native::SIZEOF_SZ, Fiddle::RUBY_FREE)
    pn = Fiddle::Pointer.malloc(Native::SIZEOF_SZ, Fiddle::RUBY_FREE)
    check(yield(pp, pn))
    len = pn[0, Native::SIZEOF_SZ].unpack1("J")
    return "".b if len.zero?

    dptr = pp.ptr
    bytes = dptr[0, len]
    Native.call("pdf_buffer_free", dptr, len)
    bytes
  end

  # Run a producer { |out_int| status } and return the written int.
  def out_int
    buf = Fiddle::Pointer.malloc(Native::SIZEOF_INT, Fiddle::RUBY_FREE)
    check(yield(buf))
    buf[0, Native::SIZEOF_INT].unpack1("i!")
  end
end

require_relative "rustpdf/document"
require_relative "rustpdf/editable_doc"
