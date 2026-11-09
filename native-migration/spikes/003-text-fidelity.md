# Spike 003: match Chromium slide text

**Result: GO on line breaking and auto-fit, with `parley` + `swash` recommended. Raster speed misses the target with the current CPU approach (25.3 ms p95 against 16 ms) and needs a decision at prompt 008.**

Measured 2026-11-09 on an Apple M2 Pro, macOS only, against Chromium 130.0.6723.191 (Electron, software compositing, device scale 1). The machine was not idle: load average was 3 to 5 because another agent session was running. Timings are therefore pessimistic and not a clean baseline. Windows and Linux were not run.

## Results

The corpus has 480 cases: 400 with text, plus 80 empty or blank-verse cases. Those 80 are trivially easy for line breaking, so the 400-case line is the one to judge.

| Metric | parley + swash | cosmic-text + swash | Target |
|---|---:|---:|---:|
| Exact line breaks, cases with text | **399/400 (99.75%)** | 399/400 (99.75%) | ≥ 99% |
| Exact line breaks, all cases | 479/480 | 479/480 | |
| Fitted size within 1 px | 200/200 | 200/200 | |
| SSIM, mean, whole frame | 0.98627 | 0.98603 | |
| SSIM, worst, whole frame | 0.81611 | 0.80003 | |
| SSIM, mean, 8×8 blocks containing text | 0.87788 | 0.88565 | |
| SSIM, worst, blocks containing text | 0.47964 | 0.44229 | |
| Raster per slide, median | 5.15 ms | 5.16 ms | |
| Raster per slide, p95 | **25.28 ms** | 25.44 ms | < 16 ms |
| Layout + raster, p95 | 26.01 ms | 26.65 ms | |

Whole-frame SSIM is dominated by empty background. The text-block figures are the honest quality measure. Raw output: [comparison.log](../evidence/003/comparison.log).

SSIM uses 8×8 non-overlapping blocks on luminance, comparing against Chromium's text over a flat #808080 background. Line comparison ignores case and collapses whitespace, because DOM Range text is untransformed while the Rust side applies `text-transform`.

## Recommendation: parley + swash

The two stacks are numerically tied, so the choice rests on behaviour:

- **Font selection.** Parley resolved the CSS family list and nearest weight directly and reached 399/400 on its first run. Cosmic-text scored 238/400 until the spike added its own family and nearest-weight lookup through `fontdb`, because an absent weight could otherwise select an exact weight from an unrelated fallback family. Prompt 052 would have to carry that workaround.
- **Worst-case raster fidelity** is slightly better with parley (0.816 against 0.800).
- Cosmic-text is slightly ahead on mean text-block SSIM (0.886 against 0.878). The gap is small next to the fixable items below.

## Mismatch categories

Each figure below comes from a diagnostic capture or per-category grouping in this run.

| Category | Evidence | Fixable? |
|---|---|---|
| **Text decoration** (`right`, `outline` variants) | Text-block SSIM 0.79 and 0.78, against 0.93 for `original` and `left`. Removing decoration from Chromium and Rust lifts worst whole-frame SSIM from 0.816 to 0.941 and mean text-block SSIM to 0.920 on the 400-case diagnostic. The spike hard-codes offsets (`+0.08em`, `-0.3em`) and draws no underline skip-ink. | Yes. Read underline and strikeout metrics from the font and implement skip-ink. Not done in this spike. |
| **Stroke** | Removing stroke from both sides lifts mean text-block SSIM by about 0.02. The spike's shadow is cast from the fill only. Whether Chromium's shadow includes the stroke was not isolated. | Likely. Needs a stroke-inclusive shadow experiment in 052. |
| **RTL paragraph direction** | Text-block SSIM 0.65. Line text matches, but parley auto-detects a right-to-left base, so the Arabic line's closing period sits at the opposite end from Chromium's LTR base. Neither `overlay-template.ts` nor `overlay.html` sets a direction. | Probably. No base-direction option was found in parley 0.10's `style/` or `builder.rs`, so 052 needs a further look or an upstream change. Arabic is not known to be used by a shipped translation; it is corpus coverage. |
| **The one line-break miss** (`midnight-long-right-fit`) | Fitted to 13 px with -0.4 px letter spacing. Chromium fits the fourth line at 1362.875 px in a 1363.195 px box, 0.32 px to spare. Both Rust stacks break one word earlier. Accumulated sub-pixel advance difference is the probable cause; the exact source inside Chromium's measurement was not traced. | Not without matching Chromium's advance rounding. It occurs only at an extreme fit of a very long passage. |
| **Glyph anti-aliasing** | Remains after the above: text-block SSIM tops out around 0.92 to 0.93. tiny-skia coverage differs from Skia's text raster. | Inherent to this rasteriser. Visually minor in the sampled images. |

