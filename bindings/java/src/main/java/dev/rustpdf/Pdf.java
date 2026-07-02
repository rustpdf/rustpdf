package dev.rustpdf;

import com.sun.jna.Memory;
import com.sun.jna.Pointer;
import com.sun.jna.ptr.LongByReference;
import com.sun.jna.ptr.PointerByReference;

import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.function.Function;

/** Top-level helpers: version, licensing, text extraction and signing. */
public final class Pdf {
    private Pdf() {}

    /** An out-buffer producer: writes {@code (ptr, len)} and returns a status. */
    @FunctionalInterface
    interface OutBuf {
        int call(PointerByReference ptr, LongByReference len);
    }

    /** Native library version string. */
    public static String version() {
        String v = FFI.C.pdf_version();
        return v == null ? "" : v;
    }

    /**
     * Activate a license token (unlocks PDF/A, signing, encryption, accessibility).
     * Tokens may also be supplied via the {@code RUSTPDF_LICENSE} or
     * {@code RUSTPDF_LICENSE_FILE} environment variables (auto-activated).
     *
     * @throws PdfException if the token is forged, expired or malformed.
     */
    public static void activateLicense(String token) {
        check(FFI.C.pdf_activate_license(token));
    }

    /** Extract a document's text (Unicode via {@code ToUnicode}). */
    public static String extractText(byte[] pdf) {
        byte[] bytes = takeBuffer((p, n) -> FFI.C.pdf_extract_text(pdf, pdf.length, p, n));
        return new String(bytes, StandardCharsets.UTF_8);
    }

    /**
     * Extract the text of a single page (0-based {@code pageIndex}), without
     * building an intermediate one-page document.
     *
     * @throws PdfException if {@code pageIndex} is out of range.
     */
    public static String extractPageText(byte[] pdf, int pageIndex) {
        byte[] bytes = takeBuffer((p, n) ->
                FFI.C.pdf_extract_page_text(pdf, pdf.length, pageIndex, p, n));
        return new String(bytes, StandardCharsets.UTF_8);
    }

    /**
     * Extract every raster image from {@code pdf} into directory {@code dir}
     * (JPEG verbatim as {@code .jpg}, everything else as {@code .png}; files named
     * {@code page{N}_{name}.{ext}}). Returns the number of images written.
     */
    public static long extractImagesToDir(byte[] pdf, String dir) {
        LongByReference count = new LongByReference();
        check(FFI.C.pdf_extract_images_to_dir(pdf, pdf.length, dir, count));
        return count.getValue();
    }

    /**
     * Render page {@code pageIndex} (0-based) of {@code pdf} to a PNG image at
     * {@code dpi} dots-per-inch. Page rendering is a licensed Pro feature: throws
     * {@link PdfException} (status {@code License}) unless a license granting it
     * is active.
     */
    public static byte[] renderPageToPng(byte[] pdf, int pageIndex, double dpi) {
        return takeBuffer((p, n) -> FFI.C.pdf_render_page_to_png(pdf, pdf.length, pageIndex, dpi, p, n));
    }

    /** Number of pages in {@code pdf} (free — no license required). */
    public static long pageCount(byte[] pdf) {
        LongByReference count = new LongByReference();
        check(FFI.C.pdf_page_count(pdf, pdf.length, count));
        return count.getValue();
    }

    /**
     * Sign {@code pdf} (PKCS#7 detached, incremental update). {@code pades} selects
     * PAdES-B-B. {@code reason}/{@code location}/{@code name} may be null. Requires a license.
     */
    public static byte[] sign(byte[] pdf, byte[] keyDer, byte[] certDer,
                              String reason, String location, String name, boolean pades) {
        return takeBuffer((p, n) -> FFI.C.pdf_sign(
                pdf, pdf.length, keyDer, keyDer.length, certDer, certDer.length,
                reason, location, name, pades ? 1 : 0, p, n));
    }

    /** Append a document timestamp ({@code /DocTimeStamp}, PAdES-B-LTA). */
    public static byte[] timestamp(byte[] pdf, byte[] tsaKeyDer, byte[] tsaCertDer, String date) {
        return takeBuffer((p, n) -> FFI.C.pdf_timestamp(
                pdf, pdf.length, tsaKeyDer, tsaKeyDer.length, tsaCertDer, tsaCertDer.length, date, p, n));
    }

