# NetSentinel Design System — "Instrument Panel"

> A design language written for this product: a monitoring dashboard read in a
> browser, on any OS, for hours at a time.
>
> **Token source of truth:** `web/app/globals.css`
> **Primitives:** `web/app/components/ui/index.tsx`

## The one rule

> **Color is a signal, never decoration.**

Chrome — canvas, surfaces, borders, text, and the primary button — is
achromatic. Saturation is spent only on `ok` / `warn` / `crit`, plus `link` for
links and focus rings. Everything else in this document follows from that.

The practical consequence: the primary action is **ink**, not a hue. A page with
no problems has no saturated pixels on it, so the first colored thing your eye
lands on is always something that needs attention.

---

## Why not Material Design 3

The previous system was M3. M3 is a good system, but it is a *phone OS* language,
and its defaults work against a desktop dashboard:

| M3 default | Problem here |
|---|---|
| Pill buttons (`corner-full`), 40–48dp tall | Reads as a mobile app; eats vertical space in dense toolbars |
| Tonal surfaces tinted with the seed color | Tinted greys fight with status colors |
| Elevation via shadow + tinted surface | Shadow on every card turns a dense screen to mud |
| 12% state layers, large touch targets everywhere | Sized for fingers, not for a pointer on a 1440px screen |
| Typescale bottoming out at 11px with wide jumps | Dashboards need a tight 11–22 ladder with tabular numerals |

Density is handled by input device, not platform: controls are 30px, and grow to
40px only under `@media (pointer: coarse)`.

---

## 1. Color

Neutrals are biased ~190° at 3–5% chroma — a whisper of blue-green. Pure grey
reads unconsidered; cool blue-grey is everywhere; this reads "instrument".

### 1.1 Light

| Token | Value | Role | Contrast |
|---|---|---|---|
| `--canvas` | `#F5F7F7` | Page ground | — |
| `--surface` | `#FFFFFF` | Panels, cards, table body | — |
| `--inset` | `#EDF0F0` | Table headers, hover, code blocks | — |
| `--hairline` | `#DFE4E4` | The 1px line that carries all structure | — |
| `--rule` | `#C7CFCF` | Borders that must read (inputs, secondary buttons) | — |
| `--ink` | `#14181A` | Body text, primary button ground | 16.9:1 |
| `--slate` | `#55605F` | Secondary text | 6.5:1 |
| `--muted` | `#6B7574` | Labels, metadata, host keys | 4.7:1 |
| `--faint` | `#939C9B` | Placeholders, disabled, em-dashes | decorative only |

### 1.2 Signal (light)

| Token | Value | Meaning | Contrast |
|---|---|---|---|
| `--ok` | `#2F6B4A` | Healthy · running · under 60% | 6.3:1 |
| `--warn` | `#8A6415` | Attention · 60–85% | 5.4:1 |
| `--crit` | `#A6413A` | Critical · offline · over 85% | 6.1:1 |
| `--link` | `#2C5C84` | Links, focus, selection — **nothing else** | 7.0:1 |

Each has a `-bg` counterpart (`--ok-bg`, `--warn-bg`, `--crit-bg`, `--link-bg`)
for badge and banner grounds.

### 1.3 Dark

Tuned independently, never inverted. The canvas is off-black (`#0D0F10`) to avoid
halation, and surfaces sit **brighter** than the canvas so the ladder still
reads. Signal colors lighten but keep their low saturation:
`--ok #62A97F` (6.4:1) · `--warn #C79F4E` (8.1:1) · `--crit #D9756A` (6.7:1).

All body-text pairings clear WCAG AA (4.5:1) in both themes.

### 1.4 Compatibility aliases

`globals.css` maps the legacy (`--bg-card`, `--accent-blue`, …) and Material
(`--md-sys-color-*`) names onto the tokens above. That is what let the whole app
re-skin without every page being rewritten at once. **New code must use the
canonical tokens.** The alias block is a migration bridge, not API.

---

## 2. Typography

Faces are unchanged: **IBM Plex Sans KR** for UI text and **IBM Plex Mono**
for values, IDs, and paths, both wired through `next/font` in `layout.tsx`.

Always reach for `var(--font-sans)` / `var(--font-code)`, never the raw
`--font-ibm-plex-sans` / `--font-mono` variables. The tokens carry an inline
`var()` fallback; a bare reference that fails to resolve makes the whole custom
property invalid at computed-value time, which silently voids every `font:`
shorthand built on the typescale.

Six sizes, three weights — 400, 500, 600. Nothing heavier: 700/800 is what made
the old UI shout.

