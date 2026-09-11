package dev.rustpdf;

import com.sun.jna.Pointer;
import com.sun.jna.ptr.DoubleByReference;
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
        return placeText(pageIndex, x, y, text, size, r, g, b, rotationDeg, Align.LEFT);
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
        return placeText(pageIndex, x, y, text, size, r, g, b, rotationDeg, align, -1);
    }

    /**
     * Like {@link #placeText(int, double, double, String, double, double, double, double, double, Align)}
     * but stamped with an embedded TrueType/OpenType font: pass {@code fontId}
     * from {@link #addFontFile(String)}/{@link #addFont(byte[])} (e.g. Times New
     * Roman), or {@code -1} for the built-in Helvetica. Alignment uses the
     * selected font's real metrics. Returns whether the page (and font) existed.
     */
    public boolean placeText(int pageIndex, double x, double y, String text, double size,
                             double r, double g, double b, double rotationDeg, Align align,
                             int fontId) {
        return placeText(pageIndex, x, y, text, size, r, g, b, rotationDeg, align, fontId,
                VerticalAnchor.BASELINE);
    }

    /**
     * Like {@link #placeText(int, double, double, String, double, double, double, double, double, Align, int)}
     * but with an explicit vertical {@code anchor} saying what {@code y} means:
     * {@link VerticalAnchor#BASELINE} (the historical default),
     * {@link VerticalAnchor#TOP} (text hangs from {@code y} — the baseline lands
     * {@code ascent * size} below it, matching legacy fixed-position layout),
     * {@link VerticalAnchor#BOTTOM} (the descender line rests on {@code y}), or
     * the layout line-box variants {@link VerticalAnchor#LINE_TOP}/
     * {@link VerticalAnchor#LINE_BOTTOM}. Ascent/descent come from the selected
     * font's metrics. Returns whether the page (and font) existed.
     */
    public boolean placeText(int pageIndex, double x, double y, String text, double size,
                             double r, double g, double b, double rotationDeg, Align align,
                             int fontId, VerticalAnchor anchor) {
        IntByReference found = new IntByReference();
        Pdf.check(FFI.C.pdf_editable_place_text_anchored(
                h(), pageIndex, x, y, text, size, r, g, b, rotationDeg,
                align.code, anchor.code, fontId, found));
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
        return maskedText(pageIndex, x, y, width, height, text, size, textColor, bgColor,
                align, -1, VerticalAlign.MIDDLE);
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
     * Like {@link #maskedText(int, double, double, double, double, String, double, double[], double[], Align)}
     * but with an embedded font ({@code fontId} from
     * {@link #addFontFile(String)}/{@link #addFont(byte[])}; {@code -1} = built-in
     * Helvetica) and an explicit vertical alignment of the line inside the box:
     * {@link VerticalAlign#MIDDLE} (the historical cap-height centering),
     * {@link VerticalAlign#TOP} (line hangs from the top edge — baseline at
     * {@code y + height - ascent * size}, top line-alignment in rectangle-based text APIs
     * semantics), or {@link VerticalAlign#BOTTOM} (descender line rests on the
     * bottom edge). Returns whether the page (and font) existed.
     */
    public boolean maskedText(int pageIndex, double x, double y, double width, double height,
                              String text, double size,
                              double[] textColor, double[] bgColor, Align align,
                              int fontId, VerticalAlign valign) {
        return maskedText(pageIndex, x, y, width, height, text, size, textColor, bgColor,
                align, fontId, valign, -1.0);
    }

    /**
     * Like {@link #maskedText(int, double, double, double, double, String, double, double[], double[], Align, int, VerticalAlign)}
     * but with an explicit horizontal edge inset {@code padding} (points) for
     * {@link Align#LEFT}/{@link Align#RIGHT}: text starts at {@code x + padding}
     * (or ends at {@code x + width - padding}). Pass a negative value to keep the
     * historical default {@code min(0.15 * size, width / 4)}; {@code 0} starts
     * flush with the box edge like rectangle-based DrawString APIs.
     */
    public boolean maskedText(int pageIndex, double x, double y, double width, double height,
                              String text, double size,
                              double[] textColor, double[] bgColor, Align align,
                              int fontId, VerticalAlign valign, double padding) {
        double[] tc = textColor == null ? new double[] {0.0, 0.0, 0.0} : textColor;
        double[] bc = bgColor == null ? new double[] {1.0, 1.0, 1.0} : bgColor;
        IntByReference found = new IntByReference();
        Pdf.check(FFI.C.pdf_editable_masked_text_pad(
                h(), pageIndex, x, y, width, height, text, size,
                tc[0], tc[1], tc[2], bc[0], bc[1], bc[2],
                align.code, valign.code, padding, fontId, found));
        return found.getValue() != 0;
    }

    /**
     * Register a TrueType/OpenType font (from a file path) for text stamping;
     * returns a {@code fontId} usable with the {@code fontId} parameter of
     * {@link #placeText}, {@link #maskedText} and {@link #placeParagraph}. The
     * font is embedded as a subset — stamped text renders with the real font's
     * glyphs and metrics, exactly like {@link Document#addFontFile} +
     * {@code showText}.
     */
    public int addFontFile(String path) {
        IntByReference id = new IntByReference();
        Pdf.check(FFI.C.pdf_editable_add_font_file(h(), path, id));
        return id.getValue();
    }

    /**
     * Register a stamping font from raw TrueType/OpenType bytes — see
     * {@link #addFontFile(String)}.
     */
    public int addFont(byte[] data) {
        IntByReference id = new IntByReference();
        Pdf.check(FFI.C.pdf_editable_add_font(h(), data, data.length, id));
        return id.getValue();
    }

    /**
     * Choose the coordinate space of the positioned stamping primitives
     * ({@link #fillRect}, {@link #placeText}, {@link #maskedText},
     * {@link #placeParagraph}, {@link #drawImage}) for subsequent calls.
     * {@link StampSpace#VISIBLE} (the default) keeps the historical behavior —
     * coordinates in the page's displayed space, compensating {@code /Rotate}.
     * {@link StampSpace#MEDIA} interprets coordinates and {@code rotationDeg} in
     * the raw PDF user space (legacy layout semantics), never composing with the page's
     * {@code /Rotate} — use it to reproduce legacy-engine placement on rotated/scanned
     * pages. Watermarks and redaction are unaffected.
     */
    public EditableDoc setStampSpace(StampSpace space) {
        Pdf.check(FFI.C.pdf_editable_set_stamp_space(h(), space.code));
        return this;
    }

    /**
     * Stamp a <b>paragraph with automatic word wrapping</b> on page
     * {@code pageIndex}: {@code text} is broken into lines that fit {@code width}
     * points and drawn from the top-left corner {@code (x, y)} downward (first
     * baseline at {@code y - ascent * size}, legacy fixed-position layout
     * semantics; {@code '\n'} forces a break), 12&nbsp;pt black Helvetica,
     * left-aligned. Returns whether the page existed and the box was valid.
     */
    public boolean placeParagraph(int pageIndex, double x, double y, double width, String text) {
        return placeParagraph(pageIndex, x, y, width, text, 12.0, 0.0, 0.0, 0.0,
                Align.LEFT, -1, 0.0, 1.0, VerticalAnchor.TOP, 0.0);
    }

    /**
     * Stamp a <b>paragraph with automatic word wrapping</b> on page
     * {@code pageIndex} (0-based): {@code text} is broken into lines that fit
     * {@code width} points (greedy, by word; {@code '\n'} forces a break) at
     * {@code size} points in RGB ({@code r}/{@code g}/{@code b}, each 0..=1).
     * {@code align} lays lines out inside {@code [x, x+width]}
     * ({@link Align#JUSTIFY} stretches the word gaps of every line but the last
     * of each paragraph). Pass {@code fontId} from
     * {@link #addFontFile(String)}/{@link #addFont(byte[])} to wrap and draw with
     * an embedded font (its real metrics drive the break points); {@code -1}
     * uses the built-in Helvetica. {@code maxHeight > 0} is a ceiling that cuts
     * overflowing lines ({@code <= 0} = unlimited); {@code lineHeight} scales the
     * default {@code 1.2 * size} baseline-to-baseline leading ({@code <= 0} =
     * {@code 1.0}). The {@code anchor} says what {@code y} means for the block:
     * {@link VerticalAnchor#TOP} (default) — top of the box;
     * {@link VerticalAnchor#BASELINE} — the first line's baseline;
     * {@link VerticalAnchor#BOTTOM}/{@link VerticalAnchor#LINE_BOTTOM} — legacy layout engines
     * {@code fixed-position layout}: {@code y} is the element's <em>bottom</em> (with
     * {@code maxHeight} the box is {@code [y, y+maxHeight]}, text flows from its
     * top and lines crossing below {@code y} are cut; without it the wrapped
     * block's bottom rests on {@code y}). {@code rotationDeg} rotates the
     * laid-out block counter-clockwise about the anchor {@code (x, y)}. Returns
     * whether the page (and font) existed and the box was valid.
     */
    public boolean placeParagraph(int pageIndex, double x, double y, double width, String text,
                                  double size, double r, double g, double b, Align align,
                                  int fontId, double maxHeight, double lineHeight,
                                  VerticalAnchor anchor, double rotationDeg) {
        IntByReference found = new IntByReference();
        Pdf.check(FFI.C.pdf_editable_place_paragraph_anchored(
                h(), pageIndex, x, y, width, text, size, r, g, b,
                align.code, anchor.code, fontId, maxHeight, lineHeight, rotationDeg,
                null, null, found));
        return found.getValue() != 0;
    }

    /**
     * Like {@link #placeParagraph(int, double, double, double, String, double, double, double, double, Align, int, double, double, VerticalAnchor, double)}
     * but returns a {@link PlaceParagraphResult} with the number of lines
     * actually drawn (detect {@code maxHeight} truncation) and the consumed
     * block height in points (stack blocks without re-measuring).
     */
    public PlaceParagraphResult placeParagraphMeasured(
            int pageIndex, double x, double y, double width, String text,
            double size, double r, double g, double b, Align align,
            int fontId, double maxHeight, double lineHeight,
            VerticalAnchor anchor, double rotationDeg) {
        DoubleByReference height = new DoubleByReference();
        IntByReference lines = new IntByReference();
        IntByReference found = new IntByReference();
        Pdf.check(FFI.C.pdf_editable_place_paragraph_anchored(
                h(), pageIndex, x, y, width, text, size, r, g, b,
                align.code, anchor.code, fontId, maxHeight, lineHeight, rotationDeg,
                height, lines, found));
        return new PlaceParagraphResult(lines.getValue(), height.getValue());
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
        return drawImage(pageIndex, image, x, y, width, height, rotationDeg, ImageAnchor.CORNER);
    }

    /**
     * Like {@link #drawImage(int, byte[], double, double, double, double, double)}
     * but with an explicit rotation {@code anchor}: {@link ImageAnchor#CORNER}
     * (the default) rotates the image about its own lower-left corner at
     * {@code (x, y)}; {@link ImageAnchor#BOUNDING_BOX} lands the <em>rotated
     * image's bounding box</em> with its lower-left at {@code (x, y)} (legacy layout engines
     * layout semantics — e.g. a 90° image occupies
     * {@code [x, x+height] x [y, y+width]}).
     */
    public boolean drawImage(int pageIndex, byte[] image, double x, double y,
                             double width, double height, double rotationDeg,
                             ImageAnchor anchor) {
        IntByReference found = new IntByReference();
        Pdf.check(FFI.C.pdf_editable_draw_image_anchored(
                h(), pageIndex, image, image.length, x, y, width, height,
                rotationDeg, anchor.code, found));
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

    /** Convert the loaded document to PDF/A. Only B-levels (A1B/A2B/A3B) are valid. */
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

    /** Encrypt on save. */
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