    /** Append a Document Security Store ({@code /DSS}, PAdES-B-LT). */
    public static byte[] addDss(byte[] pdf, List<byte[]> certs, List<byte[]> crls) {
        List<byte[]> certList = certs == null ? List.of() : certs;
        List<byte[]> crlList = crls == null ? List.of() : crls;
        // Memory blocks are kept referenced until the native call returns.
        List<Memory> keep = new ArrayList<>();
        Pointer[] certPtrs = pin(certList, keep);
        long[] certLens = lens(certList);
        Pointer[] crlPtrs = pin(crlList, keep);
        long[] crlLens = lens(crlList);
        try {
            return takeBuffer((p, n) -> FFI.C.pdf_add_dss(
                    pdf, pdf.length, certPtrs, certLens, certList.size(),
                    crlPtrs, crlLens, crlList.size(), p, n));
        } finally {
            keep.forEach(Memory::close);
        }
    }

    private static Pointer[] pin(List<byte[]> items, List<Memory> keep) {
        Pointer[] ptrs = new Pointer[items.size()];
        for (int i = 0; i < items.size(); i++) {
            byte[] b = items.get(i);
            if (b.length == 0) {
                ptrs[i] = Pointer.NULL;
            } else {
                Memory mem = new Memory(b.length);
                mem.write(0, b, 0, b.length);
                keep.add(mem);
                ptrs[i] = mem;
            }
        }
        return ptrs;
    }

    private static long[] lens(List<byte[]> items) {
        long[] out = new long[items.size()];
        for (int i = 0; i < items.size(); i++) {
            out[i] = items.get(i).length;
        }
        return out;
    }

    /**
     * Validate every signature in {@code pdf}. Returns one {@link SignatureReport}
     * per signature; an empty list means the document is unsigned.
     */
    public static List<SignatureReport> verifySignatures(byte[] pdf) {
        byte[] bytes = takeBuffer((p, n) -> FFI.C.pdf_verify_signatures_json(pdf, pdf.length, p, n));
        String json = new String(bytes, StandardCharsets.UTF_8).trim();
        List<SignatureReport> out = new ArrayList<>();
        if (json.isEmpty()) {
            return out;
        }
        Object parsed = new Json(json).parse();
        if (!(parsed instanceof List<?> arr)) {
            return out;
        }
        for (Object item : arr) {
            if (!(item instanceof Map<?, ?> obj)) {
                continue;
            }
            long[] br = new long[0];
            Object brVal = obj.get("byte_range");
            if (brVal instanceof List<?> brList) {
                br = new long[brList.size()];
                for (int i = 0; i < brList.size(); i++) {
                    br[i] = ((Number) brList.get(i)).longValue();
                }
            }
            out.add(new SignatureReport(
                    (String) obj.get("field_name"),
                    str(obj.get("sub_filter")),
                    (String) obj.get("signer"),
                    bool(obj.get("covers_whole_document")),
                    bool(obj.get("digest_valid")),
                    bool(obj.get("signature_valid")),
                    bool(obj.get("is_valid")),
                    br,
                    nullable(obj.get("issuer")),
                    nullable(obj.get("serial_number")),
                    nullable(obj.get("valid_from")),
                    nullable(obj.get("valid_to")),
                    nullable(obj.get("algorithm")),
                    nullable(obj.get("signing_time")),
                    lng(obj.get("cert_count")),
                    bool(obj.get("has_timestamp"))));
        }
        return out;
    }

    /**
     * Find every occurrence of {@code query} in {@code pdf} (case-insensitive),
     * returning a positional {@link TextHit} (page + bounding box in points) per
     * match. An empty list means no match.
     */
    public static List<TextHit> findText(byte[] pdf, String query) {
        return findText(pdf, query, false);
    }

    /** Like {@link #findText(byte[], String)} but with case-sensitivity control. */
    public static List<TextHit> findText(byte[] pdf, String query, boolean caseSensitive) {
        byte[] bytes = takeBuffer((p, n) ->
                FFI.C.pdf_find_text_json(pdf, pdf.length, query, caseSensitive ? 1 : 0, p, n));
        String json = new String(bytes, StandardCharsets.UTF_8).trim();
        List<TextHit> out = new ArrayList<>();
        if (json.isEmpty()) {
            return out;
        }
        Object parsed = new Json(json).parse();
        if (!(parsed instanceof List<?> arr)) {
            return out;
        }
        for (Object item : arr) {
            if (!(item instanceof Map<?, ?> obj)) {
                continue;
            }
            out.add(new TextHit(
                    (int) lng(obj.get("page")),
                    str(obj.get("text")),
                    dbl(obj.get("x")),
                    dbl(obj.get("y")),
                    dbl(obj.get("width")),
                    dbl(obj.get("height"))));
        }
        return out;
    }

