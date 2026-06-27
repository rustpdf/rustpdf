require "fiddle"

module RustPdf
  # Internal: dlopen's libpdf_ffi and builds memoized Fiddle::Function objects
  # from a signature table. Not part of the public API.
  module Native
    module_function

    VP   = Fiddle::TYPE_VOIDP
    I    = Fiddle::TYPE_INT
    D    = Fiddle::TYPE_DOUBLE
    SZ   = Fiddle::TYPE_SIZE_T
    VOID = Fiddle::TYPE_VOID

    # size_t / uintptr_t and int widths (for out-parameter buffers).
    SIZEOF_SZ  = Fiddle::SIZEOF_VOIDP
    SIZEOF_INT = Fiddle::SIZEOF_INT

    SIGS = {
      "pdf_version"                  => [[], VP],
      "pdf_last_error_message"       => [[], VP],
      "pdf_activate_license"         => [[VP], I],
      "pdf_buffer_free"              => [[VP, SZ], VOID],

      "pdf_document_new"             => [[], VP],
      "pdf_document_free"            => [[VP], VOID],
      "pdf_document_add_page"        => [[VP], I],
      "pdf_document_add_page_sized"  => [[VP, D, D], I],
      "pdf_document_page_count"      => [[VP], I],
      "pdf_page_set_fill_rgb"        => [[VP, D, D, D], I],
      "pdf_page_set_stroke_rgb"      => [[VP, D, D, D], I],
      "pdf_page_set_line_width"      => [[VP, D], I],
      "pdf_page_rect"                => [[VP, D, D, D, D], I],
      "pdf_page_fill"                => [[VP], I],
      "pdf_page_stroke"              => [[VP], I],
      "pdf_document_save"            => [[VP, VP], I],
      "pdf_document_write"           => [[VP, VP, VP], I],
      "pdf_document_pdfa"            => [[VP], I],
      "pdf_document_pdfa_level"      => [[VP, I], I],
      "pdf_document_tagged"          => [[VP], I],
      "pdf_document_set_version"     => [[VP, I], I],
      "pdf_document_set_default_size" => [[VP, D, D], I],
      "pdf_document_set_info"        => [[VP, VP, VP, VP, VP, VP], I],
      "pdf_document_add_font_file"   => [[VP, VP, VP], I],
      "pdf_document_add_font"        => [[VP, VP, SZ, VP], I],
      "pdf_page_show_text"           => [[VP, I, D, D, D, VP, I], I],
      "pdf_page_paragraph"           => [[VP, I, D, D, D, D, I, VP], I],
      "pdf_document_add_image_file"  => [[VP, VP, VP], I],
      "pdf_document_add_image_png"   => [[VP, VP, SZ, VP], I],
      "pdf_document_add_image_jpeg"  => [[VP, VP, SZ, VP], I],
      "pdf_page_draw_image"          => [[VP, I, D, D, D, D], I],
      "pdf_page_figure"              => [[VP, I, D, D, D, D, VP], I],
      "pdf_document_attach_file"     => [[VP, VP, VP, VP, SZ, I, VP], I],
      "pdf_document_text_field"      => [[VP, VP, SZ, D, D, D, D, VP, D], I],
      "pdf_document_checkbox"        => [[VP, VP, SZ, D, D, D, D, I], I],
      "pdf_document_dropdown"        => [[VP, VP, SZ, D, D, D, D, VP, I, D], I],
      "pdf_document_radio_group"     => [[VP, VP, SZ, SZ, VP, VP, I], I],

      "pdf_editable_load"            => [[VP, SZ], VP],
      "pdf_editable_load_password"   => [[VP, SZ, VP], VP],
      "pdf_editable_free"            => [[VP], VOID],
      "pdf_editable_page_count"      => [[VP], I],
      "pdf_editable_merge"           => [[VP, VP], I],
      "pdf_editable_rotate_page"     => [[VP, SZ, I], I],
      "pdf_editable_delete_page"     => [[VP, SZ], I],
      "pdf_editable_reorder_pages"   => [[VP, VP, SZ], I],
      "pdf_editable_extract_pages"   => [[VP, VP, SZ, VP], I],
      "pdf_editable_set_info"        => [[VP, VP, VP], I],
      "pdf_editable_get_info"        => [[VP, VP, VP, VP], I],
      "pdf_editable_set_xmp"         => [[VP, VP, SZ], I],
      "pdf_editable_overlay_page"    => [[VP, SZ, VP, SZ], I],
      "pdf_editable_fill_text_field" => [[VP, VP, VP, VP], I],
      "pdf_editable_optimize"        => [[VP], I],
      "pdf_editable_compact"         => [[VP, I], I],
      "pdf_editable_encrypt"         => [[VP, I, VP, VP, I], I],
      "pdf_editable_to_bytes"        => [[VP, VP, VP], I],
      "pdf_editable_to_bytes_incremental" => [[VP, VP, SZ, VP, VP], I],
      "pdf_editable_save"            => [[VP, VP], I],

      "pdf_extract_text"             => [[VP, SZ, VP, VP], I],
      "pdf_sign"                     => [[VP, SZ, VP, SZ, VP, SZ, VP, VP, VP, I, VP, VP], I],
      "pdf_timestamp"                => [[VP, SZ, VP, SZ, VP, SZ, VP, VP, VP], I],
      "pdf_add_dss"                  => [[VP, SZ, VP, VP, SZ, VP, VP, SZ, VP, VP], I],
    }.freeze

    def lib
      @lib ||= Fiddle.dlopen(lib_path)
    end

    def [](name)
      @fns ||= {}
      @fns[name] ||= begin
        args, ret = SIGS.fetch(name)
        Fiddle::Function.new(lib[name], args, ret)
      end
    end

    def call(name, *args)
      self[name].call(*args)
    end

    def lib_path
      env = ENV["RUSTPDF_LIB"]
      return env if env && !env.empty? && File.file?(env)

      file = lib_file_name
      dir = __dir__
      10.times do
        %w[debug release].each do |profile|
          candidate = File.join(dir, "target", profile, file)
          return candidate if File.file?(candidate)
        end
        parent = File.dirname(dir)
        break if parent == dir
        dir = parent
      end
      raise Error, "could not locate #{file}; build it with `cargo build -p pdf-ffi` or set RUSTPDF_LIB"
    end

    def lib_file_name
      case RbConfig::CONFIG["host_os"]
      when /mswin|mingw|cygwin/ then "pdf_ffi.dll"
      when /darwin/ then "libpdf_ffi.dylib"
      else "libpdf_ffi.so"
      end
    end
  end
end
