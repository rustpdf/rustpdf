package dev.rustpdf;

import com.sun.jna.Pointer;
import com.sun.jna.ptr.IntByReference;
import com.sun.jna.ptr.PointerByReference;

import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;

/**
 * An existing PDF loaded for manipulation. Use with try-with-resources so the
 * native handle is freed.
 */
public final class EditableDoc implements AutoCloseable {
    private Pointer h;

    private EditableDoc(Pointer handle) {
        if (handle == null) {
            throw new PdfException(6, Pdf.lastError());
        }
        this.h = handle;
    }

    /** Load and parse a PDF from bytes. */
    public static EditableDoc load(byte[] data) {
        return new EditableDoc(FFI.C.pdf_editable_load(data, data.length));
    }

    /** Load an encrypted PDF using the given password. */
    public static EditableDoc load(byte[] data, String password) {
        return new EditableDoc(FFI.C.pdf_editable_load_password(data, data.length, password));
    }

    public static EditableDoc loadFile(Path path) {
        return load(readAll(path));
    }

    public static EditableDoc loadFile(Path path, String password) {
        return load(readAll(path), password);
    }

    private static byte[] readAll(Path path) {
        try {
            return Files.readAllBytes(path);
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }

    private Pointer h() {
        if (h == null) {
            throw new IllegalStateException("operation on a closed EditableDoc");
        }
        return h;
    }

    @Override
    public void close() {
        if (h != null) {
            FFI.C.pdf_editable_free(h);
            h = null;
        }
    }

    public int pageCount() {
        return FFI.C.pdf_editable_page_count(h());
    }

    public EditableDoc merge(EditableDoc other) {
        Pdf.check(FFI.C.pdf_editable_merge(h(), other.h()));
        return this;
    }

    public EditableDoc rotatePage(int index, int degrees) {
        Pdf.check(FFI.C.pdf_editable_rotate_page(h(), index, degrees));
        return this;
    }

    public EditableDoc deletePage(int index) {
        Pdf.check(FFI.C.pdf_editable_delete_page(h(), index));
        return this;
    }

    public EditableDoc reorderPages(int[] order) {
        long[] arr = new long[order.length];
        for (int i = 0; i < order.length; i++) {
            arr[i] = order[i];
        }
        Pdf.check(FFI.C.pdf_editable_reorder_pages(h(), arr, arr.length));
        return this;
    }

    /** Extract the given page indices into a new document. */
    public EditableDoc extractPages(int[] indices) {
        long[] arr = new long[indices.length];
        for (int i = 0; i < indices.length; i++) {
            arr[i] = indices[i];
        }
        PointerByReference out = new PointerByReference();
        Pdf.check(FFI.C.pdf_editable_extract_pages(h(), arr, arr.length, out));
        return new EditableDoc(out.getValue());
    }

    public EditableDoc setInfo(String key, String value) {
        Pdf.check(FFI.C.pdf_editable_set_info(h(), key, value));
        return this;
    }

    public String getInfo(String key) {
        byte[] bytes = Pdf.takeBuffer((p, n) -> FFI.C.pdf_editable_get_info(h(), key, p, n));
        return new String(bytes, StandardCharsets.UTF_8);
    }

    public EditableDoc setXmp(byte[] xml) {
        Pdf.check(FFI.C.pdf_editable_set_xmp(h(), xml, xml.length));
        return this;
    }

    public EditableDoc overlayPage(int index, byte[] content) {
        Pdf.check(FFI.C.pdf_editable_overlay_page(h(), index, content, content.length));
        return this;
    }

    /** Fill an AcroForm text field; returns whether it existed. */
    public boolean fillTextField(String name, String value) {
        IntByReference found = new IntByReference();
        Pdf.check(FFI.C.pdf_editable_fill_text_field(h(), name, value, found));
        return found.getValue() != 0;
    }

    /** Set an AcroForm checkbox checked/unchecked; returns whether it existed. */
    public boolean setCheckbox(String name, boolean checked) {
        IntByReference found = new IntByReference();
        Pdf.check(FFI.C.pdf_editable_set_checkbox(h(), name, checked ? 1 : 0, found));
        return found.getValue() != 0;
    }

    /** Select a radio-button group's value by export value; returns whether it existed. */
    public boolean setRadio(String name, String exportValue) {
        IntByReference found = new IntByReference();
        Pdf.check(FFI.C.pdf_editable_set_radio(h(), name, exportValue, found));
        return found.getValue() != 0;
    }

    /** Set a choice (dropdown/list) field's value; returns whether it existed. */
    public boolean setChoice(String name, String value) {
        IntByReference found = new IntByReference();
        Pdf.check(FFI.C.pdf_editable_set_choice(h(), name, value, found));
        return found.getValue() != 0;
    }

    /** Flatten all AcroForm fields into static page content (drops interactivity). */
    public EditableDoc flattenForms() {
        Pdf.check(FFI.C.pdf_editable_flatten_forms(h()));
        return this;
    }

    /** Return every AcroForm field name. */
    public List<String> fieldNames() {
        byte[] bytes = Pdf.takeBuffer((p, n) -> FFI.C.pdf_editable_field_names(h(), p, n));
        String joined = new String(bytes, StandardCharsets.UTF_8);
        List<String> names = new java.util.ArrayList<>();
        for (String s : joined.split("\n")) {
            if (!s.isEmpty()) {
                names.add(s);
            }
        }
        return names;
    }

    /** Stamp a diagonal text watermark on every page (sensible defaults). */
    public EditableDoc watermarkText(String text) {
        return watermarkText(text, 64.0, 0.5, 0.5, 0.5, 0.30, 45.0, false);
    }

    /** Stamp a text watermark on every page. {@code rotationDeg} is the rotation in degrees. */
    public EditableDoc watermarkText(String text, double size, double r, double g, double b,
                                     double opacity, double rotationDeg) {
        return watermarkText(text, size, r, g, b, opacity, rotationDeg, false);
    }

    /**
     * Stamp a text watermark on every page. When {@code opaqueBackground} is true
     * the text is drawn over an opaque filled box (e.g. a stamp/redaction label).
     */
    public EditableDoc watermarkText(String text, double size, double r, double g, double b,
                                     double opacity, double rotationDeg, boolean opaqueBackground) {
        Pdf.check(FFI.C.pdf_editable_watermark_text(
                h(), text, size, r, g, b, opacity, rotationDeg, opaqueBackground ? 1 : 0));
        return this;
    }

    /** Stamp an image watermark (from a file) on every page. */
    public EditableDoc watermarkImageFile(String path, double width, double height, double opacity) {
        return watermarkImageFile(path, width, height, opacity, 0.0);
    }

    /** Stamp an image watermark (from a file) on every page, rotated {@code rotationDeg} degrees. */
    public EditableDoc watermarkImageFile(String path, double width, double height,
                                          double opacity, double rotationDeg) {
        Pdf.check(FFI.C.pdf_editable_watermark_image_file(
                h(), path, width, height, opacity, rotationDeg));
        return this;
    }

    /**
     * Paint a filled rectangle at {@code (x, y)} sized {@code width}×{@code height}
     * on page {@code pageIndex} (0-based), in RGB ({@code r}/{@code g}/{@code b},
     * each 0..=1) at {@code opacity} (0..=1). Coordinates are in the page's visible
     * space (origin lower-left, y up), regardless of any page {@code /Rotate}.
     * Returns whether the page existed. The common use is masking a placeholder
     * with an opaque white box.
     */
    public boolean fillRect(int pageIndex, double x, double y, double width, double height,
                            double r, double g, double b, double opacity) {
        IntByReference found = new IntByReference();
        Pdf.check(FFI.C.pdf_editable_fill_rect(
                h(), pageIndex, x, y, width, height, r, g, b, opacity, found));
        return found.getValue() != 0;
    }

    /**
     * Draw a line of positioned text with baseline at {@code (x, y)} on page
     * {@code pageIndex} (0-based), using standard Helvetica at {@code size} points
     * in RGB ({@code r}/{@code g}/{@code b}, each 0..=1). {@code rotationDeg}
     * rotates the text counter-clockwise about its anchor {@code (x, y)} (match the
     * page rotation to follow a rotated page). Coordinates are in the page's visible
     * space (origin lower-left, y up), regardless of any page {@code /Rotate}.
     * Returns whether the page existed.
     */
    public boolean placeText(int pageIndex, double x, double y, String text, double size,
                             double r, double g, double b, double rotationDeg) {
        IntByReference found = new IntByReference();
        Pdf.check(FFI.C.pdf_editable_place_text(
                h(), pageIndex, x, y, text, size, r, g, b, rotationDeg, found));
        return found.getValue() != 0;
    }

    /**
     * Draw a line of positioned text on page {@code pageIndex} (0-based) like
     * {@link #placeText(int, double, double, String, double, double, double, double, double)},
     * but horizontally {@code align}ed about the anchor {@code (x, y)}: for
     * {@link Align#RIGHT}/{@link Align#CENTER} the start point is shifted back by
     * the measured text width (Helvetica metrics). Coordinates are in the page's
     * visible space (origin lower-left, y up). Returns whether the page existed.
     */
    public boolean placeText(int pageIndex, double x, double y, String text, double size,
                             double r, double g, double b, double rotationDeg, Align align) {
        IntByReference found = new IntByReference();
        Pdf.check(FFI.C.pdf_editable_place_text_aligned(
                h(), pageIndex, x, y, text, size, r, g, b, rotationDeg, align.code, found));
        return found.getValue() != 0;
    }

    /**
     * Draw {@code text} over an opaque background box {@code [x, y, x+width, y+height]}
     * on page {@code pageIndex} (0-based): fills the box in {@code bgColor}
     * ({@code {r, g, b}}, each 0..=1), then writes the text (standard Helvetica at
     * {@code size} points in {@code textColor}) horizontally {@code align}ed and
     * vertically centered within the box. The classic use is masking a placeholder
     * and stamping the real value over it without hand-computing the baseline.
     * Coordinates are in the page's visible space (origin lower-left, y up).
     * Returns whether the page existed.
     */
    public boolean maskedText(int pageIndex, double x, double y, double width, double height,
                              String text, double size,
                              double[] textColor, double[] bgColor, Align align) {
        double[] tc = textColor == null ? new double[] {0.0, 0.0, 0.0} : textColor;
        double[] bc = bgColor == null ? new double[] {1.0, 1.0, 1.0} : bgColor;
        IntByReference found = new IntByReference();
        Pdf.check(FFI.C.pdf_editable_masked_text(
                h(), pageIndex, x, y, width, height, text, size,
                tc[0], tc[1], tc[2], bc[0], bc[1], bc[2], align.code, found));
        return found.getValue() != 0;
    }

    /**
     * Draw {@code text} over an opaque <em>white</em> box with <em>black</em> text,
     * left-aligned — see
     * {@link #maskedText(int, double, double, double, double, String, double, double[], double[], Align)}.
     */
    public boolean maskedText(int pageIndex, double x, double y, double width, double height,
                              String text, double size) {
        return maskedText(pageIndex, x, y, width, height, text, size, null, null, Align.LEFT);
    }

    /**
     * Stamp an image (PNG or JPEG bytes — the format is detected from the data
     * signature) onto page {@code pageIndex} (0-based), with the image's lower-left
     * corner at {@code (x, y)}, scaled to {@code width}×{@code height} points.
     * Coordinates are in the page's visible space (origin lower-left, y up),
     * regardless of any page {@code /Rotate}. Returns whether the page existed.
     */
    public boolean drawImage(int pageIndex, byte[] image, double x, double y,
                             double width, double height, double rotationDeg) {
        IntByReference found = new IntByReference();
        Pdf.check(FFI.C.pdf_editable_draw_image(
                h(), pageIndex, image, image.length, x, y, width, height, rotationDeg, found));
        return found.getValue() != 0;
    }

    /**
     * Stamp an image onto page {@code pageIndex} with no rotation — see
     * {@link #drawImage(int, byte[], double, double, double, double, double)}.
     */
    public boolean drawImage(int pageIndex, byte[] image, double x, double y,
                             double width, double height) {
        return drawImage(pageIndex, image, x, y, width, height, 0.0);
    }

    /**
     * Set the output PDF version (downgrade/normalize): {@code version} is
     * {@code 0}=1.4, {@code 1}=1.5, {@code 2}=1.7, {@code 3}=2.0 (the same mapping
     * as {@link Document#setVersion(int)}).
     */
    public EditableDoc setVersion(int version) {
        Pdf.check(FFI.C.pdf_editable_set_version(h(), version));
        return this;
    }

    /**
     * Strip PDF/A conformance (catalog {@code /OutputIntents}, the XMP
     * {@code pdfaid} block and {@code /Version}) so the file becomes a plain PDF.
     */
    public EditableDoc stripPdfa() {
        Pdf.check(FFI.C.pdf_editable_strip_pdfa(h()));
        return this;
    }

    /**
     * Normalize to a plain PDF at {@code version} (strip PDF/A then set the
     * version). Version codes as in {@link #setVersion(int)}.
     */
    public EditableDoc normalize(int version) {
        Pdf.check(FFI.C.pdf_editable_normalize(h(), version));
        return this;
    }

    /**
     * Redact the given rectangles on page {@code pageIndex}; each {@code rect} =
     * {x0, y0, x1, y1}. Returns whether the page existed.
     */
    public boolean redact(int pageIndex, double[][] rects) {
        double[] flat = new double[rects.length * 4];
        for (int i = 0; i < rects.length; i++) {
            flat[i * 4] = rects[i][0];
            flat[i * 4 + 1] = rects[i][1];
            flat[i * 4 + 2] = rects[i][2];
            flat[i * 4 + 3] = rects[i][3];
        }
        IntByReference found = new IntByReference();
        Pdf.check(FFI.C.pdf_editable_redact(h(), pageIndex, flat, rects.length, found));
        return found.getValue() != 0;
    }

    /** Convert the loaded document to PDF/A. Only B-levels (A1B/A2B/A3B) are valid. Requires a license. */
    public EditableDoc convertToPdfa(PdfaLevel level) {
        Pdf.check(FFI.C.pdf_editable_convert_to_pdfa(h(), level.code));
        return this;
    }

    public EditableDoc optimize() {
        Pdf.check(FFI.C.pdf_editable_optimize(h()));
        return this;
    }

    public EditableDoc compact(boolean on) {
        Pdf.check(FFI.C.pdf_editable_compact(h(), on ? 1 : 0));
        return this;
    }

    /** Encrypt on save (requires a license). */
    public EditableDoc encrypt(String user, String owner, Encryption method, boolean readOnly) {
        Pdf.check(FFI.C.pdf_editable_encrypt(h(), method.code, user, owner, readOnly ? 1 : 0));
        return this;
    }

    public byte[] toBytes() {
        return Pdf.takeBuffer((p, n) -> FFI.C.pdf_editable_to_bytes(h(), p, n));
    }

    /** Serialize as an incremental update over {@code original} (preserves it verbatim). */
    public byte[] toBytesIncremental(byte[] original) {
        return Pdf.takeBuffer((p, n) ->
                FFI.C.pdf_editable_to_bytes_incremental(h(), original, original.length, p, n));
    }

    public void save(String path) {
        Pdf.check(FFI.C.pdf_editable_save(h(), path));
    }
}
