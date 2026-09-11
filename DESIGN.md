# Design

> Visual system for the rust-pdf marketing + docs site (`site/`). Captured from
> the real implementation in `site/public/styles.css`, `docs/docs.css`,
> `legal/legal.css`.

## Theme

Dark, engineered, restrained. A near-black navy canvas with a single warm
brand accent (orange) and a cool secondary (cyan) used sparingly for validation
/ correctness signals. Reads as "serious engineering tool", not SaaS-marketing.
Color strategy: **restrained** — tinted-dark neutrals carry the surface, one
accent for emphasis and CTAs.

## Color palette

CSS custom properties (`:root` in `styles.css`):

| Token | Value | Role |
|-------|-------|------|
| `--bg` | `#0a0e16` | Page background (near-black navy) |
| `--bg-2` | `#0d1320` | Secondary background |
| `--panel` | `#111827` | Card / panel surface |
| `--panel-2` | `#0d1422` | Inset / code surface |
| `--border` | `#1e293b` | Hairline borders, dividers |
| `--text` | `#e6edf6` | Primary text |
| `--muted` | `#94a3b8` | Secondary / muted text |
| `--brand` | `#f97316` | Brand orange — CTAs, emphasis |
| `--brand-2` | `#fb923c` | Lighter orange — headline accent, hover |
| `--accent` | `#38bdf8` | Cyan — links, focus ring, "validated" marks |
| `--green` | `#34d399` | Success / checklist ticks |

A subtle radial glow (`#15233b → transparent`, top-right) lifts the hero off the
flat background. Body text and code stay at the light end of the ramp for WCAG
AA contrast on the dark canvas. Emphasis is solid color (orange), never gradient
text.

## Typography

- **Family:** system UI stack — `-apple-system, BlinkMacSystemFont, "Segoe UI",
  Roboto, Helvetica, Arial, sans-serif`. No web-font dependency (fast, no FOUT).
- **Mono:** `"SF Mono", ui-monospace, Menlo, Consolas, monospace` for code.
- **Scale:** fluid headings via `clamp()`. Hero `h1` `clamp(2.1rem, 5.5vw,
  3.6rem)`, weight 800; section `h2` `clamp(1.5rem, 3vw, 2.1rem)`. Display
  ceiling stays well under 6rem.
- **Weights:** 800 display, 700 headings/buttons, 600 labels/nav, 400 body.
- **Wrapping:** `text-wrap: balance` on hero `h1`; `text-wrap: pretty` on leads,
  muted prose and list items. Body measure capped via `.narrow` (≈760px).
- **Letter-spacing:** `-0.02em` to `-0.03em` on large display only (never below
  the -0.04em floor).

## Layout

- Container: `.wrap` max-width 1080px, 24px gutters; `.narrow` 760px for prose.
- Sections alternate plain (`.section`) and bordered tinted (`.band`) for rhythm.
- Grids: `grid-2` (1.1fr / 0.9fr) for prose+list; `feature-grid` two cards;
  `lang-grid` four columns. All collapse to one or two columns ≤820px.
- Docs use a 3-column shell: sticky left nav, content, sticky right TOC
  (`docs.css .docs-shell.with-toc`), collapsing on narrow viewports.

## Components

- **Buttons** (`.btn`): solid orange, `#1a0e02` ink, radius 10px, weight 700;
  `.btn-ghost` transparent with border; `.btn-lg` / `.btn-sm` sizes. Hover lifts
  1px (suppressed under reduced-motion).
- **Cards** (`.card`): panel bg, hairline border, radius 14px. The featured
  card gets a warm-tinted border + soft orange shadow. No nested cards.
- **Chips/pills:** rounded `999px`, panel-2 bg, used for validators and feature
  tags. `.tag-pro` is filled orange.
- **Code blocks** (`.code`, docs): `#0b1120` surface, hairline border, optional
  uppercase label, hover-revealed Copy button. Tabbed code in `.tabs`.
- **Pricing card** (`.price-card`): single emphasized panel, large price, feature
  checklist, full-width CTA.
- **Tables** (docs/legal): hairline row borders, uppercase muted `th`.
- **Footer:** brand mark + tagline + `.footer-links` (Docs / Terms / Privacy /
  Refunds / Contact).

## Motion

- **Scroll reveal:** `.reveal` fades + rises 18px → settles, eased
  `cubic-bezier(.22,1,.36,1)`. Hidden state gated behind a `.js` class so
  content is fully visible without JS / in headless renderers. Applied to grid
  items (language tiles, feature cards, pricing) with small per-item
  `transition-delay` stagger — not a uniform whole-section fade.
- **Hover:** buttons translate 1px; cards/links color shifts.
- **Reduced motion:** `@media (prefers-reduced-motion: reduce)` disables
  reveals, hover transforms and smooth scroll, and clamps all transitions.

## Accessibility (WCAG 2.2 AA)

- Visible `:focus-visible` outline (2px cyan, 3–4px offset) on all interactive
  elements.
- Skip-to-content link (`.skip-link`) as the first focusable element.
- Semantic landmarks (`header`/`main#main`/`footer`), heading hierarchy, and a
  `lang` attribute on every page.
- Color never the sole signal; checklist ticks pair color with a glyph.
- Contrast: light text on the dark canvas, code kept legible (not muted-on-tint).

## Brand assets

- `favicon.svg` — page-with-folded-corner mark in brand orange on dark, with a
  cyan validation check.
- `og.png` (1200×630) — wordmark, headline, feature chips, language line; built
  from `og.svg`.
- Wordmark: `rust` + orange middot + `pdf` (the `·` is the brand device).
