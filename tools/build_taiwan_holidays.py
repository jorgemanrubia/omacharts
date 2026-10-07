#!/usr/bin/env python3
"""Merge this year's Taiwan market holidays into the engine's calendar.

The Taiwan Stock Exchange publishes the days its market does not trade — the
public holidays, the days made up for them, and the days around Lunar New Year
that open for settlement only — as its Holiday Schedule. The Taipei Exchange
keeps the same calendar. Every date on it is a weekday or weekend on which
nothing trades, so the engine treats each one as closed.

The feed only ever answers for the current year, whatever year is asked for,
so this merges rather than overwrites: the years already in the file stay, and
this year's dates replace whatever the file said about this year. Run it once
the exchange publishes the next year's schedule, usually in December.

Run: tools/build_taiwan_holidays.py
"""

import json
import pathlib
import sys

# The same User-Agent and timeout as the other tools.
from build_listings import fetch

FEED = "https://www.twse.com.tw/rwd/en/holidaySchedule/holidaySchedule?response=json"
CALENDAR = pathlib.Path(__file__).resolve().parent.parent / (
    "crates/omacharts-engine/src/holidays_tw.txt"
)
HEADER = """\
# Days the Taiwan Stock Exchange and the Taipei Exchange do not trade.
# From the TWSE Holiday Schedule; regenerate with tools/build_taiwan_holidays.py.
# One ISO date per line, then what the exchange calls it.
"""


def schedule():
    body = json.loads(fetch(FEED))
    if body.get("stat") != "ok" or not body.get("data"):
        sys.exit(f"the exchange did not answer with a schedule: {body.get('stat')!r}")
    return body["queryYear"], {date: name.strip() for date, name in body["data"]}


def existing():
    if not CALENDAR.exists():
        return {}
    rows = {}
    for line in CALENDAR.read_text().splitlines():
        if line.strip() and not line.startswith("#"):
            date, _, name = line.partition(" ")
            rows[date] = name.strip()
    return rows


def main():
    year, fresh = schedule()
    rows = {d: n for d, n in existing().items() if not d.startswith(f"{year}-")}
    rows.update(fresh)
    body = "".join(f"{date} {rows[date]}\n" for date in sorted(rows))
    CALENDAR.write_text(HEADER + body)
    print(f"{len(fresh)} days for {year}; {len(rows)} in the calendar", file=sys.stderr)


if __name__ == "__main__":
    main()
