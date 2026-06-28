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
                    br));
        }
        return out;
    }

    private static String str(Object o) {
        return o == null ? "" : o.toString();
    }

    private static boolean bool(Object o) {
        return o instanceof Boolean b && b;
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
