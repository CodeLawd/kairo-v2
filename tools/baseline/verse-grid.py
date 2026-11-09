#!/usr/bin/env python3
"""Adapt the legacy verse-grid harness to current props in a disposable snapshot."""

import argparse
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("checkout", type=Path, help="Writable Electron snapshot")
args = parser.parse_args()
checkout = args.checkout.resolve()
original = checkout / "tests/scripture-grid.bench.cjs"
source = original.read_text()

props = "        onSelectCard: () => {},"
count = "      const cards = host.querySelectorAll('button').length;"
if source.count(props) != 1 or source.count(count) != 1:
    raise SystemExit("The benchmark changed. Review the fixture adapter against its current source.")
source = source.replace(props, props + """
        pickedCards: new Set(), onPickCard: () => {},
        onMarqueeBegin: () => {}, onMarqueeChange: () => {},""")
source = source.replace(count, count + """
      if (cards === 0) throw new Error('VerseCardGrid mounted zero cards');
      if (rows === realRows && !window.baselineScrolled) {
        window.baselineScrolled = true;
        const deltas = [];
        const origin = await new Promise(requestAnimationFrame);
        const scroller = document.scrollingElement;
        const distance = Math.min(2000, scroller.scrollHeight - scroller.clientHeight);
        if (distance <= 0) throw new Error('Verse grid is not scrollable');
        let previous = origin;
        while (previous - origin < 2000) {
          const frame = await new Promise(requestAnimationFrame);
          deltas.push(frame - previous);
          scroller.scrollTop = distance * Math.min(1, (frame - origin) / 2000);
          previous = frame;
        }
        scroller.scrollTop = 0;
        deltas.sort((a, b) => a - b);
        out.push({ label: 'synthetic scroll frame cadence', cards, distance,
          samples: deltas.length, median: deltas[Math.floor(deltas.length / 2)],
          p95: deltas[Math.ceil(deltas.length * 0.95) - 1],
          over17ms: deltas.filter(delta => delta > 17).length });
      }""")
source = source.replace("  for (const r of result) {", "  for (const r of result) {\n    if (r.samples) continue")
generated = original.with_name(".baseline-verse-grid.cjs")
with generated.open("x") as output:
    output.write(source)
try:
    subprocess.run(
        ["./node_modules/.bin/electron", str(generated.relative_to(checkout))],
        cwd=checkout, check=True,
    )
finally:
    generated.unlink()
