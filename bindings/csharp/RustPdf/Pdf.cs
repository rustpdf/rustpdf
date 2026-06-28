using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;

namespace RustPdf;

/// <summary>Thrown when a native call returns a non-zero <c>PdfStatus</c>.</summary>
public sealed class PdfException : Exception
{
    public int Status { get; }

    public PdfException(int status, string message)
        : base($"PdfStatus={status}: {message}") => Status = status;
}

/// <summary>PDF/A conformance level (archival profile).</summary>
public enum PdfaLevel
{
    A1b = 0,
    A2b = 1,
    A2a = 2,
    A3b = 3,
    A3a = 4,
}

/// <summary>Paragraph horizontal alignment.</summary>
public enum Align
{
    Left = 0,
    Right = 1,
    Center = 2,
    Justify = 3,
}

/// <summary>Embedded-file relationship (PDF/A-3 <c>/AFRelationship</c>).</summary>
public enum AFRelationship
{
    Source = 0,
    Data = 1,
    Alternative = 2,
    Supplement = 3,
    Unspecified = 4,
}

/// <summary>Encryption cipher.</summary>
public enum Encryption
{
    Rc4 = 0,
    Aes128 = 1,
    Aes256 = 2,
}

/// <summary>ZUGFeRD / Factur-X conformance profile.</summary>
public enum FacturxProfile
{
    Minimum = 0,
    BasicWL = 1,
    Basic = 2,
    En16931 = 3,
    Extended = 4,
}

/// <summary>A document outline entry. Nest with <see cref="Child"/> to build a tree.</summary>
public sealed class Bookmark
{
    public string Title { get; }
    public int Page { get; }
    public double? Top { get; }
    public List<Bookmark> Children { get; }

    public Bookmark(string title, int page, double? top = null, IEnumerable<Bookmark>? children = null)
    {
        Title = title;
        Page = page;
        Top = top;
        Children = children is null ? new List<Bookmark>() : new List<Bookmark>(children);
    }

    /// <summary>Append a nested child and return this bookmark (for chaining).</summary>
    public Bookmark Child(Bookmark bookmark)
    {
        Children.Add(bookmark);
        return this;
    }

    internal void Flatten(int level, List<(int Level, string Title, int Page, double? Top)> outList)
    {
        outList.Add((level, Title, Page, Top));
        foreach (var c in Children)
            c.Flatten(level + 1, outList);
    }
}

/// <summary>The validation result for a single signature in a PDF.</summary>
public sealed record SignatureInfo(
    string? FieldName,
    string SubFilter,
    string? Signer,
    bool CoversWholeDocument,
    bool DigestValid,
    bool SignatureValid,
    bool IsValid,
    int[] ByteRange);

/// <summary>Top-level helpers: version, licensing, extraction and signing.</summary>
public static class Pdf
{
    internal delegate int OutBuf(out IntPtr ptr, out nuint len);

    /// <summary>Native library version string.</summary>
    public static string Version()
    {
        Native.Init();
        return Marshal.PtrToStringUTF8(Native.pdf_version()) ?? "";
    }

    /// <summary>Activate a license token (unlocks PDF/A, signing, encryption,
    /// accessibility). Tokens may also be supplied via the <c>RUSTPDF_LICENSE</c>
    /// or <c>RUSTPDF_LICENSE_FILE</c> environment variables (auto-activated).</summary>
    /// <exception cref="PdfException">if the token is forged, expired or malformed.</exception>
    public static void ActivateLicense(string token)
    {
        Native.Init();
        Check(Native.pdf_activate_license(token));
    }

    /// <summary>Extract a document's text (Unicode via <c>ToUnicode</c>).</summary>
    public static string ExtractText(byte[] pdf)
    {
        var bytes = TakeBuffer((out IntPtr p, out nuint n) =>
            Native.pdf_extract_text(pdf, (nuint)pdf.Length, out p, out n));
        return Encoding.UTF8.GetString(bytes);
    }

    /// <summary>Extract every raster image from <paramref name="pdf"/> into
    /// <paramref name="outDir"/> (JPEG verbatim as <c>.jpg</c>, everything else as
    /// <c>.png</c>, named <c>page{N}_{name}.{ext}</c>). Returns the number written.</summary>
    public static int ExtractImagesToDir(byte[] pdf, string outDir)
    {
        Native.Init();
        Check(Native.pdf_extract_images_to_dir(pdf, (nuint)pdf.Length, outDir, out var count));
        return (int)count;
    }

    /// <summary>
    /// Render page <paramref name="pageIndex"/> (0-based) of <paramref name="pdf"/>
    /// to a PNG image at <paramref name="dpi"/> dots-per-inch. Page rendering is a
    /// licensed Pro feature: throws <see cref="PdfException"/> with
    /// <c>PdfStatus.License</c> unless a license granting it is active.
    /// </summary>
    public static byte[] RenderPageToPng(byte[] pdf, int pageIndex = 0, double dpi = 150.0)
    {
        return TakeBuffer((out IntPtr p, out nuint n) =>
            Native.pdf_render_page_to_png(pdf, (nuint)pdf.Length, (nuint)pageIndex, dpi, out p, out n));
    }

    /// <summary>Number of pages in <paramref name="pdf"/> (free — no license required).</summary>
    public static int PageCount(byte[] pdf)
    {
        Native.Init();
        Check(Native.pdf_page_count(pdf, (nuint)pdf.Length, out var count));
        return (int)count;
    }

