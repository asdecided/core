#!/usr/bin/env python3
"""Generate rust/spec/pycompat-tables.json from the oracle Python runtime.

Unicode behavior tables the Rust engine loads at build time so string
semantics match CPython 3.11 by construction (roadmap:native-engine-spike):
casefold mapping, str whitespace set, str.splitlines line boundaries,
str.isprintable ranges, and re module \\d / \\w codepoint ranges.
Regenerate with the oracle venv; never hand-edit the output.
"""
from __future__ import annotations

import json
import re
import sys
import unicodedata
from pathlib import Path

MAX = 0x110000

def ranges(pred):
    out, start = [], None
    for cp in range(MAX):
        if pred(cp):
            if start is None:
                start = cp
        elif start is not None:
            out.append([start, cp - 1]); start = None
    if start is not None:
        out.append([start, MAX - 1])
    return out

d_re = re.compile(r"\d")
w_re = re.compile(r"\w")

casefold = {}
for cp in range(MAX):
    c = chr(cp)
    f = c.casefold()
    if f != c:
        casefold[cp] = f

payload = {
    "generated_by": "rust/spec/extract_pycompat_tables.py",
    "python": sys.version,
    "unicodedata": unicodedata.unidata_version,
    "casefold": {str(k): v for k, v in casefold.items()},
    "str_whitespace": ranges(lambda cp: chr(cp).isspace()),
    "splitlines_boundaries": sorted(
        cp for cp in range(0x3000) if len((chr(cp)).join(["a", "b"]).splitlines()) > 1
    ),
    "isprintable": ranges(lambda cp: chr(cp).isprintable()),
    "re_digit": ranges(lambda cp: bool(d_re.match(chr(cp)))),
    "re_word": ranges(lambda cp: bool(w_re.match(chr(cp)))),
}
target = (
    Path(__file__).resolve().parents[1]
    / "rac-engine"
    / "assets"
    / "spec"
    / "pycompat-tables.json"
)
target.write_text(json.dumps(payload, ensure_ascii=True, sort_keys=True) + "\n")
print(f"wrote {target}: {len(casefold)} casefold entries, "
      f"{len(payload['str_whitespace'])} ws ranges, "
      f"{len(payload['isprintable'])} printable ranges, "
      f"{len(payload['re_word'])} word ranges, "
      f"boundaries={payload['splitlines_boundaries']}")