    /**
     * Read the geometry (size, rotation, MediaBox, CropBox) of every page in
     * {@code pdf}, in page order, without mutating it.
     */
    public static List<PageGeometry> measurePages(byte[] pdf) {
        byte[] bytes = takeBuffer((p, n) -> FFI.C.pdf_measure_pages_json(pdf, pdf.length, p, n));
        String json = new String(bytes, StandardCharsets.UTF_8).trim();
        List<PageGeometry> out = new ArrayList<>();
        if (json.isEmpty()) {
            return out;
        }
        Object parsed = new Json(json).parse();
        if (!(parsed instanceof List<?> arr)) {
            return out;
        }
        for (Object item : arr) {
            if (!(item instanceof Map<?, ?> obj)) {
                continue;
            }
            out.add(new PageGeometry(
                    (int) lng(obj.get("page")),
                    dbl(obj.get("width")),
                    dbl(obj.get("height")),
                    (int) lng(obj.get("rotation")),
                    dbl(obj.get("rotatedWidth")),
                    dbl(obj.get("rotatedHeight")),
                    rect(obj.get("mediaBox")),
                    rect(obj.get("cropBox"))));
        }
        return out;
    }

    /**
     * Read the geometry of a single page (0-based) of {@code pdf}.
     *
     * @throws IndexOutOfBoundsException if {@code pageIndex} is out of range.
     */
    public static PageGeometry measurePage(byte[] pdf, int pageIndex) {
        List<PageGeometry> pages = measurePages(pdf);
        if (pageIndex < 0 || pageIndex >= pages.size()) {
            throw new IndexOutOfBoundsException(
                    "page index " + pageIndex + " out of range [0, " + pages.size() + ")");
        }
        return pages.get(pageIndex);
    }

    /**
     * Inspect {@code pdf} without mutating it: PDF version, PDF/A level (if any),
     * encryption posture and page count. Works even on password-protected files
     * (the encryption fields are still reported).
     */
    public static PdfOverview inspect(byte[] pdf) {
        byte[] bytes = takeBuffer((p, n) -> FFI.C.pdf_inspect_json(pdf, pdf.length, p, n));
        String json = new String(bytes, StandardCharsets.UTF_8).trim();
        if (json.isEmpty() || !(new Json(json).parse() instanceof Map<?, ?> obj)) {
            return new PdfOverview("", null, false, "none", false, 0);
        }
        return new PdfOverview(
                str(obj.get("version")),
                nullable(obj.get("pdfaLevel")),
                bool(obj.get("encrypted")),
                str(obj.get("encryption")),
                bool(obj.get("requiresPassword")),
                (int) lng(obj.get("pageCount")));
    }

    /** Parse a 4-element JSON number array into a {@link PdfRect} (absent → all zeros). */
    private static PdfRect rect(Object o) {
        if (o instanceof List<?> arr && arr.size() == 4) {
            return new PdfRect(
                    dbl(arr.get(0)), dbl(arr.get(1)), dbl(arr.get(2)), dbl(arr.get(3)));
        }
        return new PdfRect(0, 0, 0, 0);
    }

    private static String str(Object o) {
        return o == null ? "" : o.toString();
    }

    private static String nullable(Object o) {
        return o == null ? null : o.toString();
    }

    private static boolean bool(Object o) {
        return o instanceof Boolean b && b;
    }

    private static long lng(Object o) {
        return o instanceof Number n ? n.longValue() : 0L;
    }

    private static double dbl(Object o) {
        return o instanceof Number n ? n.doubleValue() : 0.0;
    }

    /**
     * A tiny, dependency-free JSON reader (objects, arrays, strings, numbers,
     * booleans, null) — enough to parse the signature-report array. Kept minimal
     * on purpose so the binding stays a single JNA dependency.
     */
    private static final class Json {
        private final String s;
        private int i;

        Json(String s) {
            this.s = s;
        }

        Object parse() {
            Object v = value();
            ws();
            return v;
        }

