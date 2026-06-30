using System.Reflection;
using System.Runtime.InteropServices;

namespace RustPdf;

/// <summary>Raw P/Invoke surface over the rust-pdf C ABI (<c>libpdf_ffi</c>).
/// Mirrors <c>include/pdf.h</c> 1:1; application code uses the wrappers
/// (<see cref="Document"/>, <see cref="EditableDoc"/>, <see cref="Pdf"/>).</summary>
internal static partial class Native
{
    private const string Lib = "pdf_ffi";

    static Native()
    {
        NativeLibrary.SetDllImportResolver(typeof(Native).Assembly, Resolve);
    }

    /// <summary>Ensure the static constructor (and thus the resolver) has run
    /// before the first P/Invoke.</summary>
    internal static void Init() { }

    private static IntPtr Resolve(string name, Assembly assembly, DllImportSearchPath? path)
    {
        if (name != Lib)
            return IntPtr.Zero;
        foreach (var candidate in Candidates())
        {
            if (File.Exists(candidate) && NativeLibrary.TryLoad(candidate, out var handle))
                return handle;
        }
        return IntPtr.Zero; // fall back to the platform's default search
    }

    private static IEnumerable<string> Candidates()
    {
        var file = LibFileName();
        var env = Environment.GetEnvironmentVariable("RUSTPDF_LIB");
        if (!string.IsNullOrEmpty(env))
            yield return env;

        // Walk up from the assembly location looking for target/{debug,release}.
        var dir = AppContext.BaseDirectory;
        for (int i = 0; i < 10 && dir is not null; i++)
        {
            yield return Path.Combine(dir, "target", "debug", file);
            yield return Path.Combine(dir, "target", "release", file);
            dir = Path.GetDirectoryName(dir.TrimEnd(Path.DirectorySeparatorChar));
        }
    }

    private static string LibFileName()
    {
        if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
            return "pdf_ffi.dll";
        if (RuntimeInformation.IsOSPlatform(OSPlatform.OSX))
            return "libpdf_ffi.dylib";
        return "libpdf_ffi.so";
    }

    // ---- core ----------------------------------------------------------------

    [LibraryImport(Lib)]
    internal static partial IntPtr pdf_version();

    [LibraryImport(Lib)]
    internal static partial IntPtr pdf_last_error_message();

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_activate_license(string token);

    [LibraryImport(Lib)]
    internal static partial void pdf_buffer_free(IntPtr ptr, nuint len);

    // ---- document lifecycle + graphics --------------------------------------

    [LibraryImport(Lib)]
    internal static partial IntPtr pdf_document_new();

    [LibraryImport(Lib)]
    internal static partial void pdf_document_free(IntPtr doc);

    [LibraryImport(Lib)]
    internal static partial int pdf_document_add_page(IntPtr doc);

    [LibraryImport(Lib)]
    internal static partial int pdf_document_add_page_sized(IntPtr doc, double w, double h);

    [LibraryImport(Lib)]
    internal static partial int pdf_document_page_count(IntPtr doc);

    [LibraryImport(Lib)]
    internal static partial int pdf_page_set_fill_rgb(IntPtr doc, double r, double g, double b);

    [LibraryImport(Lib)]
    internal static partial int pdf_page_set_stroke_rgb(IntPtr doc, double r, double g, double b);

    [LibraryImport(Lib)]
    internal static partial int pdf_page_set_line_width(IntPtr doc, double w);

    [LibraryImport(Lib)]
    internal static partial int pdf_page_rect(IntPtr doc, double x, double y, double w, double h);

    [LibraryImport(Lib)]
    internal static partial int pdf_page_fill(IntPtr doc);

    [LibraryImport(Lib)]
    internal static partial int pdf_page_stroke(IntPtr doc);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_document_save(IntPtr doc, string path);

    [LibraryImport(Lib)]
    internal static partial int pdf_document_write(IntPtr doc, out IntPtr outPtr, out nuint outLen);

    // ---- configuration ------------------------------------------------------

    [LibraryImport(Lib)]
    internal static partial int pdf_document_pdfa(IntPtr doc);

    [LibraryImport(Lib)]
    internal static partial int pdf_document_pdfa_level(IntPtr doc, int level);

    [LibraryImport(Lib)]
    internal static partial int pdf_document_tagged(IntPtr doc);

    [LibraryImport(Lib)]
    internal static partial int pdf_document_set_version(IntPtr doc, int v);

    [LibraryImport(Lib)]
    internal static partial int pdf_document_set_default_size(IntPtr doc, double w, double h);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_document_set_info(
        IntPtr doc, string? title, string? author, string? subject, string? keywords, string? creator);

    // ---- fonts + text -------------------------------------------------------

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_document_add_font_file(IntPtr doc, string path, out int outId);

