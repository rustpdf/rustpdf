<?php

declare(strict_types=1);

namespace RustPdf;

/**
 * Internal: loads libpdf_ffi via PHP's FFI extension and holds low-level
 * helpers (error checking, byte-buffer marshalling). Not part of the public API.
 */
final class Ffi
{
    private static ?\FFI $ffi = null;

    public static function get(): \FFI
    {
        if (self::$ffi === null) {
            self::$ffi = \FFI::cdef(self::CDEF, self::libPath());
        }
        return self::$ffi;
    }

    private static function libPath(): string
    {
        // 1. Explicit override — always wins (advanced users, custom deploys).
        $env = getenv('RUSTPDF_LIB');
        if ($env !== false && $env !== '' && is_file($env)) {
            return $env;
        }

        $file = self::libFileName();

        // 2. The cdylib bundled into the package by the installer (Composer
        //    consumers): <package>/lib/<os>-<arch>/<file>. __DIR__ is src/.
        $packaged = Installer::libDir() . '/' . $file;
        if (is_file($packaged)) {
            return $packaged;
        }

        // 3. Development tree: cargo's target/{debug,release}, walking up.
        $dir = __DIR__;
        for ($i = 0; $i < 10; $i++) {
            foreach (['debug', 'release'] as $profile) {
                $candidate = $dir . "/target/$profile/$file";
                if (is_file($candidate)) {
                    return $candidate;
                }
            }
            $parent = \dirname($dir);
            if ($parent === $dir) {
                break;
            }
            $dir = $parent;
        }

        // 4. Last resort: download the matching prebuilt lib for this platform
        //    (no-op if disabled via RUSTPDF_NO_DOWNLOAD). Throws a descriptive
        //    PdfException listing every location tried if it can't be obtained.
        return Installer::ensure();
    }

    public static function libFileName(): string
    {
        return match (PHP_OS_FAMILY) {
            'Windows' => 'pdf_ffi.dll',
            'Darwin' => 'libpdf_ffi.dylib',
            default => 'libpdf_ffi.so',
        };
    }

    public static function lastError(): string
    {
        // A `const char *` return is auto-converted to a PHP string (null if NULL).
        $p = self::get()->pdf_last_error_message();
        return ($p === null || $p === '') ? 'unknown error' : (string) $p;
    }

    /** Throw on a non-zero PdfStatus. */
    public static function check(int $status): void
    {
        if ($status !== 0) {
            throw new PdfException(self::lastError(), $status);
        }
    }

    /**
     * Run an out-buffer producer `($ffi, $outPtr, $outLen) => int` and return
     * the produced bytes, always freeing the native buffer.
     */
    public static function takeBytes(callable $call): string
    {
        $ffi = self::get();
        $pp = $ffi->new('uint8_t*');
        $pn = $ffi->new('uintptr_t');
        self::check($call($ffi, \FFI::addr($pp), \FFI::addr($pn)));
        $len = (int) $pn->cdata;
        if ($len === 0 || \FFI::isNull($pp)) {
            return '';
        }
        $bytes = \FFI::string($pp, $len);
        $ffi->pdf_buffer_free($pp, $len);
        return $bytes;
    }

    /**
     * A `uint8_t[]` owned buffer holding `$data` (or null for empty). Keep the
     * returned CData alive for the duration of the native call.
     *
     * @return array{0: ?\FFI\CData, 1: int}
     */
    public static function bytes(string $data): array
    {
        $len = \strlen($data);
        if ($len === 0) {
            return [null, 0];
        }
        $buf = self::get()->new("uint8_t[$len]");
        \FFI::memcpy($buf, $data, $len);
        return [$buf, $len];
    }

    /**
     * Build parallel `uint8_t*[]` / `uintptr_t[]` arrays for a list of byte
     * strings (for pdf_add_dss). Returns `[ptrs, lens, keep]`; keep the
     * `keep` refs alive during the call.
     *
     * @param list<string> $items
     * @return array{0: ?\FFI\CData, 1: ?\FFI\CData, 2: list<\FFI\CData>}
     */
    public static function bytesArray(array $items): array
    {
        $n = \count($items);
        if ($n === 0) {
            return [null, null, []];
        }
        $ffi = self::get();
        $ptrs = $ffi->new("uint8_t*[$n]");
        $lens = $ffi->new("uintptr_t[$n]");
        $keep = [];
        foreach (array_values($items) as $i => $s) {
            [$buf, $len] = self::bytes($s);
            if ($buf !== null) {
                $ptrs[$i] = $ffi->cast('uint8_t*', $buf);
                $keep[] = $buf;
            }
            $lens[$i] = $len;
        }
        return [$ptrs, $lens, $keep];
    }

