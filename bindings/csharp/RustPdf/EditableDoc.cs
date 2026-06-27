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