    [LibraryImport(Lib)]
    internal static partial int pdf_document_add_font(IntPtr doc, byte[] data, nuint len, out int outId);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_page_show_text(
        IntPtr doc, int font, double size, double x, double y, string text, int headingLevel);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_page_paragraph(
        IntPtr doc, int font, double size, double x, double y, double width, int align, string text);

    // ---- images -------------------------------------------------------------

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_document_add_image_file(IntPtr doc, string path, out int outId);

    [LibraryImport(Lib)]
    internal static partial int pdf_document_add_image_png(IntPtr doc, byte[] data, nuint len, out int outId);

    [LibraryImport(Lib)]
    internal static partial int pdf_document_add_image_jpeg(IntPtr doc, byte[] data, nuint len, out int outId);

    [LibraryImport(Lib)]
    internal static partial int pdf_page_draw_image(IntPtr doc, int image, double x, double y, double w, double h);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_page_figure(
        IntPtr doc, int image, double x, double y, double w, double h, string alt);

    // ---- attachments + forms ------------------------------------------------

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_document_attach_file(
        IntPtr doc, string name, string mime, byte[] data, nuint len, int relationship, string desc);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_document_text_field(
        IntPtr doc, string name, nuint page, double x0, double y0, double x1, double y1,
        string value, double size);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_document_checkbox(
        IntPtr doc, string name, nuint page, double x0, double y0, double x1, double y1, int checkedFlag);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_document_dropdown(
        IntPtr doc, string name, nuint page, double x0, double y0, double x1, double y1,
        string options, int selected, double size);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_document_radio_group(
        IntPtr doc, string name, nuint page, nuint count, double[] rects,
        [MarshalAs(UnmanagedType.LPArray, ArraySubType = UnmanagedType.LPUTF8Str)] string[] exports,
        int selected);

    // ---- editable -----------------------------------------------------------

    [LibraryImport(Lib)]
    internal static partial IntPtr pdf_editable_load(byte[] data, nuint len);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial IntPtr pdf_editable_load_password(byte[] data, nuint len, string password);

    [LibraryImport(Lib)]
    internal static partial void pdf_editable_free(IntPtr ed);

    [LibraryImport(Lib)]
    internal static partial int pdf_editable_page_count(IntPtr ed);

    [LibraryImport(Lib)]
    internal static partial int pdf_editable_merge(IntPtr ed, IntPtr other);

    [LibraryImport(Lib)]
    internal static partial int pdf_editable_rotate_page(IntPtr ed, nuint index, int degrees);

    [LibraryImport(Lib)]
    internal static partial int pdf_editable_delete_page(IntPtr ed, nuint index);

    [LibraryImport(Lib)]
    internal static partial int pdf_editable_reorder_pages(IntPtr ed, nuint[] order, nuint count);

    [LibraryImport(Lib)]
    internal static partial int pdf_editable_extract_pages(IntPtr ed, nuint[] indices, nuint count, out IntPtr outEd);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_editable_set_info(IntPtr ed, string key, string value);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_editable_get_info(IntPtr ed, string key, out IntPtr outPtr, out nuint outLen);

    [LibraryImport(Lib)]
    internal static partial int pdf_editable_set_xmp(IntPtr ed, byte[] xml, nuint len);

    [LibraryImport(Lib)]
    internal static partial int pdf_editable_overlay_page(IntPtr ed, nuint index, byte[] content, nuint len);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_editable_fill_text_field(IntPtr ed, string name, string value, out int outFound);

    [LibraryImport(Lib)]
    internal static partial int pdf_editable_optimize(IntPtr ed);

    [LibraryImport(Lib)]
    internal static partial int pdf_editable_compact(IntPtr ed, int on);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_editable_encrypt(IntPtr ed, int method, string user, string owner, int readOnly);

    [LibraryImport(Lib)]
    internal static partial int pdf_editable_to_bytes(IntPtr ed, out IntPtr outPtr, out nuint outLen);

    [LibraryImport(Lib)]
    internal static partial int pdf_editable_to_bytes_incremental(
        IntPtr ed, byte[] original, nuint originalLen, out IntPtr outPtr, out nuint outLen);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_editable_save(IntPtr ed, string path);

    // ---- extract + sign -----------------------------------------------------

    [LibraryImport(Lib)]
    internal static partial int pdf_extract_text(byte[] data, nuint len, out IntPtr outPtr, out nuint outLen);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_extract_images_to_dir(byte[] data, nuint len, string dir, out nuint outCount);

    [LibraryImport(Lib)]
    internal static partial int pdf_render_page_to_png(
        byte[] data, nuint len, nuint pageIndex, double dpi, out IntPtr outPtr, out nuint outLen);

    [LibraryImport(Lib)]
    internal static partial int pdf_page_count(byte[] data, nuint len, out nuint outCount);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_sign(
        byte[] pdf, nuint pdfLen, byte[] keyDer, nuint keyLen, byte[] certDer, nuint certLen,
        string? reason, string? location, string? name, int pades, out IntPtr outPtr, out nuint outLen);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_timestamp(
        byte[] pdf, nuint pdfLen, byte[] keyDer, nuint keyLen, byte[] certDer, nuint certLen,
        string? date, out IntPtr outPtr, out nuint outLen);