    private const CDEF = <<<'C'
typedef struct PdfDocument PdfDocument;
typedef struct PdfEditable PdfEditable;

const char *pdf_version(void);
const char *pdf_last_error_message(void);
int pdf_activate_license(const char *token);
void pdf_buffer_free(uint8_t *ptr, uintptr_t len);

PdfDocument *pdf_document_new(void);
void pdf_document_free(PdfDocument *doc);
int pdf_document_add_page(PdfDocument *doc);
int pdf_document_add_page_sized(PdfDocument *doc, double w, double h);
int pdf_document_page_count(PdfDocument *doc);
int pdf_page_set_fill_rgb(PdfDocument *doc, double r, double g, double b);
int pdf_page_set_stroke_rgb(PdfDocument *doc, double r, double g, double b);
int pdf_page_set_line_width(PdfDocument *doc, double w);
int pdf_page_rect(PdfDocument *doc, double x, double y, double w, double h);
int pdf_page_fill(PdfDocument *doc);
int pdf_page_stroke(PdfDocument *doc);
int pdf_document_save(PdfDocument *doc, const char *path);
int pdf_document_write(PdfDocument *doc, uint8_t **out_ptr, uintptr_t *out_len);
int pdf_document_pdfa(PdfDocument *doc);
int pdf_document_pdfa_level(PdfDocument *doc, int level);
int pdf_document_tagged(PdfDocument *doc);
int pdf_document_set_version(PdfDocument *doc, int v);
int pdf_document_set_default_size(PdfDocument *doc, double w, double h);
int pdf_document_set_info(PdfDocument *doc, const char *title, const char *author, const char *subject, const char *keywords, const char *creator);
int pdf_document_add_font_file(PdfDocument *doc, const char *path, int *out_id);
int pdf_document_add_font(PdfDocument *doc, const uint8_t *data, uintptr_t len, int *out_id);
int pdf_page_show_text(PdfDocument *doc, int font, double size, double x, double y, const char *text, int heading_level);
int pdf_page_paragraph(PdfDocument *doc, int font, double size, double x, double y, double width, int align, const char *text);
int pdf_document_add_image_file(PdfDocument *doc, const char *path, int *out_id);
int pdf_document_add_image_png(PdfDocument *doc, const uint8_t *data, uintptr_t len, int *out_id);
int pdf_document_add_image_jpeg(PdfDocument *doc, const uint8_t *data, uintptr_t len, int *out_id);
int pdf_page_draw_image(PdfDocument *doc, int image, double x, double y, double w, double h);
int pdf_page_figure(PdfDocument *doc, int image, double x, double y, double w, double h, const char *alt);
int pdf_document_attach_file(PdfDocument *doc, const char *name, const char *mime, const uint8_t *data, uintptr_t len, int relationship, const char *desc);
int pdf_document_text_field(PdfDocument *doc, const char *name, uintptr_t page, double x0, double y0, double x1, double y1, const char *value, double size);
int pdf_document_checkbox(PdfDocument *doc, const char *name, uintptr_t page, double x0, double y0, double x1, double y1, int checked);
int pdf_document_dropdown(PdfDocument *doc, const char *name, uintptr_t page, double x0, double y0, double x1, double y1, const char *options, int selected, double size);
int pdf_document_radio_group(PdfDocument *doc, const char *name, uintptr_t page, uintptr_t count, const double *rects, const char **exports, int selected);

PdfEditable *pdf_editable_load(const uint8_t *data, uintptr_t len);
PdfEditable *pdf_editable_load_password(const uint8_t *data, uintptr_t len, const char *password);
void pdf_editable_free(PdfEditable *ed);
int pdf_editable_page_count(PdfEditable *ed);
int pdf_editable_merge(PdfEditable *ed, const PdfEditable *other);
int pdf_editable_rotate_page(PdfEditable *ed, uintptr_t index, int degrees);
int pdf_editable_delete_page(PdfEditable *ed, uintptr_t index);
int pdf_editable_reorder_pages(PdfEditable *ed, const uintptr_t *order, uintptr_t count);
int pdf_editable_extract_pages(PdfEditable *ed, const uintptr_t *indices, uintptr_t count, PdfEditable **out_ed);
int pdf_editable_set_info(PdfEditable *ed, const char *key, const char *value);
int pdf_editable_get_info(PdfEditable *ed, const char *key, uint8_t **out_ptr, uintptr_t *out_len);
int pdf_editable_set_xmp(PdfEditable *ed, const uint8_t *xml, uintptr_t len);
int pdf_editable_overlay_page(PdfEditable *ed, uintptr_t index, const uint8_t *content, uintptr_t len);
int pdf_editable_fill_text_field(PdfEditable *ed, const char *name, const char *value, int *out_found);
int pdf_editable_optimize(PdfEditable *ed);
int pdf_editable_compact(PdfEditable *ed, int on);
int pdf_editable_encrypt(PdfEditable *ed, int method, const char *user, const char *owner, int read_only);
int pdf_editable_to_bytes(PdfEditable *ed, uint8_t **out_ptr, uintptr_t *out_len);
int pdf_editable_to_bytes_incremental(PdfEditable *ed, const uint8_t *original, uintptr_t original_len, uint8_t **out_ptr, uintptr_t *out_len);
int pdf_editable_save(PdfEditable *ed, const char *path);

int pdf_extract_text(const uint8_t *data, uintptr_t len, uint8_t **out_ptr, uintptr_t *out_len);
int pdf_extract_images_to_dir(const uint8_t *data, uintptr_t len, const char *dir, uintptr_t *out_count);
int pdf_render_page_to_png(const uint8_t *data, uintptr_t len, uintptr_t page_index, double dpi, uint8_t **out_ptr, uintptr_t *out_len);
int pdf_page_count(const uint8_t *data, uintptr_t len, uintptr_t *out_count);
int pdf_sign(const uint8_t *pdf, uintptr_t pdf_len, const uint8_t *key_der, uintptr_t key_len, const uint8_t *cert_der, uintptr_t cert_len, const char *reason, const char *location, const char *name, int pades, uint8_t **out_ptr, uintptr_t *out_len);
int pdf_timestamp(const uint8_t *pdf, uintptr_t pdf_len, const uint8_t *key_der, uintptr_t key_len, const uint8_t *cert_der, uintptr_t cert_len, const char *date, uint8_t **out_ptr, uintptr_t *out_len);
int pdf_add_dss(const uint8_t *pdf, uintptr_t pdf_len, const uint8_t **cert_ptrs, const uintptr_t *cert_lens, uintptr_t cert_count, const uint8_t **crl_ptrs, const uintptr_t *crl_lens, uintptr_t crl_count, uint8_t **out_ptr, uintptr_t *out_len);

int pdf_page_link_uri(PdfDocument *doc, double x0, double y0, double x1, double y1, const char *uri);
int pdf_page_link_to_page(PdfDocument *doc, double x0, double y0, double x1, double y1, uintptr_t target_page, double top, int has_top);
int pdf_document_add_bookmarks(PdfDocument *doc, uintptr_t count, const int *levels, const char *const *titles, const uintptr_t *pages, const double *tops, const int *has_tops);
int pdf_document_facturx(PdfDocument *doc, const uint8_t *xml, uintptr_t len, int profile);
int pdf_editable_set_checkbox(PdfEditable *ed, const char *name, int checked, int *out_found);
int pdf_editable_set_radio(PdfEditable *ed, const char *name, const char *export_value, int *out_found);
int pdf_editable_set_choice(PdfEditable *ed, const char *name, const char *value, int *out_found);
int pdf_editable_flatten_forms(PdfEditable *ed);
int pdf_editable_field_names(const PdfEditable *ed, uint8_t **out_ptr, uintptr_t *out_len);
int pdf_editable_watermark_text(PdfEditable *ed, const char *text, double size, double r, double g, double b, double opacity, double rotation_deg);
int pdf_editable_watermark_image_file(PdfEditable *ed, const char *path, double width, double height, double opacity);
int pdf_editable_redact(PdfEditable *ed, uintptr_t index, const double *rects, uintptr_t count, int *out_found);
int pdf_editable_convert_to_pdfa(PdfEditable *ed, int level);
int pdf_verify_signatures_json(const uint8_t *data, uintptr_t len, uint8_t **out_ptr, uintptr_t *out_len);

typedef struct {
    const char *reason;
    const char *location;
    const char *name;
    int pades;
    int certification;
    uintptr_t estimated_size;
    const char *policy_oid;
    const uint8_t *policy_hash;
    uintptr_t policy_hash_len;
    const char *policy_hash_alg_oid;
    const char *policy_uri;
} PdfSigningOptions;

typedef int (*PdfSignHashFn)(void *ctx, const uint8_t *data, uintptr_t data_len, uint8_t *sig_buf, uintptr_t sig_cap, uintptr_t *sig_len);

int pdf_sign_begin(const uint8_t *pdf, uintptr_t pdf_len, const PdfSigningOptions *params, uint8_t **out_doc, uintptr_t *out_doc_len, uint8_t **out_tbs, uintptr_t *out_tbs_len);
int pdf_sign_complete(const uint8_t *document, uintptr_t document_len, const uint8_t *container, uintptr_t container_len, uint8_t **out_ptr, uintptr_t *out_len);
int pdf_sign_with(const uint8_t *pdf, uintptr_t pdf_len, const uint8_t *cert_der, uintptr_t cert_len, const uint8_t *const *chain_ptrs, const uintptr_t *chain_lens, uintptr_t chain_count, const PdfSigningOptions *params, PdfSignHashFn callback, void *ctx, uint8_t **out_ptr, uintptr_t *out_len);
int pdf_list_signatures(const uint8_t *pdf, uintptr_t pdf_len, uint8_t **out_ptr, uintptr_t *out_len);
C;
}