    /// <summary>Sign <paramref name="pdf"/> (PKCS#7 detached, incremental update).
    /// <paramref name="pades"/> selects PAdES-B-B. Requires a license.</summary>
    public static byte[] Sign(byte[] pdf, byte[] keyDer, byte[] certDer,
        string? reason = null, string? location = null, string? name = null, bool pades = false)
    {
        return TakeBuffer((out IntPtr p, out nuint n) => Native.pdf_sign(
            pdf, (nuint)pdf.Length, keyDer, (nuint)keyDer.Length, certDer, (nuint)certDer.Length,
            reason, location, name, pades ? 1 : 0, out p, out n));
    }

    /// <summary>Append a document timestamp (<c>/DocTimeStamp</c>, PAdES-B-LTA).</summary>
    public static byte[] Timestamp(byte[] pdf, byte[] tsaKeyDer, byte[] tsaCertDer, string? date = null)
    {
        return TakeBuffer((out IntPtr p, out nuint n) => Native.pdf_timestamp(
            pdf, (nuint)pdf.Length, tsaKeyDer, (nuint)tsaKeyDer.Length,
            tsaCertDer, (nuint)tsaCertDer.Length, date, out p, out n));
    }

    /// <summary>Append a Document Security Store (<c>/DSS</c>, PAdES-B-LT).</summary>
    public static byte[] AddDss(byte[] pdf, IEnumerable<byte[]>? certs = null, IEnumerable<byte[]>? crls = null)
    {
        var certList = certs?.ToArray() ?? Array.Empty<byte[]>();
        var crlList = crls?.ToArray() ?? Array.Empty<byte[]>();
        var handles = new List<GCHandle>();
        try
        {
            var (cp, cl) = Pin(certList, handles);
            var (rp, rl) = Pin(crlList, handles);
            return TakeBuffer((out IntPtr p, out nuint n) => Native.pdf_add_dss(
                pdf, (nuint)pdf.Length, cp, cl, (nuint)certList.Length,
                rp, rl, (nuint)crlList.Length, out p, out n));
        }
        finally
        {
            foreach (var h in handles)
                h.Free();
        }
    }

    /// <summary>Validate every signature in <paramref name="pdf"/>. Returns one
    /// <see cref="SignatureInfo"/> per signature; an empty list means the document
    /// is unsigned. Parses the JSON produced by <c>pdf_verify_signatures_json</c>.</summary>
    public static IReadOnlyList<SignatureInfo> VerifySignatures(byte[] pdf)
    {
        var bytes = TakeBuffer((out IntPtr p, out nuint n) =>
            Native.pdf_verify_signatures_json(pdf, (nuint)pdf.Length, out p, out n));
        var json = Encoding.UTF8.GetString(bytes);
        var result = new List<SignatureInfo>();
        if (string.IsNullOrEmpty(json))
            return result;
        using var doc = JsonDocument.Parse(json);
        foreach (var el in doc.RootElement.EnumerateArray())
        {
            string? GetStr(string k) =>
                el.TryGetProperty(k, out var v) && v.ValueKind != JsonValueKind.Null ? v.GetString() : null;
            bool GetBool(string k) => el.TryGetProperty(k, out var v) && v.ValueKind == JsonValueKind.True;
            var range = Array.Empty<int>();
            if (el.TryGetProperty("byte_range", out var br) && br.ValueKind == JsonValueKind.Array)
                range = br.EnumerateArray().Select(x => x.GetInt32()).ToArray();
            result.Add(new SignatureInfo(
                GetStr("field_name"),
                GetStr("sub_filter") ?? "",
                GetStr("signer"),
                GetBool("covers_whole_document"),
                GetBool("digest_valid"),
                GetBool("signature_valid"),
                GetBool("is_valid"),
                range));
        }
        return result;
    }

    private static (IntPtr[], nuint[]) Pin(byte[][] items, List<GCHandle> handles)
    {
        var ptrs = new IntPtr[items.Length];
        var lens = new nuint[items.Length];
        for (int i = 0; i < items.Length; i++)
        {
            var h = GCHandle.Alloc(items[i], GCHandleType.Pinned);
            handles.Add(h);
            ptrs[i] = items[i].Length == 0 ? IntPtr.Zero : h.AddrOfPinnedObject();
            lens[i] = (nuint)items[i].Length;
        }
        return (ptrs, lens);
    }

    // ---- shared helpers (used by Document/EditableDoc too) -------------------

    internal static void Check(int status)
    {
        if (status != 0)
            throw new PdfException(status, LastError());
    }

    internal static string LastError()
    {
        var p = Native.pdf_last_error_message();
        return p == IntPtr.Zero ? "unknown error" : Marshal.PtrToStringUTF8(p) ?? "unknown error";
    }

    internal static byte[] TakeBuffer(OutBuf call)
    {
        Native.Init();
        Check(call(out var ptr, out var len));
        try
        {
            if (ptr == IntPtr.Zero || len == 0)
                return Array.Empty<byte>();
            var buf = new byte[(int)len];
            Marshal.Copy(ptr, buf, 0, (int)len);
            return buf;
        }
        finally
        {
            Native.pdf_buffer_free(ptr, len);
        }
    }
}
