require "fiddle"
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