| Token | Size | Use |
|---|---|---|
| `--fs-page` | 22px / 600 / −0.02em | Page title |
| `--fs-head` | 17px / 600 / −0.016em | Section heading |
| `--fs-item` | 14px / 600 | Record name, tile value |
| `--fs-body` | 13px / 400 | Body, table cells |
| `--fs-meta` | 12px / 400 | Metadata, buttons, labels |
| `--fs-micro` | 11px / 600 / +0.055em uppercase | Column headers, eyebrows |

**Every digit that can line up in a column gets
`font-variant-numeric: tabular-nums`.** Use the `.num` or `.mono` helper. A value
that reflows as it ticks is not a readout.

---

## 3. Shape

`--r-xs: 4px` (chips, count badges) · `--r-sm: 6px` (buttons, inputs, status
badges) · `--r-md: 8px` (panels, cards) · `--r-lg: 10px` (modals) ·
`--r-full` (status dots only).

**No pills.** `--r-full` on anything wider than it is tall is a bug.

---

## 4. Elevation

Hierarchy is a hairline plus a background step. In order: `--canvas` →
`--surface` → `--inset`.

Shadows only for things that genuinely float:
`--shadow-pop` (popovers, selected segment) and `--shadow-float` (drawer, modal).
A card never has a shadow.

---

## 5. Motion

Two durations and one curve: `--t-fast: 120ms` (state change),
`--t-enter: 200ms` (enter/exit), `--ease: cubic-bezier(.2, 0, 0, 1)`.

**Animation is reserved for things that are actually wrong.** The only looping
animation in the app is `.pulse-dot[data-firing]`, used for an alert that is
currently firing. Healthy state stays still — the previous UI pulsed a dot for
every *healthy* host, spending the user's attention to say "fine".

`prefers-reduced-motion: reduce` disables it, and theme switching sets
`data-theme-switching` for one frame so the palette lands instead of smearing.

---

## 6. Spacing & density

4px grid: `--md-sys-spacing-xs` 4 · `sm` 8 · `md` 12 · `lg` 16 · `xl` 20 ·
`2xl` 28 · `3xl` 40.

Table rows are 34px with 8/12px cell padding. Controls are `--h-control` 30px
(`--h-control-sm` 25px), growing to 40/34px under `@media (pointer: coarse)`.

Content is capped at `--content-max` (1280px) — **one width for every route**.
`.page-content` owns the vertical rhythm between sections; do not add margins
between siblings.

Breakpoints: 599px (compact) · 839px (medium) · 1200px+ (expanded).

---

## 7. Primitives

Import from `@/app/components/ui`. If a thing appears on two screens it belongs
here, not in an inline `style={{}}`.

| Component | Notes |
|---|---|
| `Button` | `primary` (ink) · `secondary` · `ghost` · `danger`; `size="sm"`, `icon` |
| `Panel` / `PanelHeader` | Bordered surface; `padded` for form bodies |
| `StatusDot` | `ok` / `warn` / `crit` / `off`; `firing` to animate |
| `Badge` | `ok` / `warn` / `crit` / `mute` |
| `Meter` | Threshold color from `meterTone()` — never hardcode |
| `Segmented` | The one tab/toggle language |
| `EmptyState` | `tone="error"` variant included |
| `Field` | Label + control + hint |
| `StatTile` | Summary numbers |
| `SkeletonRows` | Loading |

Thresholds live in `web/app/lib/status.ts`: `meterTone()` (60% / 85%) and
`uptimeTone()` (99.5% / 95%). Every surface reads from these so the same number
never means two different things on two pages.

---

## 8. Rules

Violations should be fixed before merge.

| ❌ Never | ✅ Instead |
|---|---|
| Hardcoded color (`#3B82F6`, `rgba(...)`) | `var(--ok)`, `var(--ink)`, … |
| Raw `font-size: 14px` | `var(--fs-item)` |
| `font-weight: 700` or `800` | `600` |
| `border-radius: 12px` | `var(--r-md)` |
| `border-radius: 9999px` on a wide element | `var(--r-sm)` |
| `box-shadow` on a card | A background step and a hairline |
| Saturated color on chrome | Achromatic; save hue for state |
| A new looping animation | Only `data-firing` animates |
| `style={{}}` for anything reusable | A primitive in `components/ui` |
| Per-page threshold math | `meterTone()` / `uptimeTone()` |
| A page-local `max-width` | `--content-max` |

---

## 9. Known remaining work

- `.alerts-*` in `globals.css` has been folded onto the shared buttons, tabs and
  tiles, but its panels, matrix and drawer still have bespoke rules that could
  collapse into `Panel` / `DataTable`.
- No lint rule enforces §8 yet. Adding `react/forbid-dom-props` for `style` plus
  a stylelint check for raw px/hex is the next step.
