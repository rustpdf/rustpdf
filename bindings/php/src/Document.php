<?php

declare(strict_types=1);

namespace RustPdf;

/** A PDF document being authored. Call close() (or let it be destroyed) to free. */
final class Document
{
    private \FFI $ffi;
    private ?\FFI\CData $h;

    public function __construct()
    {
        $this->ffi = Ffi::get();
        $this->h = $this->ffi->pdf_document_new();
        if (\FFI::isNull($this->h)) {
            throw new PdfException('pdf_document_new returned NULL');
        }
    }

    public function close(): void
    {
        if ($this->h !== null) {
            $this->ffi->pdf_document_free($this->h);
            $this->h = null;
        }
    }

    public function __destruct()
    {
        $this->close();
    }

    private function h(): \FFI\CData
    {
        return $this->h ?? throw new PdfException('operation on a closed Document');
    }

    // ---- configuration ------------------------------------------------------

    public function pdfa(?PdfaLevel $level = null): self
    {
        Ffi::check($level === null
            ? $this->ffi->pdf_document_pdfa($this->h())
            : $this->ffi->pdf_document_pdfa_level($this->h(), $level->value));
        return $this;
    }

    public function tagged(): self
    {
        Ffi::check($this->ffi->pdf_document_tagged($this->h()));
        return $this;
    }

    public function setVersion(int $v): self
    {
        Ffi::check($this->ffi->pdf_document_set_version($this->h(), $v));
        return $this;
    }

    public function setDefaultSize(float $w, float $h): self
    {
        Ffi::check($this->ffi->pdf_document_set_default_size($this->h(), $w, $h));
        return $this;
    }

    public function setInfo(
        ?string $title = null,
        ?string $author = null,
        ?string $subject = null,
        ?string $keywords = null,
        ?string $creator = null,
    ): self {
        Ffi::check($this->ffi->pdf_document_set_info($this->h(), $title, $author, $subject, $keywords, $creator));
        return $this;
    }

    // ---- pages + graphics ---------------------------------------------------

    public function addPage(?float $width = null, ?float $height = null): self
    {
        Ffi::check($width === null || $height === null
            ? $this->ffi->pdf_document_add_page($this->h())
            : $this->ffi->pdf_document_add_page_sized($this->h(), $width, $height));
        return $this;
    }

    public function setFillRgb(float $r, float $g, float $b): self
    {
        Ffi::check($this->ffi->pdf_page_set_fill_rgb($this->h(), $r, $g, $b));
        return $this;
    }

    public function setStrokeRgb(float $r, float $g, float $b): self
    {
        Ffi::check($this->ffi->pdf_page_set_stroke_rgb($this->h(), $r, $g, $b));
        return $this;
    }

    public function setLineWidth(float $w): self
    {
        Ffi::check($this->ffi->pdf_page_set_line_width($this->h(), $w));
        return $this;
    }

    public function rect(float $x, float $y, float $w, float $h): self
    {
        Ffi::check($this->ffi->pdf_page_rect($this->h(), $x, $y, $w, $h));
        return $this;
    }

    public function fill(): self
    {
        Ffi::check($this->ffi->pdf_page_fill($this->h()));
        return $this;
    }

    public function stroke(): self
    {
        Ffi::check($this->ffi->pdf_page_stroke($this->h()));
        return $this;
    }

    // ---- fonts + text -------------------------------------------------------

    public function addFontFile(string $path): int
    {
        $id = $this->ffi->new('int');
        Ffi::check($this->ffi->pdf_document_add_font_file($this->h(), $path, \FFI::addr($id)));
        return $id->cdata;
    }

    public function addFont(string $data): int
    {
        [$buf, $len] = Ffi::bytes($data);
        $id = $this->ffi->new('int');
        Ffi::check($this->ffi->pdf_document_add_font($this->h(), $buf, $len, \FFI::addr($id)));
        return $id->cdata;
    }

    public function showText(int $font, float $size, float $x, float $y, string $text, int $headingLevel = 0): self
    {
        Ffi::check($this->ffi->pdf_page_show_text($this->h(), $font, $size, $x, $y, $text, $headingLevel));
        return $this;
    }

    public function paragraph(int $font, float $size, float $x, float $y, float $width, string $text, Align $align = Align::Left): self
    {
        Ffi::check($this->ffi->pdf_page_paragraph($this->h(), $font, $size, $x, $y, $width, $align->value, $text));
        return $this;
    }

    // ---- images -------------------------------------------------------------