**Bug fixed during review.** The Rust code clamped every overflowing fixed-size layer to the top padding. That is right for the verse, but Chromium centres the reference when it overflows its short box (108 px tall with 48 px padding), so references were drawn 14 px too low. The fix raised mean text-block SSIM from 0.809 to 0.878.

## Raster time

| Phase, parley, 400 text cases | p50 | p95 | Max |
|---|---:|---:|---:|
| Shadow | 2.58 ms | 15.73 ms | 25.2 ms |
| Stroke | 0 ms | 9.80 ms | 61.3 ms |
| Fill + composite | 2.68 ms | 9.74 ms | 26.6 ms |
| Whole raster | 6.03 ms | 28.84 ms | 113.3 ms |

64 of 400 cases exceed 16 ms, mostly long passages with shadow or stroke; the 113 ms worst case is `lyrics-stage-long-outline-fit`. Shaping is not the problem (layout p95 2.06 ms). Options for 008 and 052: blur the shadow on the GPU, cache a slide's raster so it is only produced on change, or accept the cost if text is rasterised off the frame path. None was prototyped here.

## Determinism

Two consecutive captures with the final tool:

- `layout.json` is byte-identical, and so are all five golden PNGs.
- Of 480 full-frame working PNGs, 3 differed. In the one pair quantified, 117 channels differed, each by exactly 1 level.

The committed goldens are deterministic. The working PNGs are not strictly so, which is too small to affect SSIM at the reported precision. Prompts that compare PNGs must allow a ±1 tolerance. The cause, likely Chromium's raster timing on long shadowed text, was not established.

## Corpus and tooling

- `tools/golden-capture` launches an Electron script that imports `overlay-template.ts` and `theme-library.ts` through an in-memory esbuild bundle, then renders through the real `overlay.html`. It refuses an output directory inside the Electron checkout and writes nothing there.
- 480 cases are 5 built-in themes × 12 passages × 4 style variants × auto-fit on and off. `DEFAULT_OVERLAY_THEME` is covered because the `broadcast` theme is that object.
- Passages: short and very long text, all three bundled translations (KJV, BBE, BSB), numbered verses, Yoruba tone marks with coloured gloss lines, lyrics, RTL, an unbreakable long word with HTML-sensitive characters, empty text, and blank verse with a reference.
- Variants: all alignments, letter spacing, every text transform, underline, line-through and outline.
- Run the spike's comparison with its own manifest, since it is a separate workspace.

### Failure and empty paths from the source

| Source path | Handling |
|---|---|
| Blank `text` omits the verse box; blank reference or `show: false` omits the reference | Capture asserts the expected layer count; corpus includes both cases |
| `largestFontThatFits` returns the minimum when even it overflows | Rust keeps the same floor of 12 px |
| Fit overflow tolerance of +1 px on both axes | Same check in `fit` |
| Fixed verse overflow and clipped box | Top-anchored overflow reproduced; reference centring fixed above |
| HTML-sensitive text escaped by `escapeHtml` | `<light> & “mercy”` in the corpus |
| Outline disabled or width 0 yields no stroke | Computed style gives width 0, so no stroke pass runs |
| Shadow disabled yields no shadow | Guarded in `draw` |
| Empty paragraphs and collapsed whitespace | Blank paragraph advances one line height; whitespace collapsed before shaping |
| Video and media paths in the template | Out of scope for text |

## Not verified

- Windows and Linux text stacks and fonts.
- Timing on an idle machine.
- Real backgrounds: Rust comparison uses the flat background only, while five themed PNGs are retained as references.
- Custom user-uploaded fonts.
- Skip-ink, metric-based decoration and stroke-inclusive shadow, which are recommended but not implemented.
- Whether the RTL base direction can be forced in parley.
- The exact cause of the 0.32 px miss and of the ±1 PNG noise.
