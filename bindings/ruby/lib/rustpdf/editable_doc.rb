module RustPdf
  # An existing PDF loaded for manipulation. Call #close (or rely on GC) to free.
  class EditableDoc
    # Load a PDF from bytes (optionally with a password).
    def self.load(data, password: nil)
      ptr = if password
              Native.call("pdf_editable_load_password", data, data.bytesize, password)
            else
              Native.call("pdf_editable_load", data, data.bytesize)
            end
      raise Error, RustPdf.last_error if ptr.null?

      new(ptr)
    end

    def self.load_file(path, password: nil)
      load(File.binread(path), password: password)
    end

    def initialize(ptr)
      @ptr = ptr
    end
    private_class_method :new

    def close
      return if @ptr.nil? || @ptr.null?

      Native.call("pdf_editable_free", @ptr)
      @ptr = nil
    end

    def page_count
      Native.call("pdf_editable_page_count", ptr)
    end

    def merge(other)
      RustPdf.check(Native.call("pdf_editable_merge", ptr, other.send(:ptr)))
      self
    end

    def rotate_page(index, degrees)
      RustPdf.check(Native.call("pdf_editable_rotate_page", ptr, index, degrees))
      self
    end

    def delete_page(index)
      RustPdf.check(Native.call("pdf_editable_delete_page", ptr, index))
      self
    end

    def reorder_pages(order)
      buf = order.pack("J*")
      RustPdf.check(Native.call("pdf_editable_reorder_pages", ptr, buf, order.size))
      self
    end

    def extract_pages(indices)
      buf = indices.pack("J*")
      out = Fiddle::Pointer.malloc(Native::SIZEOF_SZ, Fiddle::RUBY_FREE)
      RustPdf.check(Native.call("pdf_editable_extract_pages", ptr, buf, indices.size, out))
      EditableDoc.send(:new, out.ptr)
    end

    def info=(pair)
      key, value = pair
      RustPdf.check(Native.call("pdf_editable_set_info", ptr, key, value))
    end

    def set_info(key, value)
      RustPdf.check(Native.call("pdf_editable_set_info", ptr, key, value))
      self
    end

    def get_info(key)
      p = ptr
      RustPdf.take_bytes { |pp, pn| Native.call("pdf_editable_get_info", p, key, pp, pn) }
             .force_encoding(Encoding::UTF_8)
    end

    def set_xmp(xml)
      RustPdf.check(Native.call("pdf_editable_set_xmp", ptr, xml, xml.bytesize))
      self
    end

    def overlay_page(index, content)
      RustPdf.check(Native.call("pdf_editable_overlay_page", ptr, index, content, content.bytesize))
      self
    end

    # Returns whether the field existed.
    def fill_text_field(name, value)
      found = RustPdf.out_int { |buf| Native.call("pdf_editable_fill_text_field", ptr, name, value, buf) }
      found != 0
    end

    # ---- form fill + flatten + watermark (Tier 1) ---------------------------

    # Set a checkbox by field name. Returns whether the field existed.
    def set_checkbox(name, checked = true)
      found = RustPdf.out_int { |buf| Native.call("pdf_editable_set_checkbox", ptr, name, checked ? 1 : 0, buf) }
      found != 0
    end

    # Select a radio button by field name + export value. Returns whether found.
    def set_radio(name, export_value)
      found = RustPdf.out_int { |buf| Native.call("pdf_editable_set_radio", ptr, name, export_value, buf) }
      found != 0
    end

    # Set a choice (dropdown/list) value by field name. Returns whether found.
    def set_choice(name, value)
      found = RustPdf.out_int { |buf| Native.call("pdf_editable_set_choice", ptr, name, value, buf) }
      found != 0
    end

    # Flatten all AcroForm fields into page content (removes interactivity).
    def flatten_forms
      RustPdf.check(Native.call("pdf_editable_flatten_forms", ptr))
      self
    end

    # Returns the list of AcroForm field names.
    def field_names
      p = ptr
      text = RustPdf.take_bytes { |pp, pn| Native.call("pdf_editable_field_names", p, pp, pn) }
                    .force_encoding(Encoding::UTF_8)
      text.split("\n").reject(&:empty?)
    end

    # Stamp a diagonal text watermark across every page. When
    # +opaque_background+ is true, the text is drawn over an opaque white box
    # (otherwise it is blended into the page content).
    def watermark_text(text, size: 64.0, color: [0.5, 0.5, 0.5], opacity: 0.30,
                       rotation_deg: 45.0, opaque_background: false)
      r, g, b = color
      RustPdf.check(Native.call("pdf_editable_watermark_text", ptr, text, size, r, g, b,
                                opacity, rotation_deg, opaque_background ? 1 : 0))
      self
    end

    # Stamp an image watermark (from a file) across every page, rotated
    # +rotation_deg+ degrees counter-clockwise.
    def watermark_image_file(path, width, height, opacity: 0.30, rotation_deg: 0.0)
      RustPdf.check(Native.call("pdf_editable_watermark_image_file", ptr, path, width, height,
                                opacity, rotation_deg))
      self
    end

    # ---- positioned drawing primitives (issue #45 P1) -----------------------

    # Paint a filled rectangle at (+x+, +y+) sized +width+ x +height+ on page
    # +page_index+ (0-based), in RGB +color+ (each 0..1, default opaque white) at
    # +opacity+ (0..1). Coordinates are in the page's VISIBLE space (origin
    # lower-left, y up), regardless of the page's /Rotate. The common use is
    # masking a placeholder with an opaque white box. Returns whether the page
    # existed.
    def fill_rect(page_index, x, y, width, height, color = [1.0, 1.0, 1.0], opacity = 1.0)
      r, g, b = color
      found = RustPdf.out_int do |buf|
        Native.call("pdf_editable_fill_rect", ptr, page_index, x.to_f, y.to_f,
                    width.to_f, height.to_f, r.to_f, g.to_f, b.to_f, opacity.to_f, buf)
      end
      found != 0
    end

    # Register a TrueType/OpenType font (from a file path) for text stamping;
    # returns a font id usable with the +font_id+ keyword of #place_text /
    # #masked_text / #place_paragraph. The font is embedded as a subset —
    # stamped text renders with the real font's glyphs and metrics.
    def add_font_file(path)
      RustPdf.out_int { |buf| Native.call("pdf_editable_add_font_file", ptr, path, buf) }
    end

    # Register a stamping font from raw TrueType/OpenType bytes. See
    # #add_font_file.
    def add_font(data)
      RustPdf.out_int { |buf| Native.call("pdf_editable_add_font", ptr, data, data.bytesize, buf) }
    end

    # Draw a line of positioned +text+ anchored at (+x+, +y+) on page
    # +page_index+ (0-based), using standard Helvetica (or an embedded font —
    # pass +font_id+ from #add_font_file / #add_font) at +size+ points in RGB
    # +color+ (each 0..1, default black). +rotation_deg+ rotates the text
    # counter-clockwise about its anchor (match the page rotation to follow a
    # rotated page). +align+ (RustPdf::Align, default LEFT) shifts the start
    # point along the baseline direction by the text width for RIGHT/CENTER
    # alignment. +anchor+ (RustPdf::VerticalAnchor) says what +y+ means:
    # BASELINE (default, historical behavior), TOP (text hangs from +y+ —
    # baseline lands ascent x size below it, legacy fixed-position layout), BOTTOM
    # (descender line rests on +y+), or LINE_TOP / LINE_BOTTOM (legacy layout engines line
    # box). Coordinates are in the page's VISIBLE space (origin lower-left,
    # y up) unless #stamp_space= chose MEDIA. Returns whether the page (and
    # font) existed.
    def place_text(page_index, x, y, text, size = 12.0, color = [0.0, 0.0, 0.0], rotation_deg = 0.0,
                   align: Align::LEFT, font_id: -1, anchor: VerticalAnchor::BASELINE)
      r, g, b = color
      found = RustPdf.out_int do |buf|
        Native.call("pdf_editable_place_text_anchored", ptr, page_index, x.to_f, y.to_f, text,
                    size.to_f, r.to_f, g.to_f, b.to_f, rotation_deg.to_f, align, anchor, font_id, buf)
      end
      found != 0
    end

    # Choose the coordinate space of the positioned stamping primitives
    # (#fill_rect, #place_text, #masked_text, #place_paragraph, #draw_image)
    # for subsequent calls. StampSpace::VISIBLE (default) keeps the historical
    # behavior — coordinates in the page's displayed space, compensating
    # /Rotate. StampSpace::MEDIA interprets coordinates and +rotation_deg+ in
    # the raw PDF user space (legacy layout semantics), never composing with the
    # page's /Rotate — use it to reproduce legacy-engine placement on rotated/scanned
    # pages. Watermarks and redaction are unaffected.
    def stamp_space=(space)
      RustPdf.check(Native.call("pdf_editable_set_stamp_space", ptr, space))
      @stamp_space = space
    end

    # The current stamping coordinate space (see #stamp_space=).
    def stamp_space
      @stamp_space || StampSpace::VISIBLE
    end

    # Chainable form of #stamp_space=.
    def set_stamp_space(space)
      self.stamp_space = space
      self
    end

    # Stamp a paragraph with automatic word wrapping on page +page_index+:
    # +text+ is broken into lines that fit +width+ points (greedy, by word;
    # "\n" forces a break) and drawn downward from (+x+, +y+). +anchor+
    # (RustPdf::VerticalAnchor) says what +y+ means for the block: TOP
    # (default) — top of the box, first baseline ascent x size below +y+
    # (legacy fixed-position layout); BASELINE — the first line's baseline; BOTTOM /
    # LINE_BOTTOM — bottom-pinned: the block's bottom rests on +y+ and grows
    # upward by its real content height. +align+ lays lines out inside
    # +[x, x+width]+ (JUSTIFY stretches the word gaps of every line but the
    # last of each paragraph). +max_height+ (points, nil = unlimited) truncates
    # overflowing lines (a ceiling for the bottom-pinned anchors — the last
    # lines stay pinned). +line_height+ scales the default 1.2 x size leading.
    # Pass +font_id+ from #add_font_file / #add_font to wrap and draw with an
    # embedded font (its real metrics drive the break points); -1 uses the
    # built-in Helvetica. +rotation_deg+ rotates the laid-out block
    # counter-clockwise about the anchor. Returns whether the page (and font)
    # existed and the box was valid.
    def place_paragraph(page_index, x, y, width, text, size: 12.0, color: [0.0, 0.0, 0.0],
                        align: Align::LEFT, font_id: -1, max_height: nil, line_height: 1.0,
                        anchor: VerticalAnchor::TOP, rotation_deg: 0.0)
      found, = paragraph_anchored(page_index, x, y, width, text, size, color, align,
                                  font_id, max_height, line_height, anchor, rotation_deg)
      found
    end

    # Like #place_paragraph but returns a Hash with the number of +:lines+
    # drawn and the consumed block +:height+ in points (top of the first drawn
    # line's box to the bottom of the last one's; 0 when nothing fit) — stack
    # blocks without re-measuring.
    def place_paragraph_measured(page_index, x, y, width, text, size: 12.0, color: [0.0, 0.0, 0.0],
                                 align: Align::LEFT, font_id: -1, max_height: nil, line_height: 1.0,
                                 anchor: VerticalAnchor::TOP, rotation_deg: 0.0)
      _, lines, height = paragraph_anchored(page_index, x, y, width, text, size, color, align,
                                            font_id, max_height, line_height, anchor, rotation_deg)
      { lines: lines, height: height }
    end

    # Mask a placeholder: fill an opaque background box +[x, y, x+width,
    # y+height]+ in +bg_color+ (each 0..1, default white), then write +text+
    # over it using standard Helvetica (or an embedded font — pass +font_id+
    # from #add_font_file / #add_font) at +size+ points in +text_color+ (each
    # 0..1, default black), horizontally aligned per +align+ (RustPdf::Align,
    # default LEFT). +valign+ (RustPdf::VerticalAlign) controls the vertical
    # alignment of the line inside the box: MIDDLE (default, historical
    # cap-height centering), TOP (line hangs from the top edge — legacy PDF libraries
    # top line-alignment semantics) or BOTTOM (descender line rests on the
    # bottom edge). +padding+ is the horizontal edge inset (points) for
    # LEFT/RIGHT alignment: text starts at +x + padding+ (or ends at
    # +x + width - padding+); nil keeps the historical
    # min(0.15 x size, width / 4), 0 starts flush with the box edge like
    # rectangle-based DrawString APIs. Coordinates are in the page's VISIBLE space
    # (origin lower-left, y up). Returns whether the page (and font) existed.
    def masked_text(page_index, x, y, width, height, text, size = 12.0,
                    text_color = [0.0, 0.0, 0.0], bg_color = [1.0, 1.0, 1.0],
                    align: Align::LEFT, font_id: -1, valign: VerticalAlign::MIDDLE, padding: nil)
      tr, tg, tb = text_color
      br, bg, bb = bg_color
      found = RustPdf.out_int do |buf|
        Native.call("pdf_editable_masked_text_pad", ptr, page_index, x.to_f, y.to_f,
                    width.to_f, height.to_f, text, size.to_f,
                    tr.to_f, tg.to_f, tb.to_f, br.to_f, bg.to_f, bb.to_f, align, valign,
                    (padding || -1.0).to_f, font_id, buf)
      end
      found != 0
    end

    # Stamp an +image+ (PNG or JPEG bytes; the core dispatches on the signature)
    # onto page +page_index+ (0-based), with its lower-left corner at (+x+, +y+),
    # scaled to +width+ x +height+ points and rotated +rotation_deg+ degrees
    # counter-clockwise. +anchor+ (RustPdf::ImageAnchor) controls how a rotated
    # image is anchored: CORNER (default) rotates the image about its own
    # lower-left corner at (x, y); BOUNDING_BOX lands the rotated image's
    # bounding box with its lower-left at (x, y) (bounding-box layout semantics —
    # e.g. a 90-degree image occupies [x, x+height] x [y, y+width]).
    # Coordinates are in the page's VISIBLE space (origin lower-left, y up),
    # regardless of the page's /Rotate. Returns whether the page existed.
    def draw_image(page_index, image, x, y, width, height, rotation_deg = 0.0,
                   anchor: ImageAnchor::CORNER)
      found = RustPdf.out_int do |buf|
        Native.call("pdf_editable_draw_image_anchored", ptr, page_index, image, image.bytesize,
                    x.to_f, y.to_f, width.to_f, height.to_f, rotation_deg.to_f, anchor, buf)
      end
      found != 0
    end

    # ---- redaction + PDF/A conversion (Tier 2) ------------------------------

    # Black out rectangles on a page. rects = [[x0,y0,x1,y1], ...].
    # Returns whether the page existed.
    def redact(page_index, rects)
      flat = rects.flatten.pack("d*")
      found = RustPdf.out_int { |buf| Native.call("pdf_editable_redact", ptr, page_index, flat, rects.size, buf) }
      found != 0
    end

    # Convert the document to PDF/A (B-levels only: A1B=0, A2B=1, A3B=3).
    def convert_to_pdfa(level = Pdfa::A2B)
      RustPdf.check(Native.call("pdf_editable_convert_to_pdfa", ptr, level))
      self
    end

    # ---- version normalization (issue #41 P1) -------------------------------

    # Set the output PDF version (downgrade/normalize). +version+ uses the
    # RustPdf::Version codes (V1_4=0, V1_5=1, V1_7=2, V2_0=3).
    def set_version(version)
      RustPdf.check(Native.call("pdf_editable_set_version", ptr, version))
      self
    end

    # Strip PDF/A conformance (OutputIntents, XMP pdfaid, /Version) so the file
    # is a plain PDF.
    def strip_pdfa
      RustPdf.check(Native.call("pdf_editable_strip_pdfa", ptr))
      self
    end

    # Normalize to a plain PDF at +version+ (strip PDF/A + set version). Codes as
    # in #set_version.
    def normalize(version = Version::V1_7)
      RustPdf.check(Native.call("pdf_editable_normalize", ptr, version))
      self
    end

    def optimize
      RustPdf.check(Native.call("pdf_editable_optimize", ptr))
      self
    end

    def compact(on = true)
      RustPdf.check(Native.call("pdf_editable_compact", ptr, on ? 1 : 0))
      self
    end

    # Encrypt on save.
    def encrypt(method: Cipher::AES256, user: "", owner: "", read_only: false)
      RustPdf.check(Native.call("pdf_editable_encrypt", ptr, method, user, owner, read_only ? 1 : 0))
      self
    end

    def to_bytes
      p = ptr
      RustPdf.take_bytes { |pp, pn| Native.call("pdf_editable_to_bytes", p, pp, pn) }
    end

    def to_bytes_incremental(original)
      p = ptr
      RustPdf.take_bytes do |pp, pn|
        Native.call("pdf_editable_to_bytes_incremental", p, original, original.bytesize, pp, pn)
      end
    end

    def save(path)
      RustPdf.check(Native.call("pdf_editable_save", ptr, path))
    end

    private

    # Shared core of #place_paragraph / #place_paragraph_measured. Returns
    # [found (bool), lines (Integer), height (Float)].
    def paragraph_anchored(page_index, x, y, width, text, size, color, align,
                           font_id, max_height, line_height, anchor, rotation_deg)
      r, g, b = color
      height_buf = Fiddle::Pointer.malloc(Fiddle::SIZEOF_DOUBLE, Fiddle::RUBY_FREE)
      lines_buf = Fiddle::Pointer.malloc(Native::SIZEOF_INT, Fiddle::RUBY_FREE)
      found_buf = Fiddle::Pointer.malloc(Native::SIZEOF_INT, Fiddle::RUBY_FREE)
      RustPdf.check(Native.call("pdf_editable_place_paragraph_anchored", ptr, page_index,
                                x.to_f, y.to_f, width.to_f, text, size.to_f,
                                r.to_f, g.to_f, b.to_f, align, anchor, font_id,
                                (max_height || 0.0).to_f, line_height.to_f, rotation_deg.to_f,
                                height_buf, lines_buf, found_buf))
      [found_buf[0, Native::SIZEOF_INT].unpack1("i!") != 0,
       lines_buf[0, Native::SIZEOF_INT].unpack1("i!"),
       height_buf[0, Fiddle::SIZEOF_DOUBLE].unpack1("d")]
    end

    def ptr
      raise Error, "operation on a closed EditableDoc" if @ptr.nil? || @ptr.null?

      @ptr
    end
  end
end
