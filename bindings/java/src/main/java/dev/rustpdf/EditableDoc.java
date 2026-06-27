package dev.rustpdf;

import com.sun.jna.Pointer;
import com.sun.jna.ptr.IntByReference;
import com.sun.jna.ptr.PointerByReference;

import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;

/**
 * An existing PDF loaded for manipulation. Use with try-with-resources so the
 * native handle is freed.
 */
public final class EditableDoc implements AutoCloseable {
    private Pointer h;

    private EditableDoc(Pointer handle) {
        if (handle == null) {
            throw new PdfException(6, Pdf.lastError());
        }
        this.h = handle;
    }

    /** Load and parse a PDF from bytes. */
    public static EditableDoc load(byte[] data) {
        return new EditableDoc(FFI.C.pdf_editable_load(data, data.length));
    }

    /** Load an encrypted PDF using the given password. */
    public static EditableDoc load(byte[] data, String password) {
        return new EditableDoc(FFI.C.pdf_editable_load_password(data, data.length, password));
    }

    public static EditableDoc loadFile(Path path) {
        return load(readAll(path));
    }

    public static EditableDoc loadFile(Path path, String password) {
        return load(readAll(path), password);
    }

    private static byte[] readAll(Path path) {
        try {
            return Files.readAllBytes(path);
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }

    private Pointer h() {
        if (h == null) {
            throw new IllegalStateException("operation on a closed EditableDoc");
        }
        return h;
    }

    @Override
    public void close() {
        if (h != null) {
            FFI.C.pdf_editable_free(h);
            h = null;
        }
    }

    public int pageCount() {
        return FFI.C.pdf_editable_page_count(h());
    }

    public EditableDoc merge(EditableDoc other) {
        Pdf.check(FFI.C.pdf_editable_merge(h(), other.h()));
        return this;
    }

    public EditableDoc rotatePage(int index, int degrees) {
        Pdf.check(FFI.C.pdf_editable_rotate_page(h(), index, degrees));
        return this;
    }

    public EditableDoc deletePage(int index) {
        Pdf.check(FFI.C.pdf_editable_delete_page(h(), index));
        return this;
    }

    public EditableDoc reorderPages(int[] order) {
        long[] arr = new long[order.length];
        for (int i = 0; i < order.length; i++) {
            arr[i] = order[i];
        }
        Pdf.check(FFI.C.pdf_editable_reorder_pages(h(), arr, arr.length));
        return this;
    }

    /** Extract the given page indices into a new document. */
    public EditableDoc extractPages(int[] indices) {
        long[] arr = new long[indices.length];
        for (int i = 0; i < indices.length; i++) {
            arr[i] = indices[i];
        }
        PointerByReference out = new PointerByReference();
        Pdf.check(FFI.C.pdf_editable_extract_pages(h(), arr, arr.length, out));
        return new EditableDoc(out.getValue());
    }

    public EditableDoc setInfo(String key, String value) {
        Pdf.check(FFI.C.pdf_editable_set_info(h(), key, value));
        return this;
    }

    public String getInfo(String key) {
        byte[] bytes = Pdf.takeBuffer((p, n) -> FFI.C.pdf_editable_get_info(h(), key, p, n));
        return new String(bytes, StandardCharsets.UTF_8);
    }

    public EditableDoc setXmp(byte[] xml) {
        Pdf.check(FFI.C.pdf_editable_set_xmp(h(), xml, xml.length));
        return this;
    }

    public EditableDoc overlayPage(int index, byte[] content) {
        Pdf.check(FFI.C.pdf_editable_overlay_page(h(), index, content, content.length));
        return this;
    }

    /** Fill an AcroForm text field; returns whether it existed. */
    public boolean fillTextField(String name, String value) {
        IntByReference found = new IntByReference();
        Pdf.check(FFI.C.pdf_editable_fill_text_field(h(), name, value, found));
        return found.getValue() != 0;
    }

    public EditableDoc optimize() {
        Pdf.check(FFI.C.pdf_editable_optimize(h()));
        return this;
    }

    public EditableDoc compact(boolean on) {
        Pdf.check(FFI.C.pdf_editable_compact(h(), on ? 1 : 0));
        return this;
    }

    /** Encrypt on save (requires a license). */
    public EditableDoc encrypt(String user, String owner, Encryption method, boolean readOnly) {
        Pdf.check(FFI.C.pdf_editable_encrypt(h(), method.code, user, owner, readOnly ? 1 : 0));
        return this;
    }

    public byte[] toBytes() {
        return Pdf.takeBuffer((p, n) -> FFI.C.pdf_editable_to_bytes(h(), p, n));
    }

    /** Serialize as an incremental update over {@code original} (preserves it verbatim). */
    public byte[] toBytesIncremental(byte[] original) {
        return Pdf.takeBuffer((p, n) ->
                FFI.C.pdf_editable_to_bytes_incremental(h(), original, original.length, p, n));
    }

    public void save(String path) {
        Pdf.check(FFI.C.pdf_editable_save(h(), path));
    }
}
