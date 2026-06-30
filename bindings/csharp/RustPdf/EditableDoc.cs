using System.Text;

namespace RustPdf;

/// <summary>An existing PDF loaded for manipulation. Dispose to free native memory.</summary>
public sealed class EditableDoc : IDisposable
{
    private IntPtr _h;

    private EditableDoc(IntPtr handle)
    {
        if (handle == IntPtr.Zero)
            throw new PdfException(6, Pdf.LastError());
        _h = handle;
    }

    /// <summary>Load and parse a PDF from bytes (optionally with a password).</summary>
    public static EditableDoc Load(byte[] data, string? password = null)
    {
        Native.Init();
        var h = password is null
            ? Native.pdf_editable_load(data, (nuint)data.Length)
            : Native.pdf_editable_load_password(data, (nuint)data.Length, password);
        return new EditableDoc(h);
    }

    public static EditableDoc LoadFile(string path, string? password = null)
        => Load(File.ReadAllBytes(path), password);

    private IntPtr H => _h != IntPtr.Zero
        ? _h
        : throw new ObjectDisposedException(nameof(EditableDoc));

    public void Dispose()
    {
        if (_h != IntPtr.Zero)
        {
            Native.pdf_editable_free(_h);
            _h = IntPtr.Zero;
        }
        GC.SuppressFinalize(this);
    }

    ~EditableDoc() => Dispose();

    public int PageCount => Native.pdf_editable_page_count(H);

    public EditableDoc Merge(EditableDoc other)
    {
        Pdf.Check(Native.pdf_editable_merge(H, other.H));
        return this;
    }

    public EditableDoc RotatePage(int index, int degrees)
    {
        Pdf.Check(Native.pdf_editable_rotate_page(H, (nuint)index, degrees));
        return this;
    }

    public EditableDoc DeletePage(int index)
    {
        Pdf.Check(Native.pdf_editable_delete_page(H, (nuint)index));
        return this;
    }

    public EditableDoc ReorderPages(IReadOnlyList<int> order)
    {
        var arr = new nuint[order.Count];
        for (int i = 0; i < order.Count; i++)
            arr[i] = (nuint)order[i];
        Pdf.Check(Native.pdf_editable_reorder_pages(H, arr, (nuint)arr.Length));
        return this;
    }

    /// <summary>Extract the given page indices into a new document.</summary>
    public EditableDoc ExtractPages(IReadOnlyList<int> indices)
    {
        var arr = new nuint[indices.Count];
        for (int i = 0; i < indices.Count; i++)
            arr[i] = (nuint)indices[i];
        Pdf.Check(Native.pdf_editable_extract_pages(H, arr, (nuint)arr.Length, out var outEd));
        return new EditableDoc(outEd);
    }

    public EditableDoc SetInfo(string key, string value)
    {
        Pdf.Check(Native.pdf_editable_set_info(H, key, value));
        return this;
    }

    public string GetInfo(string key)
    {
        var bytes = Pdf.TakeBuffer((out IntPtr p, out nuint n) => Native.pdf_editable_get_info(H, key, out p, out n));
        return Encoding.UTF8.GetString(bytes);
    }

    public EditableDoc SetXmp(byte[] xml)
    {
        Pdf.Check(Native.pdf_editable_set_xmp(H, xml, (nuint)xml.Length));
        return this;
    }

    public EditableDoc OverlayPage(int index, byte[] content)
    {
        Pdf.Check(Native.pdf_editable_overlay_page(H, (nuint)index, content, (nuint)content.Length));
        return this;
    }

    /// <summary>Fill an AcroForm text field; returns whether it existed.</summary>
    public bool FillTextField(string name, string value)
    {
        Pdf.Check(Native.pdf_editable_fill_text_field(H, name, value, out int found));
        return found != 0;
    }

    /// <summary>Set an AcroForm checkbox on/off; returns whether it existed.</summary>
    public bool SetCheckbox(string name, bool checkedFlag = true)
    {
        Pdf.Check(Native.pdf_editable_set_checkbox(H, name, checkedFlag ? 1 : 0, out int found));
        return found != 0;
    }

    /// <summary>Select a radio button by export value; returns whether it existed.</summary>
    public bool SetRadio(string name, string exportValue)
    {
        Pdf.Check(Native.pdf_editable_set_radio(H, name, exportValue, out int found));
        return found != 0;
    }

    /// <summary>Set a choice (list/combo) field's value; returns whether it existed.</summary>
    public bool SetChoice(string name, string value)
    {
        Pdf.Check(Native.pdf_editable_set_choice(H, name, value, out int found));
        return found != 0;
    }

