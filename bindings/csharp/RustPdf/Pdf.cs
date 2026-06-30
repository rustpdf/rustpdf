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

    // PDF/A-4 (ISO 19005-4), based on PDF 2.0.
    A4 = 5,
    A4e = 6,
    A4f = 7,
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

/// <summary>The validation result for a single signature in a PDF. The
/// certificate-detail fields (<see cref="Issuer"/> … <see cref="HasTimestamp"/>)
/// are populated when the signer certificate could be parsed; otherwise null/0.</summary>
public sealed record SignatureInfo(
    string? FieldName,
    string SubFilter,
    string? Signer,
    bool CoversWholeDocument,
    bool DigestValid,
    bool SignatureValid,
    bool IsValid,
    int[] ByteRange,
    string? Issuer = null,
    string? SerialNumber = null,
    string? ValidFrom = null,
    string? ValidTo = null,
    string? Algorithm = null,
    string? SigningTime = null,
    int CertCount = 0,
    bool HasTimestamp = false);

/// <summary>One positional text match from <see cref="Pdf.FindText"/>: the
/// bounding box (PDF points, origin lower-left) of <see cref="Text"/> on
/// <see cref="Page"/> (0-based).</summary>
public sealed record TextHit(
    int Page, string Text, double X, double Y, double Width, double Height);

/// <summary>A rectangle in PDF user space (points, origin lower-left).</summary>
public sealed record PdfRect(double X0, double Y0, double X1, double Y1)
{
    /// <summary>Width of the rectangle (non-negative).</summary>
    public double Width => Math.Abs(X1 - X0);
    /// <summary>Height of the rectangle (non-negative).</summary>
    public double Height => Math.Abs(Y1 - Y0);
}

/// <summary>Read-only geometry of one page (from <see cref="Pdf.MeasurePage"/> /
/// <see cref="Pdf.MeasurePages"/>). Sizes are in PDF points;
/// <see cref="Width"/>/<see cref="Height"/> ignore rotation while
/// <see cref="RotatedWidth"/>/<see cref="RotatedHeight"/> account for it.</summary>
public sealed record PageGeometry(
    int Page, double Width, double Height, int Rotation,
    double RotatedWidth, double RotatedHeight, PdfRect MediaBox, PdfRect CropBox);

/// <summary>A non-mutating summary of a PDF (from <see cref="Pdf.Inspect"/>).</summary>
public sealed record PdfOverview(
    string Version, string? PdfaLevel, bool Encrypted, string Encryption,
    bool RequiresPassword, int PageCount);

/// <summary>A signature field discovered in a PDF (pre-signing inventory).</summary>
public sealed record SignatureField(string Name, bool Signed);

/// <summary>DocMDP certification level applied by the first (certifying) signature.</summary>
public enum Certify
{
    /// <summary>Not a certifying signature.</summary>
    None = 0,
    /// <summary><c>/P 1</c> — no changes permitted after signing.</summary>
    Locked = 1,
    /// <summary><c>/P 2</c> — form-filling and signing permitted.</summary>
    Forms = 2,
    /// <summary><c>/P 3</c> — form-filling, signing and annotations permitted.</summary>
    FormsAndAnnotations = 3,
}

/// <summary>A signature-policy identifier (PAdES-EPES / ICP-Brasil AD-RB).</summary>
public sealed class SignaturePolicy
{
    /// <summary>The policy OID (dotted-decimal), e.g. the ICP-Brasil AD-RB OID.</summary>
    public string Oid { get; set; } = "";
    /// <summary>The policy document hash (under <see cref="HashAlgorithmOid"/>).</summary>
    public byte[] Hash { get; set; } = Array.Empty<byte>();
    /// <summary>Hash algorithm OID; null = SHA-256.</summary>
    public string? HashAlgorithmOid { get; set; }
    /// <summary>Optional SPURI qualifier — where the policy can be retrieved.</summary>
    public string? Uri { get; set; }
}

