package dev.rustpdf;

import com.sun.jna.Pointer;
import com.sun.jna.StringArray;
import com.sun.jna.ptr.IntByReference;

import java.util.List;

/**
 * A PDF document being authored. Use with try-with-resources so the native
 * handle is freed:
 *
 * <pre>{@code
 * try (Document doc = new Document()) {
 *     int f = doc.addFontFile("Roboto-Regular.ttf");
 *     doc.addPage().showText(f, 24, 72, 740, "Hello");
 *     byte[] bytes = doc.toBytes();
 * }
 * }</pre>
 */
public final class Document implements AutoCloseable {
    private Pointer h;

    public Document() {
        h = FFI.C.pdf_document_new();
        if (h == null) {
            throw new PdfException(1, "pdf_document_new returned NULL");
        }
    }

    private Pointer h() {
        if (h == null) {
            throw new IllegalStateException("operation on a closed Document");
        }
        return h;
    }

    @Override
    public void close() {
        if (h != null) {
            FFI.C.pdf_document_free(h);
            h = null;
        }
    }

    // ---- configuration ------------------------------------------------------

    /** Emit PDF/A-2b. Requires a license. */
    public Document pdfa() {
        Pdf.check(FFI.C.pdf_document_pdfa(h()));
        return this;
    }

    /** Emit PDF/A at the given level. Requires a license. */
    public Document pdfa(PdfaLevel level) {
        Pdf.check(FFI.C.pdf_document_pdfa_level(h(), level.code));
        return this;
    }

    /** Enable the tagged/accessible structure tree. Requires a license. */
    public Document tagged() {
        Pdf.check(FFI.C.pdf_document_tagged(h()));
        return this;
    }

    /** Set the PDF version (0 = 1.4, 1 = 1.5, 2 = 1.7, 3 = 2.0). */
    public Document setVersion(int v) {
        Pdf.check(FFI.C.pdf_document_set_version(h(), v));
        return this;
    }

    public Document setDefaultSize(double width, double height) {
        Pdf.check(FFI.C.pdf_document_set_default_size(h(), width, height));
        return this;
    }

    public Document setInfo(String title, String author, String subject, String keywords, String creator) {
        Pdf.check(FFI.C.pdf_document_set_info(h(), title, author, subject, keywords, creator));
        return this;
    }

    /** Convenience: set just the title. */
    public Document setTitle(String title) {
        return setInfo(title, null, null, null, null);
    }

    // ---- pages + graphics ---------------------------------------------------

    public Document addPage() {
        Pdf.check(FFI.C.pdf_document_add_page(h()));
        return this;
    }

    public Document addPage(double width, double height) {
        Pdf.check(FFI.C.pdf_document_add_page_sized(h(), width, height));
        return this;
    }

    public Document setFillRgb(double r, double g, double b) {
        Pdf.check(FFI.C.pdf_page_set_fill_rgb(h(), r, g, b));
        return this;
    }

    public Document setStrokeRgb(double r, double g, double b) {
        Pdf.check(FFI.C.pdf_page_set_stroke_rgb(h(), r, g, b));
        return this;
    }

    public Document setLineWidth(double w) {
        Pdf.check(FFI.C.pdf_page_set_line_width(h(), w));
        return this;
    }

    public Document rect(double x, double y, double w, double h) {
        Pdf.check(FFI.C.pdf_page_rect(h(), x, y, w, h));
        return this;
    }

    public Document fill() {
        Pdf.check(FFI.C.pdf_page_fill(h()));
        return this;
    }

    public Document stroke() {
        Pdf.check(FFI.C.pdf_page_stroke(h()));
        return this;
    }

    // ---- fonts + text -------------------------------------------------------

    /** Register a font from a file; returns its id. */
    public int addFontFile(String path) {
        IntByReference id = new IntByReference();
        Pdf.check(FFI.C.pdf_document_add_font_file(h(), path, id));
        return id.getValue();
    }

    /** Register a font from TrueType/OpenType bytes; returns its id. */
    public int addFont(byte[] data) {
        IntByReference id = new IntByReference();
        Pdf.check(FFI.C.pdf_document_add_font(h(), data, data.length, id));
        return id.getValue();
    }

    /** Show a line of text at the baseline {@code (x, y)}. */
    public Document showText(int font, double size, double x, double y, String text) {
        return showText(font, size, x, y, text, 0);
    }

    /**
     * Show a line of text. {@code headingLevel} 1..6 tags it as H1..H6 (when the
     * document is tagged); 0 leaves it as a paragraph.
     */
    public Document showText(int font, double size, double x, double y, String text, int headingLevel) {
        Pdf.check(FFI.C.pdf_page_show_text(h(), font, size, x, y, text, headingLevel));
        return this;
    }

    /** Lay out a wrapping paragraph in the box at {@code (x, y, width)}. */
    public Document paragraph(int font, double size, double x, double y, double width, String text, Align align) {
        Pdf.check(FFI.C.pdf_page_paragraph(h(), font, size, x, y, width, align.code, text));
        return this;
    }

    // ---- images -------------------------------------------------------------

    public int addImageFile(String path) {
        IntByReference id = new IntByReference();
        Pdf.check(FFI.C.pdf_document_add_image_file(h(), path, id));
        return id.getValue();
    }

    public int addImagePng(byte[] data) {
        IntByReference id = new IntByReference();
        Pdf.check(FFI.C.pdf_document_add_image_png(h(), data, data.length, id));
        return id.getValue();
    }

    public int addImageJpeg(byte[] data) {
        IntByReference id = new IntByReference();
        Pdf.check(FFI.C.pdf_document_add_image_jpeg(h(), data, data.length, id));
        return id.getValue();
    }

