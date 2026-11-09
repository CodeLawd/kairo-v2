#!/usr/bin/env python3
"""Time the packaged scripture database's warm FTS query without opening the app."""

import argparse
import sqlite3
from pathlib import Path
from statistics import median
from time import perf_counter_ns

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("database", type=Path)
args = parser.parse_args()
query = """
SELECT b.name, b.abbreviation, f.translation_id, CAST(f.book_id AS INTEGER),
       CAST(f.chapter AS INTEGER), CAST(f.verse AS INTEGER), f.text
FROM verses_fts f JOIN books b ON b.id = CAST(f.book_id AS INTEGER)
WHERE verses_fts MATCH ? AND (? IS NULL OR f.translation_id = ?)
ORDER BY rank
LIMIT 50
"""
with sqlite3.connect(args.database.resolve(strict=True).as_uri() + "?mode=ro&immutable=1",
                     uri=True) as database:
    for terms in ("lord shepherd", "love patient", "beginning god created"):
        params = (terms, "KJV", "KJV")
        rows = database.execute(query, params).fetchall()
        if not rows:
            raise RuntimeError(f"No KJV results for {terms!r}")
        durations = []
        for _ in range(30):
            started = perf_counter_ns()
            database.execute(query, params).fetchall()
            durations.append((perf_counter_ns() - started) / 1_000_000)
        print(f"{terms}: {len(rows)} rows, median {median(durations):.3f} ms, "
              f"range {min(durations):.3f}–{max(durations):.3f} ms")
