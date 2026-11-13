"""Builds static Source Sans 3 weights for the operator interface.

The bundled variable subsets (crates/kairo-scene/fonts, from
@fontsource-variable/source-sans-3 5.3.0) register on macOS under their
default instance's family, "Source Sans 3 ExtraLight", so a request for
"Source Sans 3" never matched them. This writes one static file per weight
the interface uses, with the five script subsets merged so Yoruba letters
come from the same face, named family "Source Sans 3".

Run: python tools/instance-fonts.py  (needs fontTools 4.55)
"""
from pathlib import Path
import tempfile
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont
from fontTools.merge import Merger

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT.parent / "kairo-scene" / "fonts"
OUT = ROOT / "assets" / "fonts"
SUBSETS = ["latin", "latin-ext", "vietnamese", "greek", "cyrillic"]
WEIGHTS = {400: "Regular", 500: "Medium", 600: "Semibold", 700: "Bold"}
FAMILY = "Source Sans 3"


def name(font, style, weight):
    table = font["name"]
    full = f"{FAMILY} {style}" if style != "Regular" else FAMILY
    ps = f"SourceSans3-{style}"
    legacy_family = FAMILY if style in ("Regular", "Bold") else f"{FAMILY} {style}"
    legacy_style = style if style in ("Regular", "Bold") else "Regular"
    for record in list(table.names):
        if record.nameID in (1, 2, 3, 4, 6, 16, 17, 25):
            table.removeNames(nameID=record.nameID)
    table.setName(legacy_family, 1, 3, 1, 0x409)
    table.setName(legacy_style, 2, 3, 1, 0x409)
    table.setName(f"{ps};kairo", 3, 3, 1, 0x409)
    table.setName(full, 4, 3, 1, 0x409)
    table.setName(ps, 6, 3, 1, 0x409)
    table.setName(FAMILY, 16, 3, 1, 0x409)
    table.setName(style, 17, 3, 1, 0x409)
    font["OS/2"].usWeightClass = weight
    bold = weight >= 700
    font["OS/2"].fsSelection = (font["OS/2"].fsSelection & ~0b1100001) | (0b100000 if bold else 0b1000000)
    font["head"].macStyle = 1 if bold else 0


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        for weight, style in WEIGHTS.items():
            parts = []
            for subset in SUBSETS:
                font = TTFont(SOURCE / f"SourceSans3-{subset}.ttf")
                static = instantiateVariableFont(font, {"wght": weight})
                path = Path(tmp) / f"{subset}-{weight}.ttf"
                static.save(path)
                parts.append(str(path))
            merged = Merger().merge(parts)
            name(merged, style, weight)
            merged.save(OUT / f"SourceSans3-{style}.ttf")
            print(style, (OUT / f"SourceSans3-{style}.ttf").stat().st_size)


if __name__ == "__main__":
    main()