    public Document drawImage(int image, double x, double y, double w, double h) {
        Pdf.check(FFI.C.pdf_page_draw_image(h(), image, x, y, w, h));
        return this;
    }

    /** Draw a meaningful image (tagged {@code /Figure} with alternate text). */
    public Document figure(int image, double x, double y, double w, double h, String alt) {
        Pdf.check(FFI.C.pdf_page_figure(h(), image, x, y, w, h, alt));
        return this;
    }

    // ---- attachments + forms ------------------------------------------------

    public Document attachFile(String name, String mime, byte[] data,
                               AFRelationship relationship, String description) {
        Pdf.check(FFI.C.pdf_document_attach_file(
                h(), name, mime, data, data.length, relationship.code, description));
        return this;
    }

    /** Add a text field. {@code rect} = {x0, y0, x1, y1}; {@code size} 0 = auto. */
    public Document textField(String name, int page, double[] rect, String value, double size) {
        Pdf.check(FFI.C.pdf_document_text_field(
                h(), name, page, rect[0], rect[1], rect[2], rect[3], value, size));
        return this;
    }

    public Document checkbox(String name, int page, double[] rect, boolean checked) {
        Pdf.check(FFI.C.pdf_document_checkbox(
                h(), name, page, rect[0], rect[1], rect[2], rect[3], checked ? 1 : 0));
        return this;
    }

    /** Add a dropdown. {@code selected} is the 0-based index, or -1 for none. */
    public Document dropdown(String name, int page, double[] rect, List<String> options, int selected, double size) {
        Pdf.check(FFI.C.pdf_document_dropdown(
                h(), name, page, rect[0], rect[1], rect[2], rect[3],
                String.join("\n", options), selected, size));
        return this;
    }

    /**
     * Add a radio-button group. {@code rects[i]} = {x0, y0, x1, y1} and
     * {@code exports[i]} is that button's export value; {@code selected} 0-based or -1.
     */
    public Document radioGroup(String name, int page, double[][] rects, String[] exports, int selected) {
        double[] flat = new double[rects.length * 4];
        for (int i = 0; i < rects.length; i++) {
            flat[i * 4] = rects[i][0];
            flat[i * 4 + 1] = rects[i][1];
            flat[i * 4 + 2] = rects[i][2];
            flat[i * 4 + 3] = rects[i][3];
        }
        Pdf.check(FFI.C.pdf_document_radio_group(
                h(), name, page, rects.length, flat, new StringArray(exports, "UTF-8"), selected));
        return this;
    }

    // ---- hyperlinks (Tier 1) ------------------------------------------------

    /** Add a clickable URI link over {@code rect} = {x0, y0, x1, y1}. */
    public Document linkUri(double[] rect, String uri) {
        Pdf.check(FFI.C.pdf_page_link_uri(h(), rect[0], rect[1], rect[2], rect[3], uri));
        return this;
    }

    /** Add an internal link over {@code rect} jumping to {@code pageIndex} (page top). */
    public Document linkToPage(double[] rect, int pageIndex) {
        Pdf.check(FFI.C.pdf_page_link_to_page(
                h(), rect[0], rect[1], rect[2], rect[3], pageIndex, 0.0, 0));
        return this;
    }

    /** Add an internal link over {@code rect} jumping to {@code pageIndex} at vertical {@code top}. */
    public Document linkToPage(double[] rect, int pageIndex, double top) {
        Pdf.check(FFI.C.pdf_page_link_to_page(
                h(), rect[0], rect[1], rect[2], rect[3], pageIndex, top, 1));
        return this;
    }

    // ---- bookmarks / outline (Tier 1) ---------------------------------------

    /**
     * Append a bookmark tree to the document outline. The tree is flattened in
     * pre-order (root at level 0) and added in a single native call.
     */
    public Document addBookmark(Bookmark bookmark) {
        java.util.List<Bookmark> nodes = new java.util.ArrayList<>();
        java.util.List<Integer> levelList = new java.util.ArrayList<>();
        bookmark.flatten(0, nodes, levelList);
        int n = nodes.size();
        int[] levels = new int[n];
        String[] titles = new String[n];
        long[] pages = new long[n];
        double[] tops = new double[n];
        int[] hasTops = new int[n];
        for (int i = 0; i < n; i++) {
            Bookmark b = nodes.get(i);
            levels[i] = levelList.get(i);
            titles[i] = b.title;
            pages[i] = b.page;
            if (b.top == null) {
                hasTops[i] = 0;
                tops[i] = 0.0;
            } else {
                hasTops[i] = 1;
                tops[i] = b.top;
            }
        }
        Pdf.check(FFI.C.pdf_document_add_bookmarks(
                h(), n, levels, new StringArray(titles, "UTF-8"), pages, tops, hasTops));
        return this;
    }

    // ---- ZUGFeRD / Factur-X (Tier 2) ----------------------------------------

    /** Embed a Factur-X / ZUGFeRD e-invoice XML at the given profile. Requires a license. */
    public Document facturx(byte[] xml, FacturxProfile profile) {
        Pdf.check(FFI.C.pdf_document_facturx(h(), xml, xml.length, profile.code));
        return this;
    }

    // ---- output -------------------------------------------------------------

    public int pageCount() {
        return FFI.C.pdf_document_page_count(h());
    }

    public byte[] toBytes() {
        return Pdf.takeBuffer((p, n) -> FFI.C.pdf_document_write(h(), p, n));
    }

    public void save(String path) {
        Pdf.check(FFI.C.pdf_document_save(h(), path));
    }
}