    public function addImageFile(string $path): int
    {
        $id = $this->ffi->new('int');
        Ffi::check($this->ffi->pdf_document_add_image_file($this->h(), $path, \FFI::addr($id)));
        return $id->cdata;
    }

    public function addImagePng(string $data): int
    {
        [$buf, $len] = Ffi::bytes($data);
        $id = $this->ffi->new('int');
        Ffi::check($this->ffi->pdf_document_add_image_png($this->h(), $buf, $len, \FFI::addr($id)));
        return $id->cdata;
    }

    public function addImageJpeg(string $data): int
    {
        [$buf, $len] = Ffi::bytes($data);
        $id = $this->ffi->new('int');
        Ffi::check($this->ffi->pdf_document_add_image_jpeg($this->h(), $buf, $len, \FFI::addr($id)));
        return $id->cdata;
    }

    public function drawImage(int $image, float $x, float $y, float $w, float $h): self
    {
        Ffi::check($this->ffi->pdf_page_draw_image($this->h(), $image, $x, $y, $w, $h));
        return $this;
    }

    public function figure(int $image, float $x, float $y, float $w, float $h, string $alt): self
    {
        Ffi::check($this->ffi->pdf_page_figure($this->h(), $image, $x, $y, $w, $h, $alt));
        return $this;
    }

    // ---- attachments + forms ------------------------------------------------

    public function attachFile(string $name, string $mime, string $data, AFRelationship $rel = AFRelationship::Source, string $description = ''): self
    {
        [$buf, $len] = Ffi::bytes($data);
        Ffi::check($this->ffi->pdf_document_attach_file($this->h(), $name, $mime, $buf, $len, $rel->value, $description));
        return $this;
    }

    /** @param array{0: float, 1: float, 2: float, 3: float} $rect */
    public function textField(string $name, int $page, array $rect, string $value = '', float $size = 0): self
    {
        Ffi::check($this->ffi->pdf_document_text_field($this->h(), $name, $page, $rect[0], $rect[1], $rect[2], $rect[3], $value, $size));
        return $this;
    }

    /** @param array{0: float, 1: float, 2: float, 3: float} $rect */
    public function checkbox(string $name, int $page, array $rect, bool $checked): self
    {
        Ffi::check($this->ffi->pdf_document_checkbox($this->h(), $name, $page, $rect[0], $rect[1], $rect[2], $rect[3], $checked ? 1 : 0));
        return $this;
    }

    /**
     * @param array{0: float, 1: float, 2: float, 3: float} $rect
     * @param list<string> $options
     */
    public function dropdown(string $name, int $page, array $rect, array $options, ?int $selected = null, float $size = 0): self
    {
        $joined = implode("\n", $options);
        Ffi::check($this->ffi->pdf_document_dropdown($this->h(), $name, $page, $rect[0], $rect[1], $rect[2], $rect[3], $joined, $selected ?? -1, $size));
        return $this;
    }

    /**
     * @param list<array{0: array{0: float, 1: float, 2: float, 3: float}, 1: string}> $buttons
     *        each button is `[ [x0,y0,x1,y1], "exportValue" ]`
     */
    public function radioGroup(string $name, int $page, array $buttons, ?int $selected = null): self
    {
        $count = \count($buttons);
        $rects = $this->ffi->new('double[' . ($count * 4) . ']');
        $exports = $this->ffi->new("char*[$count]");
        $keep = [];
        foreach (array_values($buttons) as $i => [$rect, $export]) {
            $rects[$i * 4] = $rect[0];
            $rects[$i * 4 + 1] = $rect[1];
            $rects[$i * 4 + 2] = $rect[2];
            $rects[$i * 4 + 3] = $rect[3];
            $cs = $this->ffi->new('char[' . (\strlen($export) + 1) . ']');
            \FFI::memcpy($cs, $export, \strlen($export));
            $exports[$i] = $this->ffi->cast('char*', $cs);
            $keep[] = $cs;
        }
        Ffi::check($this->ffi->pdf_document_radio_group($this->h(), $name, $page, $count, $rects, $exports, $selected ?? -1));
        unset($keep);
        return $this;
    }

    // ---- output -------------------------------------------------------------

    public function pageCount(): int
    {
        return $this->ffi->pdf_document_page_count($this->h());
    }

    public function toBytes(): string
    {
        $h = $this->h();
        return Ffi::takeBytes(fn ($ffi, $o, $n) => $ffi->pdf_document_write($h, $o, $n));
    }

    public function save(string $path): void
    {
        Ffi::check($this->ffi->pdf_document_save($this->h(), $path));
    }
}
