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
