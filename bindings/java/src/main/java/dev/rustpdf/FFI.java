package dev.rustpdf;

import com.sun.jna.Callback;
import com.sun.jna.Library;
import com.sun.jna.Native;
import com.sun.jna.Pointer;
import com.sun.jna.Structure;
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

    /**
     * Mirrors the C-ABI {@code PdfSigningOptions} (deferred-signing options).
     * Pointer fields are {@code null} when unused; a zero {@code estimated_size}
     * or {@code policy_hash_len} means "absent". The field order is exactly the
     * struct declaration order. Strings are passed as NUL-terminated UTF-8
     * {@link Pointer}s allocated by the caller (so the field encoding does not
     * depend on JNA's default charset).
     */
    @Structure.FieldOrder({
        "reason", "location", "name", "pades", "certification", "estimatedSize",
        "policyOid", "policyHash", "policyHashLen", "policyHashAlgOid", "policyUri",
        "visible", "visPage", "visRect", "visText", "visImage", "visImageLen"
    })
    public static final class PdfSigningOptions extends Structure {
        public Pointer reason;
        public Pointer location;
        public Pointer name;
        public int pades;
        public int certification;
        public long estimatedSize;       // uintptr_t (64-bit target)
        public Pointer policyOid;
        public Pointer policyHash;
        public long policyHashLen;       // uintptr_t (64-bit target)
        public Pointer policyHashAlgOid;
        public Pointer policyUri;
        // ---- visible signature appearance (issue #41 P1) --------------------
        public int visible;
        public long visPage;             // uintptr_t (64-bit target)
        public double[] visRect = new double[4]; // [x0, y0, x1, y1], inline array
        public Pointer visText;
        public Pointer visImage;
        public long visImageLen;         // uintptr_t (64-bit target)

        public PdfSigningOptions() {
            super();
        }
    }

    /**
     * Mirrors {@code PdfSignHashFn}: produce the raw RSA PKCS#1 v1.5 signature
     * (over SHA-256 of {@code data}) from a remote HSM. Write it into
     * {@code sigBuf} (capacity {@code sigCap}), set {@code *sigLen}, return 0 on
     * success (non-zero = failure).
     */
    public interface SignHashCallback extends Callback {
        int invoke(Pointer ctx, Pointer data, long dataLen, Pointer sigBuf, long sigCap, Pointer sigLen);
    }

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
        int pdf_extract_images_to_dir(byte[] data, long len, String dir, LongByReference outCount);
        int pdf_render_page_to_png(byte[] data, long len, long pageIndex, double dpi,
                                   PointerByReference outPtr, LongByReference outLen);
        int pdf_page_count(byte[] data, long len, LongByReference outCount);
        int pdf_sign(byte[] pdf, long pdfLen, byte[] keyDer, long keyLen, byte[] certDer, long certLen,
                     String reason, String location, String name, int pades,
                     PointerByReference outPtr, LongByReference outLen);
        int pdf_timestamp(byte[] pdf, long pdfLen, byte[] keyDer, long keyLen, byte[] certDer, long certLen,
                          String date, PointerByReference outPtr, LongByReference outLen);
        int pdf_add_dss(byte[] pdf, long pdfLen,
                        Pointer[] certPtrs, long[] certLens, long certCount,
                        Pointer[] crlPtrs, long[] crlLens, long crlCount,
                        PointerByReference outPtr, LongByReference outLen);

        // ---- Tier 1: hyperlinks + bookmarks (Document) ----------------------
        int pdf_page_link_uri(Pointer doc, double x0, double y0, double x1, double y1, String uri);
        int pdf_page_link_to_page(Pointer doc, double x0, double y0, double x1, double y1,
                                  long targetPage, double top, int hasTop);
        int pdf_document_add_bookmarks(Pointer doc, long count, int[] levels, StringArray titles,
                                       long[] pages, double[] tops, int[] hasTops);

        // ---- Tier 2: ZUGFeRD / Factur-X (Document) --------------------------
        int pdf_document_facturx(Pointer doc, byte[] xml, long len, int profile);

        // ---- Tier 1: form fill + flatten + watermark (EditableDoc) ----------
        int pdf_editable_set_checkbox(Pointer ed, String name, int checked, IntByReference outFound);
        int pdf_editable_set_radio(Pointer ed, String name, String exportValue, IntByReference outFound);
        int pdf_editable_set_choice(Pointer ed, String name, String value, IntByReference outFound);
        int pdf_editable_flatten_forms(Pointer ed);
        int pdf_editable_field_names(Pointer ed, PointerByReference outPtr, LongByReference outLen);
        int pdf_editable_watermark_text(Pointer ed, String text, double size,
                                        double r, double g, double b, double opacity,
                                        double rotationDeg, int opaqueBackground);
        int pdf_editable_watermark_image_file(Pointer ed, String path,
                                              double width, double height, double opacity,
                                              double rotationDeg);

        // ---- Page content drawing — issue #45 P1 (EditableDoc) --------------
        int pdf_editable_fill_rect(Pointer ed, int index, double x, double y,
                                   double width, double height, double r, double g, double b,
                                   double opacity, IntByReference outFound);
        int pdf_editable_place_text(Pointer ed, int index, double x, double y, String text,
                                    double size, double r, double g, double b,
                                    double rotationDeg, IntByReference outFound);

        // ---- Normalization — issue #41 P1 (EditableDoc) ---------------------
        int pdf_editable_set_version(Pointer ed, int version);
        int pdf_editable_strip_pdfa(Pointer ed);
        int pdf_editable_normalize(Pointer ed, int version);

        // ---- Tier 2: redaction + PDF/A conversion (EditableDoc) -------------
        int pdf_editable_redact(Pointer ed, long index, double[] rects, long count, IntByReference outFound);
        int pdf_editable_convert_to_pdfa(Pointer ed, int level);

        // ---- Tier 2: signature validation (module-level) --------------------
        int pdf_verify_signatures_json(byte[] data, long len,
                                       PointerByReference outPtr, LongByReference outLen);

        // ---- Deferred / external (HSM) signing — issue #41 P0 ---------------
        int pdf_sign_begin(byte[] pdf, long pdfLen, PdfSigningOptions params,
                           PointerByReference outDoc, LongByReference outDocLen,
                           PointerByReference outTbs, LongByReference outTbsLen);
        int pdf_sign_complete(byte[] document, long documentLen, byte[] container, long containerLen,
                              PointerByReference outPtr, LongByReference outLen);
        int pdf_sign_with(byte[] pdf, long pdfLen, byte[] certDer, long certLen,
                          Pointer[] chainPtrs, long[] chainLens, long chainCount,
                          PdfSigningOptions params, SignHashCallback callback, Pointer ctx,
                          PointerByReference outPtr, LongByReference outLen);
        int pdf_list_signatures(byte[] pdf, long pdfLen,
                                PointerByReference outPtr, LongByReference outLen);

        // ---- Positional text search — issue #41 P1 --------------------------
        int pdf_find_text_json(byte[] data, long len, String query, int caseSensitive,
                               PointerByReference outPtr, LongByReference outLen);

        // ---- Page geometry + inspection (JSON) — issue #45 P1 ---------------
        int pdf_measure_pages_json(byte[] data, long len,
                                   PointerByReference outPtr, LongByReference outLen);
        int pdf_inspect_json(byte[] data, long len,
                             PointerByReference outPtr, LongByReference outLen);

        // ---- Network TSA (AD-RT) — issue #41 P1 -----------------------------
        int pdf_timestamp_begin(byte[] pdf, long pdfLen,
                                PointerByReference outDoc, LongByReference outDocLen,
                                PointerByReference outTbs, LongByReference outTbsLen);
        int pdf_timestamp_request(byte[] imprint, long imprintLen, byte[] nonce, long nonceLen,
                                  int certReq, PointerByReference outPtr, LongByReference outLen);
        int pdf_timestamp_token_from_response(byte[] response, long responseLen,
                                              PointerByReference outPtr, LongByReference outLen);
    }

    private static Lib load() {
        String path = locate();
        // UTF-8 for all String marshalling (the core validates UTF-8 and errors otherwise).
        return Native.load(path, Lib.class,
                Collections.singletonMap(Library.OPTION_STRING_ENCODING, "UTF-8"));
    }

    /**
     * Resolve the shared library, in priority order:
     * <ol>
     *   <li>{@code RUSTPDF_LIB} (an absolute path) — explicit override;</li>
     *   <li>{@code target/{debug,release}} walking up from the CWD — the dev tree;</li>
     *   <li>the bare name {@code "pdf_ffi"} — lets JNA extract the platform's native
     *       lib bundled in the JAR as a classpath resource under
     *       {@code <Platform.RESOURCE_PREFIX>/} (e.g. {@code darwin-aarch64/},
     *       {@code linux-x86-64/}, {@code win32-x86-64/}). This is how the published
     *       fat JAR ships: one prebuilt {@code libpdf_ffi} per platform, JNA picks
     *       the matching one at load time.</li>
     * </ol>
     */
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
        // Bare name: JNA searches jna.library.path / the system loader AND extracts a
        // bundled lib from the classpath at /<RESOURCE_PREFIX>/<libFileName> — the
        // mechanism the published fat JAR relies on.
        return "pdf_ffi";
    }

    private static String libFileName() {
        String os = System.getProperty("os.name", "").toLowerCase();
        if (os.contains("win")) return "pdf_ffi.dll";
        if (os.contains("mac") || os.contains("darwin")) return "libpdf_ffi.dylib";
        return "libpdf_ffi.so";
    }
}
