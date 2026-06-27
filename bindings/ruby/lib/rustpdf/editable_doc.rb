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

    def optimize
      RustPdf.check(Native.call("pdf_editable_optimize", ptr))
      self
    end

    def compact(on = true)
      RustPdf.check(Native.call("pdf_editable_compact", ptr, on ? 1 : 0))
      self
    end

    # Encrypt on save (requires a license).
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

    def ptr
      raise Error, "operation on a closed EditableDoc" if @ptr.nil? || @ptr.null?

      @ptr
    end
  end
end
