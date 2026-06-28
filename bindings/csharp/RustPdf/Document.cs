namespace RustPdf;

/// <summary>A PDF document being authored. Dispose to free native memory.</summary>
public sealed class Document : IDisposable
{
    private IntPtr _h;

    public Document()
    {
        Native.Init();
        _h = Native.pdf_document_new();
        if (_h == IntPtr.Zero)
            throw new PdfException(1, "pdf_document_new returned NULL");
    }

    private IntPtr H => _h != IntPtr.Zero
        ? _h
        : throw new ObjectDisposedException(nameof(Document));

    public void Dispose()
    {
        if (_h != IntPtr.Zero)
        {
            Native.pdf_document_free(_h);
            _h = IntPtr.Zero;
        }
        GC.SuppressFinalize(this);
    }

    ~Document() => Dispose();

    // ---- configuration ------------------------------------------------------

    /// <summary>Emit PDF/A-2b (or the given <paramref name="level"/>). Requires a license.</summary>
    public Document Pdfa(PdfaLevel? level = null)
    {
        Pdf.Check(level is null ? Native.pdf_document_pdfa(H) : Native.pdf_document_pdfa_level(H, (int)level));
        return this;
    }

    /// <summary>Enable the tagged/accessible structure tree. Requires a license.</summary>
    public Document Tagged()
    {
        Pdf.Check(Native.pdf_document_tagged(H));
        return this;
    }

    /// <summary>Set the PDF version (0 = 1.4, 1 = 1.5, 2 = 1.7).</summary>
    public Document SetVersion(int v)
    {
        Pdf.Check(Native.pdf_document_set_version(H, v));
        return this;
    }

    public Document SetDefaultSize(double width, double height)
    {
        Pdf.Check(Native.pdf_document_set_default_size(H, width, height));
        return this;
    }

    public Document SetInfo(string? title = null, string? author = null, string? subject = null,
        string? keywords = null, string? creator = null)
    {
        Pdf.Check(Native.pdf_document_set_info(H, title, author, subject, keywords, creator));
        return this;
    }

    // ---- pages + graphics ---------------------------------------------------

    public Document AddPage((double Width, double Height)? size = null)
    {
        Pdf.Check(size is null
            ? Native.pdf_document_add_page(H)
            : Native.pdf_document_add_page_sized(H, size.Value.Width, size.Value.Height));
        return this;
    }

    public Document SetFillRgb(double r, double g, double b)
    {
        Pdf.Check(Native.pdf_page_set_fill_rgb(H, r, g, b));
        return this;
    }

    public Document SetStrokeRgb(double r, double g, double b)
    {
        Pdf.Check(Native.pdf_page_set_stroke_rgb(H, r, g, b));
        return this;
    }

    public Document SetLineWidth(double w)
    {
        Pdf.Check(Native.pdf_page_set_line_width(H, w));
        return this;
    }

    public Document Rect(double x, double y, double w, double h)
    {
        Pdf.Check(Native.pdf_page_rect(H, x, y, w, h));
        return this;
    }

    public Document Fill()
    {
        Pdf.Check(Native.pdf_page_fill(H));
        return this;
    }

    public Document Stroke()
    {
        Pdf.Check(Native.pdf_page_stroke(H));
        return this;
    }

    // ---- fonts + text -------------------------------------------------------

    /// <summary>Register a font from a file; returns its id.</summary>
    public int AddFontFile(string path)
    {
        Pdf.Check(Native.pdf_document_add_font_file(H, path, out int id));
        return id;
    }

    /// <summary>Register a font from TrueType/OpenType bytes; returns its id.</summary>
    public int AddFont(byte[] data)
    {
        Pdf.Check(Native.pdf_document_add_font(H, data, (nuint)data.Length, out int id));
        return id;
    }

    /// <summary>Show a line of text. <paramref name="headingLevel"/> 1..6 tags it
    /// as H1..H6 (when tagged); 0 leaves it as a paragraph.</summary>
    public Document ShowText(int font, double size, double x, double y, string text, int headingLevel = 0)
    {
        Pdf.Check(Native.pdf_page_show_text(H, font, size, x, y, text, headingLevel));
        return this;
    }

    /// <summary>Lay out a wrapping paragraph in the box at <c>(x, y, width)</c>.</summary>
    public Document Paragraph(int font, double size, double x, double y, double width, string text,
        Align align = Align.Left)
    {
        Pdf.Check(Native.pdf_page_paragraph(H, font, size, x, y, width, (int)align, text));
        return this;
    }

    // ---- images -------------------------------------------------------------

    public int AddImageFile(string path)
    {
        Pdf.Check(Native.pdf_document_add_image_file(H, path, out int id));
        return id;
    }

    public int AddImagePng(byte[] data)
    {
        Pdf.Check(Native.pdf_document_add_image_png(H, data, (nuint)data.Length, out int id));
        return id;
    }

    public int AddImageJpeg(byte[] data)
    {
        Pdf.Check(Native.pdf_document_add_image_jpeg(H, data, (nuint)data.Length, out int id));
        return id;
    }

    public Document DrawImage(int image, double x, double y, double w, double h)
    {
        Pdf.Check(Native.pdf_page_draw_image(H, image, x, y, w, h));
        return this;
    }

