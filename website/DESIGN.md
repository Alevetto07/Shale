# Shale marketing site — visual language

Shale is a Bambu-only slicer on CuraEngine. The site reads like the product: a build plate, layer lines, G-code rhythm, and the four **plate field** moods from the app (`Andromeda`, `Saturn`, `Nebula`, `Lunar`).

## Type

- **UI:** Inter 400/500/600 — short labels, full sentences only where needed.
- **G-code & paths:** Geist Mono — hashes, CLI, layer readouts, nozzle coordinates.
- **Display:** Inter at large sizes with tight tracking (−0.03em to −0.05em); no extra-heavy weights.

## Colour

| Token | Role |
| --- | --- |
| `--bed` | Build plate base (#06080c) |
| `--ink` | Primary text |
| `--path` | Extrusion / accent (theme tint) |
| `--wall` | Perimeter toolpaths |
| `--infill` | Hatch lines (lower contrast) |

Four field themes (user-selectable) only shift `--path`, plate grid tint, and ambient glow — surfaces stay neutral so screenshots and widgets stay readable.

## Motifs

1. **Plate grid** — orthogonal 20 mm-ish lines behind content (`repeating-linear-gradient`), like the WebGL bed.
2. **Toolpath strokes** — 1px lines, round caps; walls solid, infill dashed. Animated draw-on for hero and demo headers.
3. **ASCII frame** — `┌─┐` box drawing from the desktop `theme.box` for terminals and MCP panes, not decorative badges.
4. **Layer ticks** — horizontal rules stepped like Z layers; scrubbers use the same tick density.

## Motion

- **On load:** plate grid fades in; hero toolpath draws once (CSS `stroke-dashoffset` or canvas).
- **Scroll:** sections ease up with `motion` / CSS (`html.motion-reveal`); default **visible without JS**.
- **Interaction:** buttons scale 0.98 on press; sliders use 150ms `transition-colors`; layer playhead ~60 fps `requestAnimationFrame`.
- **`prefers-reduced-motion`:** no draw-on, no auto-play; all demos static but operable.

## Demos

Every product claim on the page is a widget. Recorded or synthetic data is labelled in the UI copy, not hidden in design notes.

## Anti-slop (unslop-ui)

No eyebrow labels, decorative numbering, or subtitle under every heading. One hero line, one CTA pair, facts from `site.ts` and repo fixtures only.