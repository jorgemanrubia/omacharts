#!/usr/bin/env python3
"""Refuse a listings file that is not fit to publish.

    tools/check_listings.py NEW [PREVIOUS]

Every running copy of the app downloads what the listings workflow publishes,
so a bad file is a bad search for everybody until the next good one. This is
the gate in front of that, and the app holds a download to the same rules
again before it uses one (`src/listings.rs`).

A file passes when:

- its first line is the column header, and it says when it was built;
- every row has the ten columns, a kind the app knows and a ticker's shape;
- it has as many rows as a whole market has, not a truncated download's;
- each kind is mostly ranked, so a ranking feed that was down does not
  publish a day of worse ordering over a good one;
- given the file it replaces, it is within a tenth of its size: listings and
  delistings are a handful a day, and a change of thousands is a feed that
  half-answered.

With the checks passing it prints the sidecar the app reads first, as JSON:
the build date, the row count and the sha256 of the file. Exit 1 and a line
on stderr otherwise.
"""

import hashlib
import json
import re
import sys

HEADER = (
    "# kind\tsymbol\tname\tsuffix\tcurrency\ttier\tsession_origin"
    "\tyahoo_override\texchange\tpopularity"
)
KINDS = ("equity", "etf")
# A whole US market is over eleven thousand listings once the noise is out.
MIN_ROWS = 9000
# Today 90% of stocks and 80% of funds carry a band. A feed that is down
# takes its half to none.
MIN_RANKED = 0.6
MAX_CHANGE = 0.10
TICKER = re.compile(r"[A-Z][A-Z0-9.\-]{0,5}")
BUILT = re.compile(r"# built (\d{4}-\d{2}-\d{2})$")


def rows(text: str) -> list[list[str]]:
    return [line.split("\t") for line in text.splitlines() if line and not line.startswith("#")]


def check(text: str, previous: str | None) -> dict:
    lines = text.splitlines()
    if not lines or lines[0] != HEADER:
        raise ValueError("the first line is not the column header")
    built = next((m.group(1) for m in map(BUILT.match, lines[:10]) if m), None)
    if built is None:
        raise ValueError("no '# built YYYY-MM-DD' line in the header")

    listed = rows(text)
    ranked = {kind: 0 for kind in KINDS}
    counted = {kind: 0 for kind in KINDS}
    for number, fields in enumerate(listed, 1):
        if len(fields) != 10:
            raise ValueError(f"row {number} has {len(fields)} columns: {fields!r}")
        kind, symbol, popularity = fields[0], fields[1], fields[9]
        if kind not in KINDS:
            raise ValueError(f"row {number} has kind {kind!r}")
        if not TICKER.fullmatch(symbol):
            raise ValueError(f"row {number} has ticker {symbol!r}")
        counted[kind] += 1
        if popularity != "-":
            if not popularity.isdigit() or not 1 <= int(popularity) <= 9:
                raise ValueError(f"row {number} has popularity {popularity!r}")
            ranked[kind] += 1

    if len(listed) < MIN_ROWS:
        raise ValueError(f"only {len(listed)} rows; a whole market is over {MIN_ROWS}")
    for kind in KINDS:
        share = ranked[kind] / counted[kind] if counted[kind] else 0.0
        if share < MIN_RANKED:
            raise ValueError(f"only {share:.0%} of {kind} rows are ranked; a ranking feed was down")
    if previous is not None:
        before = len(rows(previous))
        if before and abs(len(listed) - before) > before * MAX_CHANGE:
            raise ValueError(f"{len(listed)} rows against {before} before; more change than a day has")

    return {
        "built": built,
        "rows": len(listed),
        "sha256": hashlib.sha256(text.encode("utf-8")).hexdigest(),
    }


def main(argv: list[str]) -> int:
    if len(argv) not in (2, 3):
        print(__doc__.strip().splitlines()[2].strip(), file=sys.stderr)
        return 2
    with open(argv[1], encoding="utf-8") as new:
        text = new.read()
    previous = None
    if len(argv) == 3:
        try:
            with open(argv[2], encoding="utf-8") as old:
                previous = old.read()
        except FileNotFoundError:
            previous = None
    try:
        sidecar = check(text, previous)
    except ValueError as err:
        print(f"refused: {err}", file=sys.stderr)
        return 1
    print(json.dumps(sidecar))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
