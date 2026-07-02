# Coordinate system contract — positioned stamping

This page is the **contract** for every positioned stamping primitive on
`EditableDoc`: `FillRect`, `PlaceText`, `MaskedText`, `PlaceParagraph`,
`DrawImage` (C# names; each binding mirrors them). Watermarks and redaction
are page-centered utilities and are not affected by the settings below.

## Units and origin

* All coordinates and sizes are **PDF points** (1 pt = 1/72 inch).
* The y axis grows **upward**. `(0, 0)` is a lower-left corner — *which*
  lower-left depends on the coordinate space below.

## Coordinate space: `StampSpace`

Set once per document via `EditableDoc.StampSpace` (default: `Visible`).

| | `Visible` (default) | `Media` |
|---|---|---|
| Origin | the **displayed** lower-left corner of the crop box | the raw media origin (no crop offset) |
| Page `/Rotate` | compensated — a `rotationDeg = 0` stamp reads upright on screen | ignored — coordinates are the raw PDF user space |
| `rotationDeg` means | angle in the *viewer's* frame | baseline/edge angle in *media* space |
| Matches | "draw where I see it" (issue #45 semantics) | legacy fixed-position layout engines and any code that already does its own `/Rotate` math |

A page scanned in landscape usually carries `/Rotate 90`. The same stamp call
lands differently in each space:

```
        media space (raw)                 visible space (screen)
      ┌───────────────┐ ▲                ┌─────────────────────────┐
      │ T             │ │ media          │  ◄T                     │
      │ e   /Rotate 90│ │ y              │     (text reads upright │
      │ x             │ │                │      because the /Rotate│
      │ t             │ │                │      was compensated)   │
      └───────────────┘ │                └─────────────────────────┘
      Media: rotationDeg 0 = horizontal   Visible: rotationDeg 0 =
      in the FILE (sideways on screen).   horizontal on SCREEN.
```

If you are porting coordinates computed for legacy PDF libraries (X↔Y swap + angle =
page rotation), use `Media` — nothing is composed on top of your math.

> **Default note:** `Visible` is the historical default and will not change
> within 0.x. `Media` is the least surprising space for code that treats the
> PDF as a file format; it may become the default in a future major release
> (with `Visible` opt-in).

## Rotation pivot

`rotationDeg` always rotates **counter-clockwise about the anchor `(x, y)`**
of the call — the anchor point is invariant under rotation:

* `PlaceText` — the anchor resolved by `VerticalAnchor` (baseline point by
  default). Alignment shifts (`Align.Center`/`Right`) and vertical-anchor
  offsets rotate **with** the text, so `(x, y)` stays the same physical point
  of the text box at any angle.
* `PlaceParagraph` — the block anchor; the whole laid-out block (all lines)
  rotates as a unit about `(x, y)`.
* `DrawImage` — the image's **lower-left corner** by default
  (`ImageAnchor.Corner`); the image sweeps around that corner. Pass
  `ImageAnchor.BoundingBox` to land the **rotated image's bounding box** with
  its lower-left at `(x, y)` instead (bounding-box layout semantics — the pixels
  always sit at/above/right of the anchor; a 90° image occupies
  `[x, x+height] × [y, y+width]`).

When porting rotated stamps from legacy layout engines, let the anchor apply the vertical
offset: call with the box-bottom `y` and `VerticalAnchor.LineBottom` (the
offset then rotates with the text, exactly like legacy PDF engines). Pre-adding the offset
to `y` and using `Baseline` applies it unrotated and drifts by
`offset · (sin θ, 1 − cos θ)`.

> **Legacy paragraph rotation note.** Some legacy layout engines give their
> paragraph element **default 4 pt top/bottom margins**, and rotation pivots at
> the corner of the occupied area — which includes that margin. So a rotated
> legacy paragraph effectively pivots **4 pt below** its fixed-position anchor
> (a constant, size-independent drift of `4 · (sin θ, 1 − cos θ)` versus an
> anchor-pivoted stamp; at θ = 0 nothing changes). To reproduce it here, shift
> the anchor: `x' = x − 4·sin θ`, `y' = y − 4·(1 − cos θ)` — or zero the
> paragraph margins on the legacy side. This library intentionally does not
> bake that margin into any anchor.

## Vertical anchors (`VerticalAnchor`) — which one to use

Two families, one axis:

| Anchor | `y` means | Use when |
|---|---|---|
| `Baseline` *(default)* | the text baseline | you know exactly where the baseline goes (typographic control) |
| `Top` | top = **ascender line** (geometric hhea metrics) | hanging text below a known edge, plain geometry |
| `Bottom` | bottom = **descender line** (geometric) | resting text above a known edge, plain geometry |
| `LineTop` | top of the **layout line box** (OS/2 win metrics — or typo × 1.2 — + 0.21 em half-leading each side) | matching legacy fixed-position layout output to ≤ 0.1 pt |
| `LineBottom` | bottom of the layout line box | same, anchored by the bottom |

Rule of thumb: `Top`/`Bottom` are clean font geometry (ascender/descender —
what a designer expects); `Line*` reproduce the line box of legacy layout
engines and exist for drop-in parity. That legacy leading model is **only**
behind the `Line*` anchors — the defaults never inherit it.

For `PlaceParagraph`, `Bottom`/`LineBottom` are **bottom-pinned**: the block's
bottom rests on `y` and grows upward by its real content height; `maxHeight`
is a ceiling that cuts overflowing lines from the **top** (the last lines stay
pinned) and never inflates the position. The `Line*` anchors also use the
legacy multiplied-leading **advance** (selected metrics + 0.35 em) as the
baseline-to-baseline distance, so wrapped blocks match legacy line spacing; the
geometric anchors keep the plain 1.2 em leading.

## `MaskedText` box alignment

`VerticalAlign` places the single line inside the box: `Top` (line box hangs
from the top edge — top line-alignment in rectangle-based text APIs), `Middle` (default,
cap-height centered), `Bottom` (descender on the bottom edge). `padding`
controls the horizontal inset for `Left`/`Right` alignment (default
`min(0.15 × size, width / 4)`; pass `0` to start flush at the box edge like
rectangle-based DrawString APIs).
