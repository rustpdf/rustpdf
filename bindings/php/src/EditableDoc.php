<?php

declare(strict_types=1);

namespace RustPdf;

/** An existing PDF loaded for manipulation. Call close() to free. */
final class EditableDoc
{
    private \FFI $ffi;
    private ?\FFI\CData $h;

    private function __construct(?\FFI\CData $handle)
    {
        $this->ffi = Ffi::get();
        // A failed native load returns a NULL pointer, which PHP-FFI surfaces as
        // PHP null. Accept it here (nullable param) so we raise a clean
        // PdfException with the core's message — not a cryptic TypeError.
        if ($handle === null || \FFI::isNull($handle)) {
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
        bool $opaqueBackground = false,
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
            $opaqueBackground ? 1 : 0,
        ));
        return $this;
    }

    /** Stamp an image watermark (from a file) across every page. */
    public function watermarkImageFile(
        string $path,
        float $width,
        float $height,
        float $opacity = 0.30,
        float $rotationDeg = 0.0,
    ): self {
        Ffi::check($this->ffi->pdf_editable_watermark_image_file($this->h(), $path, $width, $height, $opacity, $rotationDeg));
        return $this;
    }

    /**
     * Set the output PDF version (downgrade/normalize). Version codes:
     * `0`=1.4, `1`=1.5, `2`=1.7, `3`=2.0.
     */
    public function setVersion(int $version): self
    {
        Ffi::check($this->ffi->pdf_editable_set_version($this->h(), $version));
        return $this;
    }

    /** Strip PDF/A conformance (OutputIntents, XMP pdfaid, /Version). */
    public function stripPdfa(): self
    {
        Ffi::check($this->ffi->pdf_editable_strip_pdfa($this->h()));
        return $this;
    }

    /** Normalize to a plain PDF at `$version` (strip PDF/A + set version). */
    public function normalize(int $version = 2): self
    {
        Ffi::check($this->ffi->pdf_editable_normalize($this->h(), $version));
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

    /**
     * Paint a filled rectangle on page `$pageIndex` (0-based). Coordinates are
     * in the page's VISIBLE space (origin lower-left, y up), regardless of any
     * `/Rotate`. `$color` is RGB in 0..1; `$opacity` in 0..1.
     *
     * @param array{0: float, 1: float, 2: float} $color RGB in 0..1
     * @return bool whether the page existed
     */
    public function fillRect(
        int $pageIndex,
        float $x,
        float $y,
        float $width,
        float $height,
        array $color = [1.0, 1.0, 1.0],
        float $opacity = 1.0,
    ): bool {
        $found = $this->ffi->new('int');
        Ffi::check($this->ffi->pdf_editable_fill_rect(
            $this->h(),
            $pageIndex,
            $x,
            $y,
            $width,
            $height,
            $color[0],
            $color[1],
            $color[2],
            $opacity,
            \FFI::addr($found),
        ));
        return $found->cdata !== 0;
    }

    /**
     * Draw a line of positioned text with its baseline at `($x, $y)` on page
     * `$pageIndex` (0-based), in standard Helvetica. Coordinates are in the
     * page's VISIBLE space (origin lower-left, y up), regardless of any
     * `/Rotate`. `$rotationDeg` rotates the text counter-clockwise about the
     * anchor `($x, $y)`. `$align` shifts the start point along the baseline so
     * the text is left/right/center aligned about `($x, $y)`. `$color` is RGB
     * in 0..1.
     *
     * Pass `$fontId` from addFontFile()/addFont() to stamp with an embedded
     * TrueType/OpenType font; leave it at `-1` for the built-in Helvetica.
     * `$anchor` says what `$y` means: `VerticalAnchor::Baseline` (default,
     * historical behavior), `Top` (text hangs from `$y` — the baseline lands
     * `ascent x size` below it, matching legacy fixed-position layout), `Bottom`
     * (the descender line rests on `$y`), or `LineTop`/`LineBottom` (the legacy layout engines
     * line box). Ascent/descent come from the selected font's metrics.
     *
     * @param array{0: float, 1: float, 2: float} $color RGB in 0..1
     * @return bool whether the page (and font) existed
     */
    public function placeText(
        int $pageIndex,
        float $x,
        float $y,
        string $text,
        float $size = 12.0,
        array $color = [0.0, 0.0, 0.0],
        float $rotationDeg = 0.0,
        Align $align = Align::Left,
        int $fontId = -1,
        VerticalAnchor $anchor = VerticalAnchor::Baseline,
    ): bool {
        $found = $this->ffi->new('int');
        Ffi::check($this->ffi->pdf_editable_place_text_anchored(
            $this->h(),
            $pageIndex,
            $x,
            $y,
            $text,
            $size,
            $color[0],
            $color[1],
            $color[2],
            $rotationDeg,
            $align->value,
            $anchor->value,
            $fontId,
            \FFI::addr($found),
        ));
        return $found->cdata !== 0;
    }

    /**
     * Register a TrueType/OpenType font (from a file path) for text stamping;
     * returns a `fontId` usable with the `$fontId` parameter of placeText() /
     * maskedText() / placeParagraph(). The font is embedded as a subset —
     * stamped text renders with the real font's glyphs and metrics, exactly
     * like Document::addFontFile() + showText().
     */
    public function addFontFile(string $path): int
    {
        $id = $this->ffi->new('int');
        Ffi::check($this->ffi->pdf_editable_add_font_file($this->h(), $path, \FFI::addr($id)));
        return $id->cdata;
    }

    /** Register a stamping font from raw TrueType/OpenType bytes. See addFontFile(). */
    public function addFont(string $data): int
    {
        [$buf, $len] = Ffi::bytes($data);
        $id = $this->ffi->new('int');
        Ffi::check($this->ffi->pdf_editable_add_font($this->h(), $buf, $len, \FFI::addr($id)));
        return $id->cdata;
    }

    /**
     * Choose the coordinate space of the positioned stamping primitives
     * (fillRect, placeText, maskedText, placeParagraph, drawImage) for
     * subsequent calls. `StampSpace::Visible` (default) keeps the historical
     * behavior — coordinates in the page's displayed space, compensating
     * `/Rotate`. `StampSpace::Media` interprets coordinates and `rotationDeg`
     * in the raw PDF user space (legacy layout semantics), never composing with the
     * page's `/Rotate` — use it to reproduce legacy-engine placement on
     * rotated/scanned pages. Watermarks and redaction are unaffected.
     */
    public function setStampSpace(StampSpace $space): self
    {
        Ffi::check($this->ffi->pdf_editable_set_stamp_space($this->h(), $space->value));
        return $this;
    }

    /**
     * Stamp a paragraph with automatic word wrapping: `$text` is broken into
     * lines that fit `$width` points (greedy, by word; `"\n"` forces a break)
     * and drawn downward from `($x, $y)`. `$anchor` says what `$y` means for
     * the block: `VerticalAnchor::Top` (default) — top of the box, the first
     * baseline lands `ascent x size` below `$y` (legacy fixed-position layout);
     * `Baseline` — the first line's baseline; `Bottom`/`LineBottom` —
     * bottom-pinned: the block's bottom rests on `$y` and grows upward (with
     * `$maxHeight` the box is `[y, y+maxHeight]` and overflowing lines are cut
     * from the top). `$maxHeight` truncates lines whose descender would cross
     * the limit (`null` = unlimited); `$lineHeight` scales the default
     * `1.2 x size` leading. Pass `$fontId` from addFontFile()/addFont() to
     * wrap and draw with an embedded font (its real metrics drive the break
     * points); `-1` uses the built-in Helvetica. `$rotationDeg` rotates the
     * laid-out block counter-clockwise about the anchor.
     *
     * @param array{0: float, 1: float, 2: float} $color RGB in 0..1
     * @return bool whether the page (and font) existed and the box was valid
     */
    public function placeParagraph(
        int $pageIndex,
        float $x,
        float $y,
        float $width,
        string $text,
        float $size = 12.0,
        array $color = [0.0, 0.0, 0.0],
        Align $align = Align::Left,
        int $fontId = -1,
        ?float $maxHeight = null,
        float $lineHeight = 1.0,
        VerticalAnchor $anchor = VerticalAnchor::Top,
        float $rotationDeg = 0.0,
    ): bool {
        $found = $this->ffi->new('int');
        Ffi::check($this->ffi->pdf_editable_place_paragraph_anchored(
            $this->h(),
            $pageIndex,
            $x,
            $y,
            $width,
            $text,
            $size,
            $color[0],
            $color[1],
            $color[2],
            $align->value,
            $anchor->value,
            $fontId,
            $maxHeight ?? 0.0,
            $lineHeight,
            $rotationDeg,
            null,
            null,
            \FFI::addr($found),
        ));
        return $found->cdata !== 0;
    }

    /**
     * Like placeParagraph() but returns both the number of lines drawn and the
     * consumed block height in points (top of the first drawn line's box to
     * the bottom of the last one's; 0 when nothing fit) — stack blocks without
     * re-measuring.
     *
     * @param array{0: float, 1: float, 2: float} $color RGB in 0..1
     * @return array{lines: int, height: float}
     */
    public function placeParagraphMeasured(
        int $pageIndex,
        float $x,
        float $y,
        float $width,
        string $text,
        float $size = 12.0,
        array $color = [0.0, 0.0, 0.0],
        Align $align = Align::Left,
        int $fontId = -1,
        ?float $maxHeight = null,
        float $lineHeight = 1.0,
        VerticalAnchor $anchor = VerticalAnchor::Top,
        float $rotationDeg = 0.0,
    ): array {
        $height = $this->ffi->new('double');
        $lines = $this->ffi->new('int');
        $found = $this->ffi->new('int');
        Ffi::check($this->ffi->pdf_editable_place_paragraph_anchored(
            $this->h(),
            $pageIndex,
            $x,
            $y,
            $width,
            $text,
            $size,
            $color[0],
            $color[1],
            $color[2],
            $align->value,
            $anchor->value,
            $fontId,
            $maxHeight ?? 0.0,
            $lineHeight,
            $rotationDeg,
            \FFI::addr($height),
            \FFI::addr($lines),
            \FFI::addr($found),
        ));
        return ['lines' => $lines->cdata, 'height' => $height->cdata];
    }

    /**
     * Draw `$text` over an opaque background box `[$x, $y, $x+$width, $y+$height]`
     * on page `$pageIndex` (0-based): fills the box in `$bgColor`, then writes the
     * text (standard Helvetica, `$size` points, `$textColor`) horizontally aligned
     * per `$align` and vertically centered within the box. The classic use is
     * masking a placeholder and stamping the real value over it without
     * hand-computing the baseline. Coordinates are in the page's VISIBLE space
     * (origin lower-left, y up).
     *
     * Pass `$fontId` from addFontFile()/addFont() to stamp with an embedded
     * font; `-1` uses the built-in Helvetica. `$valign` controls the vertical
     * alignment of the line inside the box: `VerticalAlign::Middle` (default,
     * historical cap-height centering), `Top` (line hangs from the top edge —
     * baseline at `y + height - ascent x size`, legacy PDF libraries
     * top line-alignment in rectangle-based text APIs), or `Bottom` (descender line rests on the bottom
     * edge). `$padding` is the horizontal edge inset (points) for Left/Right
     * alignment: text starts at `x + padding` (or ends at
     * `x + width - padding`). `null` keeps the historical
     * `min(0.15 x size, width / 4)`; pass `0.0` to start flush with the box
     * edge like rectangle-based DrawString APIs.
     *
     * @param array{0: float, 1: float, 2: float} $textColor RGB in 0..1 (default black)
     * @param array{0: float, 1: float, 2: float} $bgColor   RGB in 0..1 (default white)
     * @return bool whether the page (and font) existed
     */
    public function maskedText(
        int $pageIndex,
        float $x,
        float $y,
        float $width,
        float $height,
        string $text,
        float $size = 12.0,
        array $textColor = [0.0, 0.0, 0.0],
        array $bgColor = [1.0, 1.0, 1.0],
        Align $align = Align::Left,
        int $fontId = -1,
        VerticalAlign $valign = VerticalAlign::Middle,
        ?float $padding = null,
    ): bool {
        $found = $this->ffi->new('int');
        Ffi::check($this->ffi->pdf_editable_masked_text_pad(
            $this->h(),
            $pageIndex,
            $x,
            $y,
            $width,
            $height,
            $text,
            $size,
            $textColor[0],
            $textColor[1],
            $textColor[2],
            $bgColor[0],
            $bgColor[1],
            $bgColor[2],
            $align->value,
            $valign->value,
            $padding ?? -1.0,
            $fontId,
            \FFI::addr($found),
        ));
        return $found->cdata !== 0;
    }

    /**
     * Draw an image (PNG or JPEG bytes) onto page `$pageIndex` (0-based) with
     * its lower-left corner at `($x, $y)`, scaled to `$width`x`$height` points.
     * Coordinates are in the page's VISIBLE space (origin lower-left, y up),
     * regardless of any `/Rotate`. `$rotationDeg` rotates the image
     * counter-clockwise about the anchor `($x, $y)`. `$image` is a binary
     * string (the core dispatches on the signature).
     *
     * `$anchor` controls how a rotated image is anchored:
     * `ImageAnchor::Corner` (default) rotates the image about its own
     * lower-left corner at `($x, $y)`; `ImageAnchor::BoundingBox` lands the
     * rotated image's bounding box with its lower-left at `($x, $y)` (legacy layout engines
     * layout semantics — e.g. a 90 degree image occupies
     * `[x, x+height] x [y, y+width]`).
     *
     * @return bool whether the page existed
     */
    public function drawImage(
        int $pageIndex,
        string $image,
        float $x,
        float $y,
        float $width,
        float $height,
        float $rotationDeg = 0.0,
        ImageAnchor $anchor = ImageAnchor::Corner,
    ): bool {
        [$buf, $len] = Ffi::bytes($image);
        $found = $this->ffi->new('int');
        Ffi::check($this->ffi->pdf_editable_draw_image_anchored(
            $this->h(),
            $pageIndex,
            $buf,
            $len,
            $x,
            $y,
            $width,
            $height,
            $rotationDeg,
            $anchor->value,
            \FFI::addr($found),
        ));
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
