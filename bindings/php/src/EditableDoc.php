<?php

declare(strict_types=1);

namespace RustPdf;

/** An existing PDF loaded for manipulation. Call close() to free. */
final class EditableDoc
{
    private \FFI $ffi;
    private ?\FFI\CData $h;

    private function __construct(\FFI\CData $handle)
    {
        $this->ffi = Ffi::get();
        if (\FFI::isNull($handle)) {
            throw new PdfException(Ffi::lastError());
        }
        $this->h = $handle;
    }

    /** Load a PDF from bytes (optionally with a password). */
    public static function load(string $data, ?string $password = null): self
    {
        $ffi = Ffi::get();
        [$buf, $len] = Ffi::bytes($data);
        $h = $password === null
            ? $ffi->pdf_editable_load($buf, $len)
            : $ffi->pdf_editable_load_password($buf, $len, $password);
        return new self($h);
    }

    public static function loadFile(string $path, ?string $password = null): self
    {
        $data = file_get_contents($path);
        if ($data === false) {
            throw new PdfException("could not read $path");
        }
        return self::load($data, $password);
    }

    public function close(): void
    {
        if ($this->h !== null) {
            $this->ffi->pdf_editable_free($this->h);
            $this->h = null;
        }
    }

    public function __destruct()
    {
        $this->close();
    }

    private function h(): \FFI\CData
    {
        return $this->h ?? throw new PdfException('operation on a closed EditableDoc');
    }

    public function pageCount(): int
    {
        return $this->ffi->pdf_editable_page_count($this->h());
    }

    public function merge(EditableDoc $other): self
    {
        Ffi::check($this->ffi->pdf_editable_merge($this->h(), $other->h()));
        return $this;
    }

    public function rotatePage(int $index, int $degrees): self
    {
        Ffi::check($this->ffi->pdf_editable_rotate_page($this->h(), $index, $degrees));
        return $this;
    }

    public function deletePage(int $index): self
    {
        Ffi::check($this->ffi->pdf_editable_delete_page($this->h(), $index));
        return $this;
    }

    /** @param list<int> $order */
    public function reorderPages(array $order): self
    {
        [$arr, $n] = $this->uintptrs($order);
        Ffi::check($this->ffi->pdf_editable_reorder_pages($this->h(), $arr, $n));
        return $this;
    }

    /** @param list<int> $indices */
    public function extractPages(array $indices): self
    {
        [$arr, $n] = $this->uintptrs($indices);
        $out = $this->ffi->new('PdfEditable*');
        Ffi::check($this->ffi->pdf_editable_extract_pages($this->h(), $arr, $n, \FFI::addr($out)));
        return new self($out);
    }

    public function setInfo(string $key, string $value): self
    {
        Ffi::check($this->ffi->pdf_editable_set_info($this->h(), $key, $value));
        return $this;
    }

    public function getInfo(string $key): string
    {
        $h = $this->h();
        return Ffi::takeBytes(fn ($ffi, $o, $n) => $ffi->pdf_editable_get_info($h, $key, $o, $n));
    }

    public function setXmp(string $xml): self
    {
        [$buf, $len] = Ffi::bytes($xml);
        Ffi::check($this->ffi->pdf_editable_set_xmp($this->h(), $buf, $len));
        return $this;
    }

    public function overlayPage(int $index, string $content): self
    {
        [$buf, $len] = Ffi::bytes($content);
        Ffi::check($this->ffi->pdf_editable_overlay_page($this->h(), $index, $buf, $len));
        return $this;
    }

    /** Fill an AcroForm text field; returns whether it existed. */
    public function fillTextField(string $name, string $value): bool
    {
        $found = $this->ffi->new('int');
        Ffi::check($this->ffi->pdf_editable_fill_text_field($this->h(), $name, $value, \FFI::addr($found)));
        return $found->cdata !== 0;
    }

    /** Set an AcroForm checkbox on/off; returns whether it existed. */
    public function setCheckbox(string $name, bool $checked = true): bool
    {
        $found = $this->ffi->new('int');
        Ffi::check($this->ffi->pdf_editable_set_checkbox($this->h(), $name, $checked ? 1 : 0, \FFI::addr($found)));
        return $found->cdata !== 0;
    }

    /** Select a radio button by its export value; returns whether it existed. */
    public function setRadio(string $name, string $exportValue): bool
    {
        $found = $this->ffi->new('int');
        Ffi::check($this->ffi->pdf_editable_set_radio($this->h(), $name, $exportValue, \FFI::addr($found)));
        return $found->cdata !== 0;
    }