/// <summary>Options for deferred / external signing (issue #41 P0).</summary>
public sealed class SigningOptions
{
    public string? Reason { get; set; }
    public string? Location { get; set; }
    public string? Name { get; set; }
    /// <summary>Produce a PAdES-B-B signature (<c>ETSI.CAdES.detached</c>).</summary>
    public bool Pades { get; set; }
    /// <summary>Certify the document (DocMDP) — use only on the first signature.</summary>
    public Certify Certify { get; set; } = Certify.None;
    /// <summary>Reserved <c>/Contents</c> bytes; 0 = default (8192). Raise for
    /// large cloud-HSM CMS containers.</summary>
    public int ContainerSize { get; set; }
    /// <summary>Signature-policy identifier (PAdES-EPES); null = none.</summary>
    public SignaturePolicy? Policy { get; set; }

    // ---- visible signature + embedded image (issue #41 P1) ------------------

    /// <summary>Draw a visible signature appearance using the fields below.</summary>
    public bool Visible { get; set; }
    /// <summary>0-based page index for the visible appearance.</summary>
    public int VisiblePage { get; set; }
    /// <summary>Appearance rectangle <c>[x0, y0, x1, y1]</c> in page points.</summary>
    public double[] VisibleRect { get; set; } = new double[4];
    /// <summary>Appearance text lines (separated by <c>\n</c>); null = none.</summary>
    public string? VisibleText { get; set; }
    /// <summary>PNG/JPEG bytes of a handwritten-signature image; null = none.</summary>
    public byte[]? VisibleImage { get; set; }
}

/// <summary>The "bring your own signer" callback: produces the raw RSA PKCS#1
/// v1.5 signature over SHA-256 of <paramref name="dataToSign"/>, typically by
/// calling a remote HSM (Azure Key Vault, VIDaaS, BirdID). The private key never
/// reaches this library. Pass it to <see cref="Pdf.SignWith(byte[],byte[],RemoteSign,IEnumerable{byte[]},SigningOptions)"/>.</summary>
public delegate byte[] RemoteSign(byte[] dataToSign);

/// <summary>Object-shaped alternative to the <see cref="RemoteSign"/> delegate
/// for callers who prefer an interface. For an asynchronous HSM, either block
/// inside <see cref="SignHash"/> or use the two-phase
/// <see cref="Pdf.BeginSigning"/> / <see cref="SigningSession.Complete"/> flow.</summary>
public interface IRemoteSigner
{
    /// <summary>Produce the raw RSA PKCS#1 v1.5 signature over SHA-256 of
    /// <paramref name="dataToSign"/>.</summary>
    byte[] SignHash(byte[] dataToSign);
}

/// <summary>An in-progress two-phase signature: <see cref="Document"/> holds the
/// placeholder PDF and <see cref="Bytes"/> the exact bytes the signature covers.
/// Hand <see cref="Hash"/> to a remote signer, build the CMS container, then call
/// <see cref="Complete"/>.</summary>
public sealed class SigningSession
{
    /// <summary>The prepared PDF (with a zero-filled <c>/Contents</c> placeholder).</summary>
    public byte[] Document { get; }
    /// <summary>The exact bytes covered by the signature (the two ByteRange segments).</summary>
    public byte[] Bytes { get; }

    internal SigningSession(byte[] document, byte[] bytes)
    {
        Document = document;
        Bytes = bytes;
    }

    /// <summary>SHA-256 of <see cref="Bytes"/> — the value an HSM signs.</summary>
    public byte[] Hash => System.Security.Cryptography.SHA256.HashData(Bytes);

    /// <summary>Phase 2: complete the signature by embedding a finished DER CMS /
    /// PKCS#7 container, returning the final signed PDF.</summary>
    public byte[] Complete(byte[] container) => Pdf.CompleteSignature(Document, container);
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