        private Object value() {
            ws();
            char c = s.charAt(i);
            switch (c) {
                case '{': return object();
                case '[': return array();
                case '"': return string();
                case 't': i += 4; return Boolean.TRUE;   // true
                case 'f': i += 5; return Boolean.FALSE;  // false
                case 'n': i += 4; return null;           // null
                default:  return number();
            }
        }

        private Map<String, Object> object() {
            Map<String, Object> m = new LinkedHashMap<>();
            i++; // {
            ws();
            if (s.charAt(i) == '}') { i++; return m; }
            while (true) {
                ws();
                String key = string();
                ws();
                i++; // :
                m.put(key, value());
                ws();
                char c = s.charAt(i++);
                if (c == '}') break;
                // c == ','
            }
            return m;
        }

        private List<Object> array() {
            List<Object> list = new ArrayList<>();
            i++; // [
            ws();
            if (s.charAt(i) == ']') { i++; return list; }
            while (true) {
                list.add(value());
                ws();
                char c = s.charAt(i++);
                if (c == ']') break;
                // c == ','
            }
            return list;
        }

        private String string() {
            StringBuilder b = new StringBuilder();
            i++; // opening quote
            while (true) {
                char c = s.charAt(i++);
                if (c == '"') break;
                if (c == '\\') {
                    char e = s.charAt(i++);
                    switch (e) {
                        case '"': b.append('"'); break;
                        case '\\': b.append('\\'); break;
                        case '/': b.append('/'); break;
                        case 'b': b.append('\b'); break;
                        case 'f': b.append('\f'); break;
                        case 'n': b.append('\n'); break;
                        case 'r': b.append('\r'); break;
                        case 't': b.append('\t'); break;
                        case 'u':
                            b.append((char) Integer.parseInt(s.substring(i, i + 4), 16));
                            i += 4;
                            break;
                        default: b.append(e);
                    }
                } else {
                    b.append(c);
                }
            }
            return b.toString();
        }

        private Number number() {
            int start = i;
            while (i < s.length() && "+-0123456789.eE".indexOf(s.charAt(i)) >= 0) {
                i++;
            }
            String n = s.substring(start, i);
            if (n.indexOf('.') >= 0 || n.indexOf('e') >= 0 || n.indexOf('E') >= 0) {
                return Double.parseDouble(n);
            }
            return Long.parseLong(n);
        }

        private void ws() {
            while (i < s.length() && Character.isWhitespace(s.charAt(i))) {
                i++;
            }
        }
    }

    // ---- Deferred / external (HSM) signing — issue #41 P0 --------------------

    /**
     * List the signature fields in {@code pdf} (detect existing signatures before
     * signing — the classic pre-sign signature-field inventory). An
     * empty list means there are no signature fields.
     */
    public static List<SignatureField> listSignatures(byte[] pdf) {
        byte[] bytes = takeBuffer((p, n) -> FFI.C.pdf_list_signatures(pdf, pdf.length, p, n));
        String text = new String(bytes, StandardCharsets.UTF_8);
        List<SignatureField> out = new ArrayList<>();
        for (String line : text.split("\n")) {
            if (line.isEmpty()) {
                continue;
            }
            int tab = line.indexOf('\t');
            if (tab < 0) {
                continue;
            }
            out.add(new SignatureField(line.substring(tab + 1), line.substring(0, tab).equals("1")));
        }
        return out;
    }

    /**
     * <b>Model A — remote signer.</b> Sign {@code pdf} without handing this
     * library a key: it builds the CMS signed attributes and calls
     * {@code signHash} for the raw RSA PKCS#1 v1.5 signature (over SHA-256 of the
     * supplied data), then assembles and embeds the CMS. {@code certDer} is the
     * signer certificate; {@code chain} are intermediate certificates (DER),
     * supplied independently of the key. The private key never reaches this
     * library — typical {@code signHash} implementations call a cloud HSM
     * (Azure Key Vault, VIDaaS, BirdID).
     */
    public static byte[] signWith(byte[] pdf, byte[] certDer, Function<byte[], byte[]> signHash,
                                  List<byte[]> chain, SigningOptions options) {
        List<byte[]> chainList = chain == null ? List.of() : chain;
        List<Memory> keep = new ArrayList<>();
        Pointer[] chainPtrs = pin(chainList, keep);
        long[] chainLens = lens(chainList);
        FFI.PdfSigningOptions params = buildOptions(options, keep);
        // Strong reference held on the stack for the duration of the native call
        // (a synchronous call inside takeBuffer) so the callback is not GC'd.
        FFI.SignHashCallback cb = (ctx, data, dataLen, sigBuf, sigCap, sigLen) -> {
            try {
                byte[] input = data.getByteArray(0, (int) dataLen);
                byte[] sig = signHash.apply(input);
                if (sig == null) {
                    return 1;
                }
                if (sig.length > sigCap) {
                    return 2; // buffer too small
                }
                sigBuf.write(0, sig, 0, sig.length);
                sigLen.setLong(0, sig.length);
                return 0;
            } catch (Throwable t) {
                return 1; // signer threw
            }
        };
        try {
            return takeBuffer((p, n) -> FFI.C.pdf_sign_with(
                    pdf, pdf.length, certDer, certDer.length,
                    chainPtrs, chainLens, chainList.size(),
                    params, cb, Pointer.NULL, p, n));
        } finally {
            keep.forEach(Memory::close);
        }
    }

