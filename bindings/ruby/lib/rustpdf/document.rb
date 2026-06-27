module RustPdf
  # A PDF document being authored. Call #close (or rely on GC) to free.
  class Document
    def initialize
      @ptr = Native.call("pdf_document_new")
      raise Error, "pdf_document_new returned NULL" if @ptr.null?
    end

    def close
      return if @ptr.nil? || @ptr.null?

      Native.call("pdf_document_free", @ptr)
      @ptr = nil
    end

    # ---- configuration ------------------------------------------------------

    def pdfa(level = nil)
      RustPdf.check(
        level ? Native.call("pdf_document_pdfa_level", ptr, level) : Native.call("pdf_document_pdfa", ptr)
      )
      self
    end

    def tagged
      RustPdf.check(Native.call("pdf_document_tagged", ptr))
      self
    end

    def version=(v)
      RustPdf.check(Native.call("pdf_document_set_version", ptr, v))
    end

    def default_size(width, height)
      RustPdf.check(Native.call("pdf_document_set_default_size", ptr, width, height))
      self
    end

    def info(title: nil, author: nil, subject: nil, keywords: nil, creator: nil)
      RustPdf.check(Native.call("pdf_document_set_info", ptr, title, author, subject, keywords, creator))
      self
    end

    # ---- pages + graphics ---------------------------------------------------

    def add_page(width: nil, height: nil)
      RustPdf.check(
        width && height ? Native.call("pdf_document_add_page_sized", ptr, width, height)
                        : Native.call("pdf_document_add_page", ptr)
      )
      self
    end

    def fill_rgb(r, g, b)
      RustPdf.check(Native.call("pdf_page_set_fill_rgb", ptr, r, g, b))
      self
    end

    def stroke_rgb(r, g, b)
      RustPdf.check(Native.call("pdf_page_set_stroke_rgb", ptr, r, g, b))
      self
    end

    def line_width(w)
      RustPdf.check(Native.call("pdf_page_set_line_width", ptr, w))
      self
    end

    def rect(x, y, w, h)
      RustPdf.check(Native.call("pdf_page_rect", ptr, x, y, w, h))
      self
    end

    def fill
      RustPdf.check(Native.call("pdf_page_fill", ptr))
      self
    end

    def stroke
      RustPdf.check(Native.call("pdf_page_stroke", ptr))
      self
    end

    # ---- fonts + text -------------------------------------------------------

    def add_font_file(path)
      RustPdf.out_int { |buf| Native.call("pdf_document_add_font_file", ptr, path, buf) }
    end

    def add_font(data)
      RustPdf.out_int { |buf| Native.call("pdf_document_add_font", ptr, data, data.bytesize, buf) }
    end

    def show_text(font, size, x, y, text, heading_level: 0)
      RustPdf.check(Native.call("pdf_page_show_text", ptr, font, size, x, y, text, heading_level))
      self
    end

    def paragraph(font, size, x, y, width, text, align: Align::LEFT)
      RustPdf.check(Native.call("pdf_page_paragraph", ptr, font, size, x, y, width, align, text))
      self
    end

    # ---- images -------------------------------------------------------------

    def add_image_file(path)
      RustPdf.out_int { |buf| Native.call("pdf_document_add_image_file", ptr, path, buf) }
    end

    def add_image_png(data)
      RustPdf.out_int { |buf| Native.call("pdf_document_add_image_png", ptr, data, data.bytesize, buf) }
    end

    def add_image_jpeg(data)
      RustPdf.out_int { |buf| Native.call("pdf_document_add_image_jpeg", ptr, data, data.bytesize, buf) }
    end

    def draw_image(image, x, y, w, h)
      RustPdf.check(Native.call("pdf_page_draw_image", ptr, image, x, y, w, h))
      self
    end

    def figure(image, x, y, w, h, alt)
      RustPdf.check(Native.call("pdf_page_figure", ptr, image, x, y, w, h, alt))
      self
    end

    # ---- attachments + forms ------------------------------------------------

    def attach_file(name, mime, data, relationship: Relationship::SOURCE, description: "")
      RustPdf.check(Native.call("pdf_document_attach_file", ptr, name, mime, data, data.bytesize, relationship, description))
      self
    end

    # rect = [x0, y0, x1, y1]
    def text_field(name, page, rect, value: "", size: 0.0)
      RustPdf.check(Native.call("pdf_document_text_field", ptr, name, page, rect[0], rect[1], rect[2], rect[3], value, size))
      self
    end

    def checkbox(name, page, rect, checked)
      RustPdf.check(Native.call("pdf_document_checkbox", ptr, name, page, rect[0], rect[1], rect[2], rect[3], checked ? 1 : 0))
      self
    end

    # options: Array<String>; selected: index or nil
    def dropdown(name, page, rect, options, selected: nil, size: 0.0)
      RustPdf.check(Native.call("pdf_document_dropdown", ptr, name, page, rect[0], rect[1], rect[2], rect[3],
                                options.join("\n"), selected || -1, size))
      self
    end

    # buttons: Array<[ [x0,y0,x1,y1], "export" ]>; selected: index or nil
    def radio_group(name, page, buttons, selected: nil)
      rects = buttons.flat_map { |(r, _)| r }.pack("d*")
      cstrs = buttons.map { |(_, e)| Fiddle::Pointer[e.to_s] }
      addrs = cstrs.map(&:to_i).pack("J*")
      RustPdf.check(Native.call("pdf_document_radio_group", ptr, name, page, buttons.size, rects, addrs, selected || -1))
      cstrs.clear # released after the call returned
      self
    end

    # ---- output -------------------------------------------------------------

    def page_count
      Native.call("pdf_document_page_count", ptr)
    end

    def to_bytes
      p = ptr
      RustPdf.take_bytes { |pp, pn| Native.call("pdf_document_write", p, pp, pn) }
    end

    def save(path)
      RustPdf.check(Native.call("pdf_document_save", ptr, path))
    end

    private

    def ptr
      raise Error, "operation on a closed Document" if @ptr.nil? || @ptr.null?

      @ptr
    end
  end
end