    /** Set a choice (dropdown/list) field value; returns whether it existed. */
    public function setChoice(string $name, string $value): bool
    {
        $found = $this->ffi->new('int');
        Ffi::check($this->ffi->pdf_editable_set_choice($this->h(), $name, $value, \FFI::addr($found)));
        return $found->cdata !== 0;
    }

    /** Flatten all AcroForm fields into static page content. */
    public function flattenForms(): self
    {
        Ffi::check($this->ffi->pdf_editable_flatten_forms($this->h()));
        return $this;
    }

    /**
     * List every AcroForm field name.
     *
     * @return list<string>
     */
    public function fieldNames(): array
    {
        $h = $this->h();
        $joined = Ffi::takeBytes(fn ($ffi, $o, $n) => $ffi->pdf_editable_field_names($h, $o, $n));
        if ($joined === '') {
            return [];
        }
        return array_values(array_filter(explode("\n", $joined), static fn ($s) => $s !== ''));
    }

    /**
     * Stamp a diagonal text watermark across every page.
     *
     * @param array{0: float, 1: float, 2: float} $color RGB in 0..1
     */
    public function watermarkText(
        string $text,
        float $size = 64.0,
        array $color = [0.5, 0.5, 0.5],
        float $opacity = 0.30,
        float $rotationDeg = 45.0,
    ): self {
        Ffi::check($this->ffi->pdf_editable_watermark_text(
            $this->h(),
            $text,
            $size,
            $color[0],
            $color[1],
            $color[2],
            $opacity,
            $rotationDeg,
        ));
        return $this;
    }

    /** Stamp an image watermark (from a file) across every page. */
    public function watermarkImageFile(string $path, float $width, float $height, float $opacity = 0.30): self
    {
        Ffi::check($this->ffi->pdf_editable_watermark_image_file($this->h(), $path, $width, $height, $opacity));
        return $this;
    }

    /**
     * Redact rectangular regions on a page (content removed + black boxes).
     *
     * @param list<array{0: float, 1: float, 2: float, 3: float}> $rects
     * @return bool whether the page existed
     */
    public function redact(int $pageIndex, array $rects): bool
    {
        $count = \count($rects);
        $flat = $this->ffi->new('double[' . ($count * 4) . ']');
        foreach (array_values($rects) as $i => $r) {
            $flat[$i * 4] = $r[0];
            $flat[$i * 4 + 1] = $r[1];
            $flat[$i * 4 + 2] = $r[2];
            $flat[$i * 4 + 3] = $r[3];
        }
        $found = $this->ffi->new('int');
        Ffi::check($this->ffi->pdf_editable_redact($this->h(), $pageIndex, $flat, $count, \FFI::addr($found)));
        return $found->cdata !== 0;
    }

    /** Convert the document to PDF/A (only B-levels A1b/A2b/A3b; requires a license). */
    public function convertToPdfa(PdfaLevel $level = PdfaLevel::A2b): self
    {
        Ffi::check($this->ffi->pdf_editable_convert_to_pdfa($this->h(), $level->value));
        return $this;
    }

    public function optimize(): self
    {
        Ffi::check($this->ffi->pdf_editable_optimize($this->h()));
        return $this;
    }

    public function compact(bool $on = true): self
    {
        Ffi::check($this->ffi->pdf_editable_compact($this->h(), $on ? 1 : 0));
        return $this;
    }

    /** Encrypt on save (requires a license). */
    public function encrypt(Encryption $method = Encryption::Aes256, string $user = '', string $owner = '', bool $readOnly = false): self
    {
        Ffi::check($this->ffi->pdf_editable_encrypt($this->h(), $method->value, $user, $owner, $readOnly ? 1 : 0));
        return $this;
    }

    public function toBytes(): string
    {
        $h = $this->h();
        return Ffi::takeBytes(fn ($ffi, $o, $n) => $ffi->pdf_editable_to_bytes($h, $o, $n));
    }

    public function toBytesIncremental(string $original): string
    {
        $h = $this->h();
        return Ffi::takeBytes(function ($ffi, $o, $n) use ($h, $original) {
            [$buf, $len] = Ffi::bytes($original);
            return $ffi->pdf_editable_to_bytes_incremental($h, $buf, $len, $o, $n);
        });
    }

    public function save(string $path): void
    {
        Ffi::check($this->ffi->pdf_editable_save($this->h(), $path));
    }

    /**
     * @param list<int> $xs
     * @return array{0: ?\FFI\CData, 1: int}
     */
    private function uintptrs(array $xs): array
    {
        $n = \count($xs);
        if ($n === 0) {
            return [null, 0];
        }
        $arr = $this->ffi->new("uintptr_t[$n]");
        foreach (array_values($xs) as $i => $v) {
            $arr[$i] = $v;
        }
        return [$arr, $n];
    }
}