    /// <summary>Find every occurrence of <paramref name="query"/> in
    /// <paramref name="pdf"/>, returning each match's positional bounding box
    /// (PDF points, origin lower-left). An empty list means no match.</summary>
    public static IReadOnlyList<TextHit> FindText(byte[] pdf, string query, bool caseSensitive = false)
    {
        var bytes = TakeBuffer((out IntPtr p, out nuint n) =>
            Native.pdf_find_text_json(pdf, (nuint)pdf.Length, query, caseSensitive ? 1 : 0, out p, out n));
        var json = Encoding.UTF8.GetString(bytes);
        var result = new List<TextHit>();
        if (string.IsNullOrEmpty(json))
            return result;
        using var doc = JsonDocument.Parse(json);
        foreach (var el in doc.RootElement.EnumerateArray())
        {
            double GetNum(string k) =>
                el.TryGetProperty(k, out var v) && v.ValueKind == JsonValueKind.Number ? v.GetDouble() : 0.0;
            int GetInt(string k) =>
                el.TryGetProperty(k, out var v) && v.ValueKind == JsonValueKind.Number ? v.GetInt32() : 0;
            string GetStr(string k) =>
                el.TryGetProperty(k, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() ?? "" : "";
            result.Add(new TextHit(
                GetInt("page"), GetStr("text"),
                GetNum("x"), GetNum("y"), GetNum("width"), GetNum("height")));
        }
        return result;
    }

    /// <summary>Read the geometry (size, rotation, MediaBox, CropBox) of every
    /// page in <paramref name="pdf"/>, in page order, without mutating it.</summary>
    public static IReadOnlyList<PageGeometry> MeasurePages(byte[] pdf)
    {
        var bytes = TakeBuffer((out IntPtr p, out nuint n) =>
            Native.pdf_measure_pages_json(pdf, (nuint)pdf.Length, out p, out n));
        var json = Encoding.UTF8.GetString(bytes);
        var result = new List<PageGeometry>();
        if (string.IsNullOrEmpty(json))
            return result;
        using var doc = JsonDocument.Parse(json);
        foreach (var el in doc.RootElement.EnumerateArray())
        {
            double Num(string k) =>
                el.TryGetProperty(k, out var v) && v.ValueKind == JsonValueKind.Number ? v.GetDouble() : 0.0;
            int Int(string k) =>
                el.TryGetProperty(k, out var v) && v.ValueKind == JsonValueKind.Number ? v.GetInt32() : 0;
            PdfRect Rect(string k)
            {
                if (el.TryGetProperty(k, out var v) && v.ValueKind == JsonValueKind.Array && v.GetArrayLength() == 4)
                    return new PdfRect(v[0].GetDouble(), v[1].GetDouble(), v[2].GetDouble(), v[3].GetDouble());
                return new PdfRect(0, 0, 0, 0);
            }
            result.Add(new PageGeometry(
                Int("page"), Num("width"), Num("height"), Int("rotation"),
                Num("rotatedWidth"), Num("rotatedHeight"), Rect("mediaBox"), Rect("cropBox")));
        }
        return result;
    }

    /// <summary>Read the geometry of a single page (0-based) of
    /// <paramref name="pdf"/>.</summary>
    /// <exception cref="ArgumentOutOfRangeException">if <paramref name="pageIndex"/> is invalid.</exception>
    public static PageGeometry MeasurePage(byte[] pdf, int pageIndex)
    {
        var pages = MeasurePages(pdf);
        if (pageIndex < 0 || pageIndex >= pages.Count)
            throw new ArgumentOutOfRangeException(nameof(pageIndex));
        return pages[pageIndex];
    }

    /// <summary>Inspect <paramref name="pdf"/> without mutating it: PDF version,
    /// PDF/A level (if any), encryption posture and page count. Works even on
    /// password-protected files (the encryption fields are still reported).</summary>
    public static PdfOverview Inspect(byte[] pdf)
    {
        var bytes = TakeBuffer((out IntPtr p, out nuint n) =>
            Native.pdf_inspect_json(pdf, (nuint)pdf.Length, out p, out n));
        var json = Encoding.UTF8.GetString(bytes);
        using var doc = JsonDocument.Parse(json);
        var root = doc.RootElement;
        string Str(string k) =>
            root.TryGetProperty(k, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() ?? "" : "";
        bool Bool(string k) =>
            root.TryGetProperty(k, out var v) && (v.ValueKind == JsonValueKind.True || v.ValueKind == JsonValueKind.False) && v.GetBoolean();
        int Int(string k) =>
            root.TryGetProperty(k, out var v) && v.ValueKind == JsonValueKind.Number ? v.GetInt32() : 0;
        string? pdfa = root.TryGetProperty("pdfaLevel", out var pv) && pv.ValueKind == JsonValueKind.String
            ? pv.GetString() : null;
        return new PdfOverview(
            Str("version"), pdfa, Bool("encrypted"), Str("encryption"),
            Bool("requiresPassword"), Int("pageCount"));
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

    /// <summary>List the signature fields in <paramref name="pdf"/> (detect
    /// existing signatures before signing — the iText
    /// <c>SignatureUtil.getSignatureNames</c> equivalent). An empty list means
    /// there are no signature fields.</summary>
    public static IReadOnlyList<SignatureField> ListSignatures(byte[] pdf)
    {
        var bytes = TakeBuffer((out IntPtr p, out nuint n) =>
            Native.pdf_list_signatures(pdf, (nuint)pdf.Length, out p, out n));
        var text = Encoding.UTF8.GetString(bytes);
        var list = new List<SignatureField>();
        foreach (var line in text.Split('\n', StringSplitOptions.RemoveEmptyEntries))
        {
            var tab = line.IndexOf('\t');
            if (tab < 0)
                continue;
            list.Add(new SignatureField(line[(tab + 1)..], line[..tab] == "1"));
        }
        return list;
    }

    /// <summary>
    /// <b>Model B — two-phase signing, phase 1.</b> Prepare <paramref name="pdf"/>
    /// for deferred signing: returns a <see cref="SigningSession"/> whose
    /// <see cref="SigningSession.Digest"/> you send to a remote HSM. Build the
    /// CMS container, then call <see cref="SigningSession.Embed"/>
    /// (or <see cref="CompleteSignature"/>). The key never reaches this library.
    /// </summary>
    public static SigningSession BeginSigning(byte[] pdf, SigningOptions? options = null)
    {
        Native.Init();
        var handles = new List<GCHandle>();
        var allocs = new List<IntPtr>();
        try
        {
            var pms = BuildSigningOptions(options, allocs, handles);
            Check(Native.pdf_sign_begin(
                pdf, (nuint)pdf.Length, in pms,
                out var docPtr, out var docLen, out var tbsPtr, out var tbsLen));
            var document = CopyAndFree(docPtr, docLen);
            var tbs = CopyAndFree(tbsPtr, tbsLen);
            return new SigningSession(document, tbs);
        }
        finally
        {
            FreeAll(handles, allocs);
        }
    }

    /// <summary><b>Model B — two-phase signing, phase 2.</b> Embed a complete DER
    /// CMS / PKCS#7 <paramref name="container"/> into a prepared
    /// <paramref name="document"/> (from <see cref="BeginSigning"/>),
    /// producing the final signed PDF.</summary>
    public static byte[] CompleteSignature(byte[] document, byte[] container)
    {
        return TakeBuffer((out IntPtr p, out nuint n) => Native.pdf_sign_complete(
            document, (nuint)document.Length, container, (nuint)container.Length, out p, out n));
    }

    /// <summary><b>Network timestamp (AD-RT), phase 1.</b> Prepare
    /// <paramref name="pdf"/> for a <c>/DocTimeStamp</c> from a network RFC 3161
    /// TSA. Returns the prepared <c>Document</c> (with a placeholder) and the
    /// <c>Bytes</c> to timestamp. SHA-256 <c>Bytes</c>, build a request with
    /// <see cref="TimestampRequest"/>, POST it to the TSA, extract the token with
    /// <see cref="TimestampTokenFromResponse"/>, then embed it via
    /// <see cref="CompleteSignature"/>.</summary>
    public static (byte[] Document, byte[] Bytes) BeginTimestamp(byte[] pdf)
    {
        Native.Init();
        Check(Native.pdf_timestamp_begin(
            pdf, (nuint)pdf.Length,
            out var docPtr, out var docLen, out var tbsPtr, out var tbsLen));
        var document = CopyAndFree(docPtr, docLen);
        var tbs = CopyAndFree(tbsPtr, tbsLen);
        return (document, tbs);
    }

    /// <summary>Build an RFC 3161 <c>TimeStampReq</c> (DER) for
    /// <paramref name="imprint"/> (the SHA-256 of the bytes to timestamp).
    /// <paramref name="nonce"/> is optional; <paramref name="certReq"/> asks the
    /// TSA to embed its certificate. POST the result to the TSA.</summary>
    public static byte[] TimestampRequest(byte[] imprint, byte[]? nonce = null, bool certReq = true)
    {
        var nonceLen = (nuint)(nonce?.Length ?? 0);
        return TakeBuffer((out IntPtr p, out nuint n) => Native.pdf_timestamp_request(
            imprint, (nuint)imprint.Length, nonce, nonceLen, certReq ? 1 : 0, out p, out n));
    }

    /// <summary>Extract the <c>TimeStampToken</c> (a CMS <c>ContentInfo</c>) from a
    /// TSA's RFC 3161 <c>TimeStampResp</c>. Embed the token via
    /// <see cref="CompleteSignature"/>.</summary>
    public static byte[] TimestampTokenFromResponse(byte[] response)
    {
        return TakeBuffer((out IntPtr p, out nuint n) => Native.pdf_timestamp_token_from_response(
            response, (nuint)response.Length, out p, out n));
    }

    /// <summary>
    /// <b>Model A — remote signer.</b> Sign <paramref name="pdf"/> without handing
    /// this library a key: it builds the CMS signed attributes and calls
    /// <paramref name="signHash"/> for the raw RSA signature, then assembles and
    /// embeds the CMS. <paramref name="certDer"/> is the signer certificate;
    /// <paramref name="chain"/> are intermediates (DER), supplied independently of
    /// the key.
    /// </summary>
    public static unsafe byte[] SignWith(
        byte[] pdf, byte[] certDer, RemoteSign signHash,
        IEnumerable<byte[]>? chain = null, SigningOptions? options = null)
    {
        Native.Init();
        var chainList = chain?.ToArray() ?? Array.Empty<byte[]>();
        var handles = new List<GCHandle>();
        var allocs = new List<IntPtr>();
        var callbackHandle = GCHandle.Alloc(signHash);
        try
        {
            var (cp, cl) = Pin(chainList, handles);
            var pms = BuildSigningOptions(options, allocs, handles);
            delegate* unmanaged[Cdecl]<IntPtr, byte*, nuint, byte*, nuint, nuint*, int> cb =
                &SignTrampoline;
            Check(Native.pdf_sign_with(
                pdf, (nuint)pdf.Length, certDer, (nuint)certDer.Length,
                cp, cl, (nuint)chainList.Length, in pms, cb,
                GCHandle.ToIntPtr(callbackHandle), out var ptr, out var len));
            return CopyAndFree(ptr, len);
        }
        finally
        {
            callbackHandle.Free();
            FreeAll(handles, allocs);
        }
    }

    /// <summary>Interface-shaped overload of
    /// <see cref="SignWith(byte[],byte[],RemoteSign,IEnumerable{byte[]},SigningOptions)"/>.</summary>
    public static byte[] SignWith(
        byte[] pdf, byte[] certDer, IRemoteSigner signer,
        IEnumerable<byte[]>? chain = null, SigningOptions? options = null)
        => SignWith(pdf, certDer, signer.SignHash, chain, options);

    /// <summary><b>Model A — async remote signer (issue #45 P2).</b> Convenience
    /// overload of <see cref="SignWith(byte[],byte[],RemoteSign,IEnumerable{byte[]},SigningOptions)"/>
    /// for an asynchronous HSM/HTTP signer. The whole signing runs on a thread-pool
    /// thread so the caller is not blocked; <paramref name="signHashAsync"/> is
    /// awaited for each raw RSA signature. For full back-pressure control prefer the
    /// two-phase <see cref="BeginSigning"/> / <see cref="SigningSession.Complete"/>
    /// flow.</summary>
    public static Task<byte[]> SignWithAsync(
        byte[] pdf, byte[] certDer, Func<byte[], Task<byte[]>> signHashAsync,
        IEnumerable<byte[]>? chain = null, SigningOptions? options = null)
        => Task.Run(() =>
            SignWith(pdf, certDer, data => signHashAsync(data).GetAwaiter().GetResult(), chain, options));

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
    private static unsafe int SignTrampoline(
        IntPtr ctx, byte* data, nuint dataLen, byte* sigBuf, nuint sigCap, nuint* sigLen)
    {
        try
        {
            var signHash = (RemoteSign)GCHandle.FromIntPtr(ctx).Target!;
            var input = new byte[(int)dataLen];
            Marshal.Copy((IntPtr)data, input, 0, (int)dataLen);
            var sig = signHash(input);
            if ((nuint)sig.Length > sigCap)
                return 2; // buffer too small
            Marshal.Copy(sig, 0, (IntPtr)sigBuf, sig.Length);
            *sigLen = (nuint)sig.Length;
            return 0;
        }
        catch
        {
            return 1; // signer threw
        }
    }

    private static Native.SigningOptionsNative BuildSigningOptions(
        SigningOptions? p, List<IntPtr> allocs, List<GCHandle> handles)
    {
        var n = default(Native.SigningOptionsNative);
        if (p is null)
            return n;
        IntPtr Str(string? s)
        {
            if (s is null)
                return IntPtr.Zero;
            var ptr = Marshal.StringToCoTaskMemUTF8(s);
            allocs.Add(ptr);
            return ptr;
        }
        n.Reason = Str(p.Reason);
        n.Location = Str(p.Location);
        n.Name = Str(p.Name);
        n.Pades = p.Pades ? 1 : 0;
        n.Certification = (int)p.Certify;
        n.EstimatedSize = p.ContainerSize > 0 ? (nuint)p.ContainerSize : 0;
        if (p.Policy is { } pol)
        {
            n.PolicyOid = Str(pol.Oid);
            if (pol.Hash.Length > 0)
            {
                var h = GCHandle.Alloc(pol.Hash, GCHandleType.Pinned);
                handles.Add(h);
                n.PolicyHash = h.AddrOfPinnedObject();
                n.PolicyHashLen = (nuint)pol.Hash.Length;
            }
            n.PolicyHashAlgOid = Str(pol.HashAlgorithmOid);
            n.PolicyUri = Str(pol.Uri);
        }
        n.Visible = p.Visible ? 1 : 0;
        n.VisPage = (nuint)p.VisiblePage;
        var rect = p.VisibleRect;
        if (rect is { Length: 4 })
        {
            n.VisRect0 = rect[0];
            n.VisRect1 = rect[1];
            n.VisRect2 = rect[2];
            n.VisRect3 = rect[3];
        }
        n.VisText = Str(p.VisibleText);
        if (p.VisibleImage is { Length: > 0 } img)
        {
            var h = GCHandle.Alloc(img, GCHandleType.Pinned);
            handles.Add(h);
            n.VisImage = h.AddrOfPinnedObject();
            n.VisImageLen = (nuint)img.Length;
        }
        return n;
    }

    private static void FreeAll(List<GCHandle> handles, List<IntPtr> allocs)
    {
        foreach (var h in handles)
            h.Free();
        foreach (var a in allocs)
            Marshal.FreeCoTaskMem(a);
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
            int GetInt(string k) =>
                el.TryGetProperty(k, out var v) && v.ValueKind == JsonValueKind.Number ? v.GetInt32() : 0;
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
                range,
                GetStr("issuer"),
                GetStr("serial_number"),
                GetStr("valid_from"),
                GetStr("valid_to"),
                GetStr("algorithm"),
                GetStr("signing_time"),
                GetInt("cert_count"),
                GetBool("has_timestamp")));
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

    /// <summary>Copy a native out-buffer into managed memory and free it.</summary>
    internal static byte[] CopyAndFree(IntPtr ptr, nuint len)
    {
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
