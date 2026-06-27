package dev.rustpdf;

import com.sun.jna.Memory;
import com.sun.jna.Pointer;
import com.sun.jna.ptr.LongByReference;
import com.sun.jna.ptr.PointerByReference;

import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;

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
