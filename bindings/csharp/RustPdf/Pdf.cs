using System.Runtime.InteropServices;
using System.Text;

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
