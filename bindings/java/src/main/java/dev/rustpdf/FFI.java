package dev.rustpdf;

import com.sun.jna.Library;
import com.sun.jna.Native;
import com.sun.jna.Pointer;
import com.sun.jna.StringArray;
import com.sun.jna.ptr.IntByReference;
import com.sun.jna.ptr.LongByReference;
import com.sun.jna.ptr.PointerByReference;

import java.io.File;
import java.util.Collections;

/**
 * Raw JNA surface over the rust-pdf C ABI ({@code libpdf_ffi}). Mirrors
 * {@code include/pdf.h} one-to-one; application code uses the idiomatic wrappers
 * ({@link Document}, {@link EditableDoc}, {@link Pdf}).
 *
 * <p>Pointer-sized integers ({@code uintptr_t}) are mapped to Java {@code long};
 * the binding targets 64-bit platforms (as do the other language bindings).
 */
final class FFI {
    private FFI() {}

    /** The loaded native library. */
    static final Lib C = load();

    /** JNA interface mapping the C exports. */
    interface Lib extends Library {
        // ---- core -----------------------------------------------------------
        String pdf_version();
        String pdf_last_error_message();
        int pdf_activate_license(String token);
        void pdf_buffer_free(Pointer ptr, long len);

        // ---- document lifecycle + graphics ----------------------------------
        Pointer pdf_document_new();
        void pdf_document_free(Pointer doc);
        int pdf_document_add_page(Pointer doc);
        int pdf_document_add_page_sized(Pointer doc, double w, double h);
        int pdf_document_page_count(Pointer doc);
        int pdf_page_set_fill_rgb(Pointer doc, double r, double g, double b);
        int pdf_page_set_stroke_rgb(Pointer doc, double r, double g, double b);
        int pdf_page_set_line_width(Pointer doc, double w);
        int pdf_page_rect(Pointer doc, double x, double y, double w, double h);
        int pdf_page_fill(Pointer doc);
        int pdf_page_stroke(Pointer doc);
        int pdf_document_save(Pointer doc, String path);
        int pdf_document_write(Pointer doc, PointerByReference outPtr, LongByReference outLen);

        // ---- configuration --------------------------------------------------
        int pdf_document_pdfa(Pointer doc);
        int pdf_document_pdfa_level(Pointer doc, int level);
        int pdf_document_tagged(Pointer doc);
        int pdf_document_set_version(Pointer doc, int v);
        int pdf_document_set_default_size(Pointer doc, double w, double h);
        int pdf_document_set_info(Pointer doc, String title, String author, String subject,
                                  String keywords, String creator);

        // ---- fonts + text ---------------------------------------------------
        int pdf_document_add_font_file(Pointer doc, String path, IntByReference outId);
        int pdf_document_add_font(Pointer doc, byte[] data, long len, IntByReference outId);
        int pdf_page_show_text(Pointer doc, int font, double size, double x, double y,
                               String text, int headingLevel);
        int pdf_page_paragraph(Pointer doc, int font, double size, double x, double y,
                               double width, int align, String text);

        // ---- images ---------------------------------------------------------
        int pdf_document_add_image_file(Pointer doc, String path, IntByReference outId);
        int pdf_document_add_image_png(Pointer doc, byte[] data, long len, IntByReference outId);
        int pdf_document_add_image_jpeg(Pointer doc, byte[] data, long len, IntByReference outId);
        int pdf_page_draw_image(Pointer doc, int image, double x, double y, double w, double h);
        int pdf_page_figure(Pointer doc, int image, double x, double y, double w, double h, String alt);

        // ---- attachments + forms --------------------------------------------
        int pdf_document_attach_file(Pointer doc, String name, String mime, byte[] data, long len,
                                     int relationship, String desc);
        int pdf_document_text_field(Pointer doc, String name, long page, double x0, double y0,
                                    double x1, double y1, String value, double size);
        int pdf_document_checkbox(Pointer doc, String name, long page, double x0, double y0,
                                  double x1, double y1, int checked);
        int pdf_document_dropdown(Pointer doc, String name, long page, double x0, double y0,
                                  double x1, double y1, String options, int selected, double size);
        int pdf_document_radio_group(Pointer doc, String name, long page, long count, double[] rects,
                                     StringArray exports, int selected);