    [LibraryImport(Lib)]
    internal static partial int pdf_add_dss(
        byte[] pdf, nuint pdfLen,
        IntPtr[] certPtrs, nuint[] certLens, nuint certCount,
        IntPtr[] crlPtrs, nuint[] crlLens, nuint crlCount,
        out IntPtr outPtr, out nuint outLen);

    // ---- Tier 1: hyperlinks + bookmarks (Document) --------------------------

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_page_link_uri(
        IntPtr doc, double x0, double y0, double x1, double y1, string uri);

    [LibraryImport(Lib)]
    internal static partial int pdf_page_link_to_page(
        IntPtr doc, double x0, double y0, double x1, double y1,
        nuint targetPage, double top, int hasTop);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_document_add_bookmarks(
        IntPtr doc, nuint count, int[] levels,
        [MarshalAs(UnmanagedType.LPArray, ArraySubType = UnmanagedType.LPUTF8Str)] string[] titles,
        nuint[] pages, double[] tops, int[] hasTops);

    // ---- Tier 2: ZUGFeRD / Factur-X (Document) ------------------------------

    [LibraryImport(Lib)]
    internal static partial int pdf_document_facturx(IntPtr doc, byte[] xml, nuint len, int profile);

    // ---- Tier 1: form fill + flatten + watermark (EditableDoc) --------------

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_editable_set_checkbox(IntPtr ed, string name, int checkedFlag, out int outFound);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_editable_set_radio(IntPtr ed, string name, string exportValue, out int outFound);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_editable_set_choice(IntPtr ed, string name, string value, out int outFound);

    [LibraryImport(Lib)]
    internal static partial int pdf_editable_flatten_forms(IntPtr ed);

    [LibraryImport(Lib)]
    internal static partial int pdf_editable_field_names(IntPtr ed, out IntPtr outPtr, out nuint outLen);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_editable_watermark_text(
        IntPtr ed, string text, double size, double r, double g, double b, double opacity, double rotationDeg);

    [LibraryImport(Lib, StringMarshalling = StringMarshalling.Utf8)]
    internal static partial int pdf_editable_watermark_image_file(
        IntPtr ed, string path, double width, double height, double opacity);

    // ---- Tier 2: redaction + PDF/A conversion (EditableDoc) -----------------

    [LibraryImport(Lib)]
    internal static partial int pdf_editable_redact(
        IntPtr ed, nuint index, double[] rects, nuint count, out int outFound);

    [LibraryImport(Lib)]
    internal static partial int pdf_editable_convert_to_pdfa(IntPtr ed, int level);

    // ---- Tier 2: signature validation (module-level) ------------------------

    [LibraryImport(Lib)]
    internal static partial int pdf_verify_signatures_json(
        byte[] data, nuint len, out IntPtr outPtr, out nuint outLen);

    // ---- Deferred / external (HSM) signing — issue #41 P0 -------------------

    /// <summary>Mirrors the C-ABI <c>PdfSigningOptions</c> (deferred-signing options).
    /// All pointer fields are NULL when unused; a zero <c>EstimatedSize</c> or
    /// <c>PolicyHashLen</c> means "absent".</summary>
    [StructLayout(LayoutKind.Sequential)]
    internal struct SigningOptionsNative
    {
        public IntPtr Reason;
        public IntPtr Location;
        public IntPtr Name;
        public int Pades;
        public int Certification;
        public nuint EstimatedSize;
        public IntPtr PolicyOid;
        public IntPtr PolicyHash;
        public nuint PolicyHashLen;
        public IntPtr PolicyHashAlgOid;
        public IntPtr PolicyUri;
    }

    [LibraryImport(Lib)]
    internal static partial int pdf_sign_begin(
        byte[] pdf, nuint pdfLen, in SigningOptionsNative pms,
        out IntPtr outDoc, out nuint outDocLen, out IntPtr outTbs, out nuint outTbsLen);

    [LibraryImport(Lib)]
    internal static partial int pdf_sign_complete(
        byte[] document, nuint documentLen, byte[] container, nuint containerLen,
        out IntPtr outPtr, out nuint outLen);

    [LibraryImport(Lib)]
    internal static unsafe partial int pdf_sign_with(
        byte[] pdf, nuint pdfLen, byte[] certDer, nuint certLen,
        IntPtr[] chainPtrs, nuint[] chainLens, nuint chainCount,
        in SigningOptionsNative pms,
        delegate* unmanaged[Cdecl]<IntPtr, byte*, nuint, byte*, nuint, nuint*, int> callback,
        IntPtr ctx, out IntPtr outPtr, out nuint outLen);

    [LibraryImport(Lib)]
    internal static partial int pdf_list_signatures(
        byte[] pdf, nuint pdfLen, out IntPtr outPtr, out nuint outLen);
}