    /// <summary>Flatten all AcroForm fields into page content (non-editable).</summary>
    public EditableDoc FlattenForms()
    {
        Pdf.Check(Native.pdf_editable_flatten_forms(H));
        return this;
    }

    /// <summary>List every AcroForm field's fully-qualified name.</summary>
    public IReadOnlyList<string> FieldNames()
    {
        var bytes = Pdf.TakeBuffer((out IntPtr p, out nuint n) => Native.pdf_editable_field_names(H, out p, out n));
        var text = Encoding.UTF8.GetString(bytes);
        return text.Split('\n', StringSplitOptions.RemoveEmptyEntries);
    }

    /// <summary>Stamp diagonal text across every page as a watermark.
    /// <paramref name="opaqueBackground"/> draws an opaque box behind the text
    /// (e.g. a redaction-style banner) instead of overlaying transparently.</summary>
    public EditableDoc WatermarkText(string text, double size = 64.0,
        (double R, double G, double B)? color = null, double opacity = 0.30, double rotationDeg = 45.0,
        bool opaqueBackground = false)
    {
        var (r, g, b) = color ?? (0.5, 0.5, 0.5);
        Pdf.Check(Native.pdf_editable_watermark_text(
            H, text, size, r, g, b, opacity, rotationDeg, opaqueBackground ? 1 : 0));
        return this;
    }

    /// <summary>Stamp an image file across every page as a watermark, rotated
    /// <paramref name="rotationDeg"/> degrees.</summary>
    public EditableDoc WatermarkImageFile(string path, double width, double height,
        double opacity = 0.30, double rotationDeg = 0.0)
    {
        Pdf.Check(Native.pdf_editable_watermark_image_file(H, path, width, height, opacity, rotationDeg));
        return this;
    }

    /// <summary>Paint a filled rectangle at <paramref name="x"/>,<paramref name="y"/>
    /// (size <paramref name="width"/>×<paramref name="height"/>) on page
    /// <paramref name="pageIndex"/>, in RGB <paramref name="color"/> (default opaque
    /// white) at <paramref name="opacity"/>. Coordinates are in the page's visible
    /// space (origin lower-left, y up). Returns whether the page existed. The
    /// common use is masking a placeholder with an opaque white box.</summary>
    public bool FillRect(int pageIndex, double x, double y, double width, double height,
        (double R, double G, double B)? color = null, double opacity = 1.0)
    {
        var (r, g, b) = color ?? (1.0, 1.0, 1.0);
        Pdf.Check(Native.pdf_editable_fill_rect(
            H, pageIndex, x, y, width, height, r, g, b, opacity, out int found));
        return found != 0;
    }

    /// <summary>Draw a line of positioned text with baseline at
    /// <paramref name="x"/>,<paramref name="y"/> on page <paramref name="pageIndex"/>,
    /// using standard Helvetica at <paramref name="size"/> points in RGB
    /// <paramref name="color"/>. <paramref name="rotationDeg"/> rotates the text
    /// counter-clockwise about its anchor (match the page rotation to follow a
    /// rotated page). Coordinates are in the page's visible space. Returns whether
    /// the page existed.</summary>
    public bool PlaceText(int pageIndex, double x, double y, string text, double size = 12.0,
        (double R, double G, double B)? color = null, double rotationDeg = 0.0,
        Align align = Align.Left)
    {
        var (r, g, b) = color ?? (0.0, 0.0, 0.0);
        Pdf.Check(Native.pdf_editable_place_text_aligned(
            H, pageIndex, x, y, text, size, r, g, b, rotationDeg, (int)align, out int found));
        return found != 0;
    }

    /// <summary>Draw <paramref name="text"/> over an opaque background box
    /// <c>[x, y, x+width, y+height]</c>: fills the box in <paramref name="bgColor"/>,
    /// then writes the text (standard Helvetica, <paramref name="size"/> points,
    /// <paramref name="textColor"/>) horizontally aligned per <paramref name="align"/>
    /// and vertically centered within the box. The classic use is masking a
    /// placeholder and stamping the real value over it without hand-computing the
    /// baseline. Coordinates are in the page's visible space (origin lower-left, y up).
    /// Returns whether the page existed.</summary>
    public bool MaskedText(int pageIndex, double x, double y, double width, double height,
        string text, double size = 12.0,
        (double R, double G, double B)? textColor = null,
        (double R, double G, double B)? bgColor = null,
        Align align = Align.Left)
    {
        var (tr, tg, tb) = textColor ?? (0.0, 0.0, 0.0);
        var (br, bg, bb) = bgColor ?? (1.0, 1.0, 1.0);
        Pdf.Check(Native.pdf_editable_masked_text(
            H, pageIndex, x, y, width, height, text, size,
            tr, tg, tb, br, bg, bb, (int)align, out int found));
        return found != 0;
    }