        // ---- editable -------------------------------------------------------
        Pointer pdf_editable_load(byte[] data, long len);
        Pointer pdf_editable_load_password(byte[] data, long len, String password);
        void pdf_editable_free(Pointer ed);
        int pdf_editable_page_count(Pointer ed);
        int pdf_editable_merge(Pointer ed, Pointer other);
        int pdf_editable_rotate_page(Pointer ed, long index, int degrees);
        int pdf_editable_delete_page(Pointer ed, long index);
        int pdf_editable_reorder_pages(Pointer ed, long[] order, long count);
        int pdf_editable_extract_pages(Pointer ed, long[] indices, long count, PointerByReference outEd);
        int pdf_editable_set_info(Pointer ed, String key, String value);
        int pdf_editable_get_info(Pointer ed, String key, PointerByReference outPtr, LongByReference outLen);
        int pdf_editable_set_xmp(Pointer ed, byte[] xml, long len);
        int pdf_editable_overlay_page(Pointer ed, long index, byte[] content, long len);
        int pdf_editable_fill_text_field(Pointer ed, String name, String value, IntByReference outFound);
        int pdf_editable_optimize(Pointer ed);
        int pdf_editable_compact(Pointer ed, int on);
        int pdf_editable_encrypt(Pointer ed, int method, String user, String owner, int readOnly);
        int pdf_editable_to_bytes(Pointer ed, PointerByReference outPtr, LongByReference outLen);
        int pdf_editable_to_bytes_incremental(Pointer ed, byte[] original, long originalLen,
                                              PointerByReference outPtr, LongByReference outLen);
        int pdf_editable_save(Pointer ed, String path);

        // ---- extract + sign -------------------------------------------------
        int pdf_extract_text(byte[] data, long len, PointerByReference outPtr, LongByReference outLen);
        int pdf_sign(byte[] pdf, long pdfLen, byte[] keyDer, long keyLen, byte[] certDer, long certLen,
                     String reason, String location, String name, int pades,
                     PointerByReference outPtr, LongByReference outLen);
        int pdf_timestamp(byte[] pdf, long pdfLen, byte[] keyDer, long keyLen, byte[] certDer, long certLen,
                          String date, PointerByReference outPtr, LongByReference outLen);
        int pdf_add_dss(byte[] pdf, long pdfLen,
                        Pointer[] certPtrs, long[] certLens, long certCount,
                        Pointer[] crlPtrs, long[] crlLens, long crlCount,
                        PointerByReference outPtr, LongByReference outLen);
    }

    private static Lib load() {
        String path = locate();
        // UTF-8 for all String marshalling (the core validates UTF-8 and errors otherwise).
        return Native.load(path, Lib.class,
                Collections.singletonMap(Library.OPTION_STRING_ENCODING, "UTF-8"));
    }

    /** Resolve the shared library: {@code RUSTPDF_LIB}, else target/{debug,release}. */
    private static String locate() {
        String env = System.getenv("RUSTPDF_LIB");
        if (env != null && !env.isEmpty()) {
            return env;
        }
        String file = libFileName();
        // Walk up from the working directory looking for the Cargo workspace root.
        File dir = new File(System.getProperty("user.dir")).getAbsoluteFile();
        for (int i = 0; i < 12 && dir != null; i++) {
            for (String profile : new String[] {"debug", "release"}) {
                File cand = new File(dir, "target/" + profile + "/" + file);
                if (cand.isFile()) {
                    return cand.getAbsolutePath();
                }
            }
            dir = dir.getParentFile();
        }
        // Fall back to the platform loader's search path (jna.library.path / system).
        return "pdf_ffi";
    }

    private static String libFileName() {
        String os = System.getProperty("os.name", "").toLowerCase();
        if (os.contains("win")) return "pdf_ffi.dll";
        if (os.contains("mac") || os.contains("darwin")) return "libpdf_ffi.dylib";
        return "libpdf_ffi.so";
    }
}
