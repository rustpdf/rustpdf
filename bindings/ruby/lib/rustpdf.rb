require "fiddle"
require "json"
require "digest"
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

  # What +y+ means for stamped text (EditableDoc#place_text /
  # #place_paragraph). BASELINE is the historical place_text behavior; TOP
  # hangs the text from +y+ (baseline lands ascent x size below it, legacy layout engines
  # fixed-position layout semantics); BOTTOM rests the descender line on +y+.
  # LINE_TOP / LINE_BOTTOM anchor via the layout line box (its height is also
  # the leading basis, so a single line and a wrapped block agree vertically).
  module VerticalAnchor
    BASELINE = 0
    TOP = 1
    BOTTOM = 2
    LINE_TOP = 3
    LINE_BOTTOM = 4
  end

  # Vertical alignment of the line inside a masked_text box. MIDDLE is the
  # historical cap-height centering; TOP hangs the line from the top edge
  # (top line-alignment in rectangle-based text APIs semantics); BOTTOM rests the descender
  # line on the bottom edge.
  module VerticalAlign
    TOP = 0
    MIDDLE = 1
    BOTTOM = 2
  end

  # Coordinate space of the positioned stamping primitives (EditableDoc).
  # VISIBLE (default) compensates the page /Rotate so coordinates match what
  # a viewer displays; MEDIA is the raw PDF user space (legacy layout engines
  # fixed-position layout/rotation semantics — no /Rotate composition).
  module StampSpace
    VISIBLE = 0
    MEDIA = 1
  end

  # How a rotated EditableDoc#draw_image is anchored. CORNER (default) rotates
  # the image about its own lower-left corner at (x, y); BOUNDING_BOX lands
  # the rotated image's bounding box with its lower-left at (x, y) (legacy layout engines
  # layout semantics).
  module ImageAnchor
    CORNER = 0
    BOUNDING_BOX = 1
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

  # Output PDF version (for set_version / normalize).
  module Version
    V1_4 = 0
    V1_5 = 1
    V1_7 = 2
    V2_0 = 3
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

  # DocMDP certification level applied by the first (certifying) signature.
  module Certify
    NONE = 0                  # not a certifying signature
    LOCKED = 1                # /P 1 — no changes permitted after signing
    FORMS = 2                 # /P 2 — form-filling and signing permitted
    FORMS_AND_ANNOTATIONS = 3 # /P 3 — also annotations permitted
  end

  # A signature-policy identifier (PAdES-EPES / ICP-Brasil AD-RB).
  class SignaturePolicy
    attr_accessor :oid, :hash, :hash_algorithm_oid, :uri

    # +oid+: dotted-decimal policy OID; +hash+: policy document hash bytes
    # (under +hash_algorithm_oid+, nil = SHA-256); +uri+: optional SPURI.
    def initialize(oid:, hash:, hash_algorithm_oid: nil, uri: nil)
      @oid = oid
      @hash = hash
      @hash_algorithm_oid = hash_algorithm_oid
      @uri = uri
    end
  end

  # Options for deferred / external signing (issue #41). The +visible_*+ fields
  # request a visible signature appearance: +visible_rect+ is [x0,y0,x1,y1] in
  # points, +visible_text+ is newline-separated appearance lines, and
  # +visible_image+ is PNG/JPEG bytes of a handwritten-signature image.
  class SigningOptions
    attr_accessor :reason, :location, :name, :pades, :certify, :container_size, :policy,
                  :visible, :visible_page, :visible_rect, :visible_text, :visible_image

    def initialize(reason: nil, location: nil, name: nil, pades: false,
                   certify: Certify::NONE, container_size: 0, policy: nil,
                   visible: false, visible_page: 0, visible_rect: nil,
                   visible_text: nil, visible_image: nil)
      @reason = reason
      @location = location
      @name = name
      @pades = pades
      @certify = certify
      @container_size = container_size
      @policy = policy
      @visible = visible
      @visible_page = visible_page
      @visible_rect = visible_rect
      @visible_text = visible_text
      @visible_image = visible_image
    end
  end

  # A signature field discovered in a PDF (pre-signing inventory). +signed+ is
  # true when the field already carries a signature.
  SignatureField = Struct.new(:name, :signed)

  # One positional text match from #find_text. Coordinates are in PDF points,
  # origin lower-left; +x+/+y+ is the lower-left corner of the box.
  TextHit = Struct.new(:page, :text, :x, :y, :width, :height)

  # A rectangle in PDF user space (points, origin lower-left). Used for a page's
  # MediaBox/CropBox. #width / #height are the (non-negative) extents.
  PdfRect = Struct.new(:x0, :y0, :x1, :y1) do
    def width
      (x1 - x0).abs
    end

    def height
      (y1 - y0).abs
    end
  end

  # Read-only geometry of one page (from #measure_page / #measure_pages). Sizes
  # are in PDF points; +width+/+height+ ignore rotation while +rotated_width+/
  # +rotated_height+ account for it (swapped for 90/270 pages). +media_box+ and
  # +crop_box+ are PdfRect values.
  PageGeometry = Struct.new(:page, :width, :height, :rotation,
                            :rotated_width, :rotated_height, :media_box, :crop_box)

  # A non-mutating summary of a PDF (from #inspect_pdf). +pdfa_level+ is nil when
  # the document is not PDF/A; +encryption+ is the cipher name ("None" when the
  # document is not encrypted).
  PdfOverview = Struct.new(:version, :pdfa_level, :encrypted, :encryption,
                           :requires_password, :page_count)

  # An in-progress two-phase signature: #document holds the placeholder PDF and
  # #bytes the exact bytes the signature covers. Hand #hash to a remote signer,
  # build the CMS container, then call #complete.
  class SigningSession
    # The prepared PDF (with a zero-filled /Contents placeholder).
    attr_reader :document
    # The exact bytes covered by the signature (the two ByteRange segments).
    attr_reader :bytes

    def initialize(document, bytes)
      @document = document
      @bytes = bytes
    end

    # SHA-256 of #bytes — the 32-byte value an HSM signs.
    def hash
      Digest::SHA256.digest(@bytes)
    end

    # Phase 2: complete the signature by embedding a finished DER CMS / PKCS#7
    # +container+, returning the final signed PDF.
    def complete(container)
      RustPdf.complete_signature(@document, container)
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

  # Extract the text of a single 0-based +page_index+ (Unicode via ToUnicode),
  # without building an intermediate one-page document.
  def extract_page_text(pdf, page_index)
    take_bytes { |pp, pn| Native.call("pdf_extract_page_text", pdf, pdf.bytesize, page_index, pp, pn) }
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

  # Find every occurrence of +query+ in +pdf+, returning positional boxes.
  # Returns an Array of TextHit (page, text, x, y, width, height) in PDF points
  # (origin lower-left). +case_sensitive+ defaults to false. An empty Array
  # means no match.
  def find_text(pdf, query, case_sensitive: false)
    js = take_bytes do |pp, pn|
      Native.call("pdf_find_text_json", pdf, pdf.bytesize, query, case_sensitive ? 1 : 0, pp, pn)
    end.force_encoding(Encoding::UTF_8)
    return [] if js.empty?

    JSON.parse(js).map do |h|
      TextHit.new(h["page"], h["text"], h["x"], h["y"], h["width"], h["height"])
    end
  end

  # Read the geometry (size, rotation, MediaBox, CropBox) of every page in +pdf+,
  # in page order, without mutating it. Returns an Array of PageGeometry. Sizes
  # are in PDF points; +rotated_width+/+rotated_height+ swap for 90/270 pages.
  def measure_pages(pdf)
    js = take_bytes { |pp, pn| Native.call("pdf_measure_pages_json", pdf, pdf.bytesize, pp, pn) }
         .force_encoding(Encoding::UTF_8)
    return [] if js.empty?

    rect = lambda do |arr|
      a = Array(arr)
      a.size == 4 ? PdfRect.new(a[0], a[1], a[2], a[3]) : PdfRect.new(0, 0, 0, 0)
    end
    JSON.parse(js).map do |g|
      PageGeometry.new(g["page"], g["width"], g["height"], g["rotation"],
                       g["rotatedWidth"], g["rotatedHeight"],
                       rect.call(g["mediaBox"]), rect.call(g["cropBox"]))
    end
  end

  # Read the geometry of a single 0-based page of +pdf+. Raises IndexError if
  # +index+ is out of range.
  def measure_page(pdf, index)
    pages = measure_pages(pdf)
    raise IndexError, "page index #{index} out of range (#{pages.size} pages)" if index.negative? || index >= pages.size

    pages[index]
  end

  # Inspect +pdf+ without mutating it: PDF version, PDF/A level (if any),
  # encryption posture and page count. Works even on password-protected files
  # (the encryption fields are still reported). Returns a PdfOverview. Named
  # +inspect_pdf+ to avoid shadowing Object#inspect.
  def inspect_pdf(pdf)
    js = take_bytes { |pp, pn| Native.call("pdf_inspect_json", pdf, pdf.bytesize, pp, pn) }
         .force_encoding(Encoding::UTF_8)
    o = JSON.parse(js)
    PdfOverview.new(o["version"], o["pdfaLevel"], o["encrypted"] ? true : false,
                    o["encryption"], o["requiresPassword"] ? true : false, o["pageCount"])
  end

  # Validate every signature in +pdf+. Returns one Hash per signature with keys
  # "field_name", "sub_filter", "signer", "covers_whole_document",
  # "digest_valid", "signature_valid", "is_valid" and "byte_range", plus the
  # richer certificate fields (any may be null): "issuer", "serial_number",
  # "valid_from", "valid_to", "algorithm", "signing_time", "cert_count" and
  # "has_timestamp". An empty array means the document is unsigned.
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

  # ---- deferred / external (HSM) signing — issue #41 ------------------------

  # Model A — remote signer. Sign +pdf+ without handing this library a key: it
  # builds the CMS signed attributes and calls the given block for the raw RSA
  # PKCS#1 v1.5 signature (over SHA-256 of the block's argument), then assembles
  # and embeds the CMS. +cert_der+ is the signer certificate (DER); +chain+ are
  # intermediate certificates (DER), supplied independently of the key. The
  # private key never reaches this library. Returns the signed PDF bytes.
  def sign_with(pdf, cert_der, chain: [], options: nil, &block)
    raise Error, "sign_with requires a block that produces the signature" unless block

    opts_bytes, keep = build_signing_options(options)
    cptrs = chain.map { |c| Fiddle::Pointer[c] }
    cptr_buf = cptrs.map(&:to_i).pack("J*")
    clen_buf = chain.map(&:bytesize).pack("J*")

    signer_error = nil
    closure = Fiddle::Closure::BlockCaller.new(
      Fiddle::TYPE_INT,
      [Fiddle::TYPE_VOIDP, Fiddle::TYPE_VOIDP, Fiddle::TYPE_SIZE_T,
       Fiddle::TYPE_VOIDP, Fiddle::TYPE_SIZE_T, Fiddle::TYPE_VOIDP]
    ) do |_ctx, data, data_len, sig_buf, sig_cap, sig_len|
      begin
        sig = block.call(data[0, data_len]).to_s
        if sig.bytesize > sig_cap
          signer_error = Error.new("signature (#{sig.bytesize} bytes) exceeds buffer capacity #{sig_cap}")
          next 2
        end
        sig_buf[0, sig.bytesize] = sig
        sig_len[0, Native::SIZEOF_SZ] = [sig.bytesize].pack("J")
        0
      rescue StandardError => e
        signer_error = e
        1
      end
    end

    begin
      result = take_bytes do |pp, pn|
        Native.call("pdf_sign_with", pdf, pdf.bytesize, cert_der, cert_der.bytesize,
                    cptr_buf, clen_buf, chain.size, opts_bytes, closure, Fiddle::NULL, pp, pn)
      end
    rescue Error
      raise signer_error if signer_error

      raise
    end
    # keep the callback, struct and per-cert pointers alive until the call returned
    cptrs.clear
    keep.clear
    closure.to_i # touch to keep it referenced past the native call
    result
  end

  # Model B — two-phase signing, phase 1. Prepare +pdf+ for deferred signing:
  # returns a SigningSession whose #hash you send to a remote HSM. Build the CMS
  # container, then call SigningSession#complete (or .complete_signature). The
  # key never reaches this library.
  def begin_signing(pdf, options: nil)
    opts_bytes, keep = build_signing_options(options)
    doc_p = Fiddle::Pointer.malloc(Native::SIZEOF_SZ, Fiddle::RUBY_FREE)
    doc_n = Fiddle::Pointer.malloc(Native::SIZEOF_SZ, Fiddle::RUBY_FREE)
    tbs_p = Fiddle::Pointer.malloc(Native::SIZEOF_SZ, Fiddle::RUBY_FREE)
    tbs_n = Fiddle::Pointer.malloc(Native::SIZEOF_SZ, Fiddle::RUBY_FREE)
    check(Native.call("pdf_sign_begin", pdf, pdf.bytesize, opts_bytes, doc_p, doc_n, tbs_p, tbs_n))
    keep.clear
    SigningSession.new(read_buffer(doc_p, doc_n), read_buffer(tbs_p, tbs_n))
  end

  # Model B — phase 2. Embed a complete DER CMS / PKCS#7 +container+ into a
  # prepared +document+ (from #begin_signing), returning the final signed PDF.
  def complete_signature(document, container)
    take_bytes do |pp, pn|
      Native.call("pdf_sign_complete", document, document.bytesize, container, container.bytesize, pp, pn)
    end
  end

  # ---- network TSA (AD-RT) document timestamp — issue #41 -------------------

  # Phase 1 of a network-TSA document timestamp (/DocTimeStamp). Prepares +pdf+
  # and returns [document_bytes, tbs_bytes]: +tbs_bytes+ are the bytes whose
  # SHA-256 forms the RFC 3161 message imprint. Build a TimeStampReq with
  # #timestamp_request, POST it to the TSA, extract the token with
  # #timestamp_token_from_response, then embed it via #complete_signature.
  def begin_timestamp(pdf)
    doc_p = Fiddle::Pointer.malloc(Native::SIZEOF_SZ, Fiddle::RUBY_FREE)
    doc_n = Fiddle::Pointer.malloc(Native::SIZEOF_SZ, Fiddle::RUBY_FREE)
    tbs_p = Fiddle::Pointer.malloc(Native::SIZEOF_SZ, Fiddle::RUBY_FREE)
    tbs_n = Fiddle::Pointer.malloc(Native::SIZEOF_SZ, Fiddle::RUBY_FREE)
    check(Native.call("pdf_timestamp_begin", pdf, pdf.bytesize, doc_p, doc_n, tbs_p, tbs_n))
    [read_buffer(doc_p, doc_n), read_buffer(tbs_p, tbs_n)]
  end

  # Build an RFC 3161 TimeStampReq (DER) for +imprint+ (the SHA-256 of the bytes
  # to timestamp, e.g. the +tbs_bytes+ from #begin_timestamp). +nonce+ is
  # optional; +cert_req+ asks the TSA to embed its certificate. Returns the
  # request bytes to POST to the TSA.
  def timestamp_request(imprint, nonce: nil, cert_req: true)
    take_bytes do |pp, pn|
      Native.call("pdf_timestamp_request", imprint, imprint.bytesize,
                  nonce, nonce ? nonce.bytesize : 0, cert_req ? 1 : 0, pp, pn)
    end
  end

  # Extract the TimeStampToken (a CMS ContentInfo) from a TSA's RFC 3161
  # TimeStampResp +response+ bytes. The returned token is embedded via
  # #complete_signature(document, token).
  def timestamp_token_from_response(response)
    take_bytes do |pp, pn|
      Native.call("pdf_timestamp_token_from_response", response, response.bytesize, pp, pn)
    end
  end

  # List the signature fields in +pdf+ (detect existing signatures before
  # signing). Returns an Array of SignatureField; an empty Array means there are
  # no signature fields.
  def list_signatures(pdf)
    text = take_bytes { |pp, pn| Native.call("pdf_list_signatures", pdf, pdf.bytesize, pp, pn) }
           .force_encoding(Encoding::UTF_8)
    fields = []
    text.each_line do |line|
      line = line.chomp
      tab = line.index("\t")
      next unless tab

      fields << SignatureField.new(line[(tab + 1)..-1], line[0...tab] == "1")
    end
    fields
  end

  # ---- internal helpers (used by Document/EditableDoc) ----------------------

  # Marshal a SigningOptions (or nil) into the C PdfSigningOptions struct bytes,
  # returning [packed_struct, keepalive] where +keepalive+ holds the Fiddle
  # pointers backing the struct's string/byte fields (keep it referenced until
  # the native call returns). 64-bit layout: 3 ptr, 2 int (8 bytes together),
  # size_t, ptr, ptr, size_t, ptr, ptr, then the visible-signature tail:
  # int (+ 4 pad), size_t, double[4], ptr, ptr, size_t.
  def build_signing_options(options)
    keep = []
    cstr = lambda do |s|
      return 0 if s.nil?

      p = Fiddle::Pointer[s.to_s]
      keep << p
      p.to_i
    end

    reason   = cstr.call(options&.reason)
    location = cstr.call(options&.location)
    name     = cstr.call(options&.name)
    pades    = options&.pades ? 1 : 0
    cert     = (options&.certify || Certify::NONE).to_i
    est      = (options&.container_size || 0).to_i

    policy_oid = 0
    policy_hash = 0
    policy_hash_len = 0
    policy_alg = 0
    policy_uri = 0
    if (pol = options&.policy)
      policy_oid = cstr.call(pol.oid)
      if pol.hash && !pol.hash.empty?
        hp = Fiddle::Pointer[pol.hash]
        keep << hp
        policy_hash = hp.to_i
        policy_hash_len = pol.hash.bytesize
      end
      policy_alg = cstr.call(pol.hash_algorithm_oid)
      policy_uri = cstr.call(pol.uri)
    end

    visible  = options&.visible ? 1 : 0
    vis_page = (options&.visible_page || 0).to_i
    vis_rect = Array(options&.visible_rect || [0.0, 0.0, 0.0, 0.0]).map(&:to_f)[0, 4]
    vis_rect += [0.0] * (4 - vis_rect.size)
    vis_text = cstr.call(options&.visible_text)
    vis_image = 0
    vis_image_len = 0
    if (img = options&.visible_image) && !img.empty?
      ip = Fiddle::Pointer[img]
      keep << ip
      vis_image = ip.to_i
      vis_image_len = img.bytesize
    end

    bytes = [reason, location, name].pack("J3") +
            [pades, cert].pack("l2") +
            [est, policy_oid, policy_hash, policy_hash_len, policy_alg, policy_uri].pack("J6") +
            [visible].pack("l") + "\x00\x00\x00\x00".b +          # int visible + 4-byte pad
            [vis_page].pack("J") + vis_rect.pack("d4") +
            [vis_text, vis_image, vis_image_len].pack("J3")
    [bytes, keep]
  end

  # Read an out-buffer (pointer-to-pointer +pp+, pointer-to-len +pn+) into a
  # binary String, freeing the native buffer.
  def read_buffer(pp, pn)
    len = pn[0, Native::SIZEOF_SZ].unpack1("J")
    return "".b if len.zero?

    dptr = pp.ptr
    bytes = dptr[0, len]
    Native.call("pdf_buffer_free", dptr, len)
    bytes
  end

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
    read_buffer(pp, pn)
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