    /// <summary>Draw an image (PNG or JPEG bytes — dispatched on the file
    /// signature) onto page <paramref name="pageIndex"/> with its lower-left corner
    /// at <paramref name="x"/>,<paramref name="y"/>, scaled to
    /// <paramref name="width"/>×<paramref name="height"/> points and rotated
    /// <paramref name="rotationDeg"/> degrees counter-clockwise about that corner.
    /// Coordinates are in the page's visible space (origin lower-left, honoring
    /// <c>/Rotate</c>). Returns whether the page existed.</summary>
    public bool DrawImage(int pageIndex, byte[] image, double x, double y,
        double width, double height, double rotationDeg = 0.0)
    {
        Pdf.Check(Native.pdf_editable_draw_image(
            H, pageIndex, image, (nuint)image.Length, x, y, width, height,
            rotationDeg, out int found));
        return found != 0;
    }

    /// <summary>Set the output PDF version (0 = 1.4, 1 = 1.5, 2 = 1.7, 3 = 2.0).
    /// Clears any catalog <c>/Version</c> override.</summary>
    public EditableDoc SetVersion(int version)
    {
        Pdf.Check(Native.pdf_editable_set_version(H, version));
        return this;
    }

    /// <summary>Strip PDF/A conformance (<c>/OutputIntents</c>, XMP <c>pdfaid</c>,
    /// <c>/Version</c>) so the file is a plain PDF.</summary>
    public EditableDoc StripPdfa()
    {
        Pdf.Check(Native.pdf_editable_strip_pdfa(H));
        return this;
    }

    /// <summary>Normalize to a plain PDF at <paramref name="version"/> (strip
    /// PDF/A + set version). Version codes as in <see cref="SetVersion"/>.</summary>
    public EditableDoc Normalize(int version)
    {
        Pdf.Check(Native.pdf_editable_normalize(H, version));
        return this;
    }

    /// <summary>Redact rectangular regions on a page; returns whether the page existed.
    /// Each rect is <c>(x0, y0, x1, y1)</c>.</summary>
    public bool Redact(int pageIndex, IReadOnlyList<(double, double, double, double)> rects)
    {
        var flat = new double[rects.Count * 4];
        for (int i = 0; i < rects.Count; i++)
        {
            var (x0, y0, x1, y1) = rects[i];
            flat[i * 4] = x0;
            flat[i * 4 + 1] = y0;
            flat[i * 4 + 2] = x1;
            flat[i * 4 + 3] = y1;
        }
        Pdf.Check(Native.pdf_editable_redact(H, (nuint)pageIndex, flat, (nuint)rects.Count, out int found));
        return found != 0;
    }

    /// <summary>Convert the loaded document to PDF/A (B-levels only: A1b/A2b/A3b).
    /// Requires a license.</summary>
    public EditableDoc ConvertToPdfa(PdfaLevel level = PdfaLevel.A2b)
    {
        Pdf.Check(Native.pdf_editable_convert_to_pdfa(H, (int)level));
        return this;
    }

    public EditableDoc Optimize()
    {
        Pdf.Check(Native.pdf_editable_optimize(H));
        return this;
    }

    public EditableDoc Compact(bool on = true)
    {
        Pdf.Check(Native.pdf_editable_compact(H, on ? 1 : 0));
        return this;
    }

    /// <summary>Encrypt on save (requires a license).</summary>
    public EditableDoc Encrypt(string user = "", string owner = "",
        Encryption method = Encryption.Aes256, bool readOnly = false)
    {
        Pdf.Check(Native.pdf_editable_encrypt(H, (int)method, user, owner, readOnly ? 1 : 0));
        return this;
    }

    public byte[] ToBytes() => Pdf.TakeBuffer((out IntPtr p, out nuint n) => Native.pdf_editable_to_bytes(H, out p, out n));

    /// <summary>Serialize as an incremental update over <paramref name="original"/>.</summary>
    public byte[] ToBytesIncremental(byte[] original) => Pdf.TakeBuffer((out IntPtr p, out nuint n) =>
        Native.pdf_editable_to_bytes_incremental(H, original, (nuint)original.Length, out p, out n));

    public void Save(string path) => Pdf.Check(Native.pdf_editable_save(H, path));
}