    /**
     * <b>Model B — two-phase signing, phase 1.</b> Prepare {@code pdf} for
     * deferred signing: returns a {@link SigningSession} whose {@link
     * SigningSession#hash()} you send to a remote HSM. Build the CMS container,
     * then call {@link SigningSession#complete(byte[])} (or {@link
     * #completeSignature(byte[], byte[])}). The key never reaches this library.
     */
    public static SigningSession beginSigning(byte[] pdf, SigningOptions options) {
        List<Memory> keep = new ArrayList<>();
        FFI.PdfSigningOptions params = buildOptions(options, keep);
        PointerByReference docPtr = new PointerByReference();
        LongByReference docLen = new LongByReference();
        PointerByReference tbsPtr = new PointerByReference();
        LongByReference tbsLen = new LongByReference();
        try {
            check(FFI.C.pdf_sign_begin(pdf, pdf.length, params, docPtr, docLen, tbsPtr, tbsLen));
            byte[] document = copyAndFree(docPtr.getValue(), docLen.getValue());
            byte[] tbs = copyAndFree(tbsPtr.getValue(), tbsLen.getValue());
            return new SigningSession(document, tbs);
        } finally {
            keep.forEach(Memory::close);
        }
    }

    /**
     * <b>Model B — two-phase signing, phase 2.</b> Embed a complete DER CMS /
     * PKCS#7 {@code container} into a prepared {@code document} (from {@link
     * #beginSigning(byte[], SigningOptions)}), producing the final signed PDF.
     */
    public static byte[] completeSignature(byte[] document, byte[] container) {
        return takeBuffer((p, n) -> FFI.C.pdf_sign_complete(
                document, document.length, container, container.length, p, n));
    }

    // ---- Network timestamp (AD-RT) — issue #41 P1 ---------------------------

    /**
     * <b>Network timestamp, phase 1.</b> Prepare {@code pdf} for a
     * {@code /DocTimeStamp} from a network RFC 3161 TSA. Returns a {@link
     * SigningSession} whose {@link SigningSession#hash()} (SHA-256 of {@link
     * SigningSession#bytes()}) feeds {@link #timestampRequest(byte[])}. POST that
     * request to the TSA, extract the token with {@link
     * #timestampTokenFromResponse(byte[])}, then embed it via {@link
     * SigningSession#complete(byte[])}.
     */
    public static SigningSession beginTimestamp(byte[] pdf) {
        PointerByReference docPtr = new PointerByReference();
        LongByReference docLen = new LongByReference();
        PointerByReference tbsPtr = new PointerByReference();
        LongByReference tbsLen = new LongByReference();
        check(FFI.C.pdf_timestamp_begin(pdf, pdf.length, docPtr, docLen, tbsPtr, tbsLen));
        byte[] document = copyAndFree(docPtr.getValue(), docLen.getValue());
        byte[] tbs = copyAndFree(tbsPtr.getValue(), tbsLen.getValue());
        return new SigningSession(document, tbs);
    }

    /** Build an RFC 3161 {@code TimeStampReq} (DER) for {@code imprint} (a SHA-256 digest). */
    public static byte[] timestampRequest(byte[] imprint) {
        return timestampRequest(imprint, null, true);
    }