    /// <summary>Draw a meaningful image (tagged <c>/Figure</c> with alt text).</summary>
    public Document Figure(int image, double x, double y, double w, double h, string alt)
    {
        Pdf.Check(Native.pdf_page_figure(H, image, x, y, w, h, alt));
        return this;
    }

    // ---- attachments + forms ------------------------------------------------

    public Document AttachFile(string name, string mime, byte[] data,
        AFRelationship relationship = AFRelationship.Source, string description = "")
    {
        Pdf.Check(Native.pdf_document_attach_file(
            H, name, mime, data, (nuint)data.Length, (int)relationship, description));
        return this;
    }

    public Document TextField(string name, int page, (double, double, double, double) rect,
        string value = "", double size = 0)
    {
        var (x0, y0, x1, y1) = rect;
        Pdf.Check(Native.pdf_document_text_field(H, name, (nuint)page, x0, y0, x1, y1, value, size));
        return this;
    }

    public Document Checkbox(string name, int page, (double, double, double, double) rect, bool checkedFlag)
    {
        var (x0, y0, x1, y1) = rect;
        Pdf.Check(Native.pdf_document_checkbox(H, name, (nuint)page, x0, y0, x1, y1, checkedFlag ? 1 : 0));
        return this;
    }

    public Document Dropdown(string name, int page, (double, double, double, double) rect,
        IEnumerable<string> options, int? selected = null, double size = 0)
    {
        var (x0, y0, x1, y1) = rect;
        var joined = string.Join('\n', options);
        Pdf.Check(Native.pdf_document_dropdown(
            H, name, (nuint)page, x0, y0, x1, y1, joined, selected ?? -1, size));
        return this;
    }

    public Document RadioGroup(string name, int page,
        IReadOnlyList<((double, double, double, double) Rect, string Export)> buttons, int? selected = null)
    {
        var rects = new double[buttons.Count * 4];
        var exports = new string[buttons.Count];
        for (int i = 0; i < buttons.Count; i++)
        {
            var (r, e) = buttons[i];
            rects[i * 4] = r.Item1;
            rects[i * 4 + 1] = r.Item2;
            rects[i * 4 + 2] = r.Item3;
            rects[i * 4 + 3] = r.Item4;
            exports[i] = e;
        }
        Pdf.Check(Native.pdf_document_radio_group(
            H, name, (nuint)page, (nuint)buttons.Count, rects, exports, selected ?? -1));
        return this;
    }

    // ---- hyperlinks + bookmarks (Tier 1) ------------------------------------

    /// <summary>Add a clickable link rectangle that opens an external URI.</summary>
    public Document LinkUri((double, double, double, double) rect, string uri)
    {
        var (x0, y0, x1, y1) = rect;
        Pdf.Check(Native.pdf_page_link_uri(H, x0, y0, x1, y1, uri));
        return this;
    }

    /// <summary>Add a clickable link rectangle that jumps to another page
    /// (optionally scrolled to <paramref name="top"/>).</summary>
    public Document LinkToPage((double, double, double, double) rect, int pageIndex, double? top = null)
    {
        var (x0, y0, x1, y1) = rect;
        Pdf.Check(Native.pdf_page_link_to_page(
            H, x0, y0, x1, y1, (nuint)pageIndex, top ?? 0.0, top is null ? 0 : 1));
        return this;
    }

    /// <summary>Append one outline (bookmark) tree to the document. Children are
    /// flattened pre-order and submitted in a single native call.</summary>
    public Document AddBookmark(Bookmark bookmark)
    {
        var entries = new List<(int Level, string Title, int Page, double? Top)>();
        bookmark.Flatten(0, entries);
        int n = entries.Count;
        var levels = new int[n];
        var titles = new string[n];
        var pages = new nuint[n];
        var tops = new double[n];
        var hasTops = new int[n];
        for (int i = 0; i < n; i++)
        {
            var e = entries[i];
            levels[i] = e.Level;
            titles[i] = e.Title;
            pages[i] = (nuint)e.Page;
            hasTops[i] = e.Top is null ? 0 : 1;
            tops[i] = e.Top ?? 0.0;
        }
        Pdf.Check(Native.pdf_document_add_bookmarks(H, (nuint)n, levels, titles, pages, tops, hasTops));
        return this;
    }

    // ---- ZUGFeRD / Factur-X (Tier 2) ----------------------------------------

    /// <summary>Embed a Factur-X/ZUGFeRD invoice XML (PDF/A-3). Requires a license.</summary>
    public Document Facturx(byte[] xml, FacturxProfile profile = FacturxProfile.En16931)
    {
        Pdf.Check(Native.pdf_document_facturx(H, xml, (nuint)xml.Length, (int)profile));
        return this;
    }

    // ---- output -------------------------------------------------------------

    public int PageCount => Native.pdf_document_page_count(H);

    public byte[] ToBytes() => Pdf.TakeBuffer((out IntPtr p, out nuint n) => Native.pdf_document_write(H, out p, out n));

    public void Save(string path) => Pdf.Check(Native.pdf_document_save(H, path));
}