    /**
     * Build an RFC 3161 {@code TimeStampReq} (DER) for {@code imprint} (a SHA-256
     * digest). {@code nonce} is optional ({@code null} = none); {@code certReq}
     * asks the TSA to embed its certificate.
     */
    public static byte[] timestampRequest(byte[] imprint, byte[] nonce, boolean certReq) {
        byte[] nz = nonce == null ? new byte[0] : nonce;
        return takeBuffer((p, n) -> FFI.C.pdf_timestamp_request(
                imprint, imprint.length, nz, nz.length, certReq ? 1 : 0, p, n));
    }

    /** Extract the {@code TimeStampToken} (CMS) from a TSA's RFC 3161 {@code TimeStampResp}. */
    public static byte[] timestampTokenFromResponse(byte[] response) {
        return takeBuffer((p, n) -> FFI.C.pdf_timestamp_token_from_response(
                response, response.length, p, n));
    }

    /** Build the native {@code PdfSigningOptions}, allocating UTF-8 strings into {@code keep}. */
    private static FFI.PdfSigningOptions buildOptions(SigningOptions opts, List<Memory> keep) {
        FFI.PdfSigningOptions n = new FFI.PdfSigningOptions();
        if (opts == null) {
            return n;
        }
        n.reason = utf8(opts.reason, keep);
        n.location = utf8(opts.location, keep);
        n.name = utf8(opts.name, keep);
        n.pades = opts.pades ? 1 : 0;
        n.certification = opts.certify == null ? 0 : opts.certify.code;
        n.estimatedSize = opts.containerSize > 0 ? opts.containerSize : 0;
        SignaturePolicy pol = opts.policy;
        if (pol != null) {
            n.policyOid = utf8(pol.oid, keep);
            if (pol.hash != null && pol.hash.length > 0) {
                Memory m = new Memory(pol.hash.length);
                m.write(0, pol.hash, 0, pol.hash.length);
                keep.add(m);
                n.policyHash = m;
                n.policyHashLen = pol.hash.length;
            }
            n.policyHashAlgOid = utf8(pol.hashAlgorithmOid, keep);
            n.policyUri = utf8(pol.uri, keep);
        }
        // Visible signature appearance (issue #41 P1).
        n.visible = opts.visible ? 1 : 0;
        n.visPage = opts.visiblePage;
        if (opts.visibleRect != null) {
            for (int i = 0; i < 4 && i < opts.visibleRect.length; i++) {
                n.visRect[i] = opts.visibleRect[i];
            }
        }
        n.visText = utf8(opts.visibleText, keep);
        if (opts.visibleImage != null && opts.visibleImage.length > 0) {
            Memory m = new Memory(opts.visibleImage.length);
            m.write(0, opts.visibleImage, 0, opts.visibleImage.length);
            keep.add(m);
            n.visImage = m;
            n.visImageLen = opts.visibleImage.length;
        }
        return n;
    }

    /** Allocate a NUL-terminated UTF-8 copy of {@code s} (null → NULL pointer). */
    private static Pointer utf8(String s, List<Memory> keep) {
        if (s == null) {
            return Pointer.NULL;
        }
        byte[] b = s.getBytes(StandardCharsets.UTF_8);
        Memory m = new Memory(b.length + 1L);
        m.write(0, b, 0, b.length);
        m.setByte(b.length, (byte) 0);
        keep.add(m);
        return m;
    }

    /** Copy a native out-buffer into a Java array and free it with {@code pdf_buffer_free}. */
    private static byte[] copyAndFree(Pointer p, long n) {
        try {
            if (p == null || n == 0) {
                return new byte[0];
            }
            return p.getByteArray(0, (int) n);
        } finally {
            if (p != null) {
                FFI.C.pdf_buffer_free(p, n);
            }
        }
    }

    // ---- shared helpers (used by Document/EditableDoc too) -------------------

    static void check(int status) {
        if (status != 0) {
            throw new PdfException(status, lastError());
        }
    }

    static String lastError() {
        String m = FFI.C.pdf_last_error_message();
        return (m == null || m.isEmpty()) ? "unknown error" : m;
    }

    static byte[] takeBuffer(OutBuf call) {
        PointerByReference ptr = new PointerByReference();
        LongByReference len = new LongByReference();
        check(call.call(ptr, len));
        Pointer p = ptr.getValue();
        long n = len.getValue();
        try {
            if (p == null || n == 0) {
                return new byte[0];
            }
            return p.getByteArray(0, (int) n);
        } finally {
            if (p != null) {
                FFI.C.pdf_buffer_free(p, n);
            }
        }
    }
}
