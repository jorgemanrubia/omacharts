#!/usr/bin/env python3
"""Build the generated half of the symbol inventory.

The curated half lives in crates/omacharts-engine/src/seed.tsv and is written
by hand: it carries the tier weights that make GC mean gold futures, the
session origins that make a 4H bar bucket correctly, and every futures root,
currency pair and crypto, none of which appear in any listings feed.

This writes the other half — every US-listed equity and ETF — from Nasdaq
Trader's daily files. They give a ticker and a name and nothing else, so
everything here lands at tier 2, below anything curated, and the merge at load
time lets a curated row win wherever both have a symbol.

Two more feeds say how well known each listing is, because a ticker and a name
cannot: eleven thousand rows that all look equally important make search spell
out microcaps ahead of household names. Equities are ranked by market cap and
ETFs by the money that actually changes hands in them, each onto the same 1-9
popularity band. Neither feed is load-bearing — when one is down the column
goes to `-` and search falls back to exactly the ordering it had before.

Run: tools/build_listings.py > crates/omacharts-engine/src/listings.tsv

That is for a local look. The file the app uses is built every weekday by
.github/workflows/listings.yml, checked by tools/check_listings.py, and
published on the `listings` branch, which every running copy of the app
fetches once a day; bin/release copies it in before a tag.
"""

import csv
import datetime
import io
import json
import re
import sys
import urllib.error
import urllib.request

NASDAQ = "https://www.nasdaqtrader.com/dynamic/SymDir/nasdaqlisted.txt"
OTHER = "https://www.nasdaqtrader.com/dynamic/SymDir/otherlisted.txt"

# Market cap for every US-listed operating company. Covers 99% of the equities
# in the listing files and no ETF at all, which is why there are two feeds.
SCREENER = "https://api.nasdaq.com/api/screener/stocks?tableonly=true&download=true"

# One venue's share volume per symbol, which is the only per-symbol volume
# anyone publishes in bulk. A single venue is a few percent of consolidated
# tape, but its share barely varies by symbol, so as a *ranking* it stands in
# for the whole market perfectly well — and ranking is all this is used for.
CBOE = "https://www.cboe.com/us/equities/market_statistics/symbol_data/csv/?mkt=bzx"

# Both feeds sit behind a CDN that turns away anything that does not look like
# a browser. Nasdaq Trader does not care, but one header for all three is less
# to explain than two.
AGENT = "Mozilla/5.0 (X11; Linux x86_64) omacharts-listings"

# otherlisted.txt names the venue with a single letter. A code not in here is
# written as unknown rather than printed raw: a column saying "F" tells nobody
# anything, and guessing which venue it is would be worse.
VENUES = {
    "N": "NYSE",
    "P": "NYSE Arca",
    "Z": "Cboe BZX",
    "A": "NYSE American",
    "V": "IEX",
}

# Instruments whose name says they are not something anyone charts: the paper
# around a listing rather than the listing. Dropping them is 14% of the file.
NOISE = re.compile(
    r"\b(warrant|warrants|right|rights|unit|units|preferred|depositary|"
    r"debenture|notes due|subordinated)\b|%\s*series",
    re.IGNORECASE,
)

# What share of a population each popularity band holds, most popular first.
# Not deciles: an even split would spend five bands telling apart microcaps
# nobody can tell apart, and lump Apple in with the five hundredth largest
# company. Fame is distributed like this, so the bands are too — band 9 is a
# few dozen names, band 1 is half the market.
BANDS = (0.005, 0.015, 0.04, 0.10, 0.20, 0.35, 0.55, 0.75)


def fetch(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": AGENT})
    with urllib.request.urlopen(request, timeout=90) as response:
        return response.read()


def lines(url: str) -> list[str]:
    return fetch(url).decode("utf-8", "replace").splitlines()


def rows(
    lines: list[str],
    symbol_at: int,
    name_at: int,
    etf_at: int,
    test_at: int,
    venue_at: int | None,
):
    for line in lines[1:]:
        fields = line.split("|")
        # The last line of both files is a timestamp, not a listing.
        if len(fields) <= max(symbol_at, name_at, etf_at, test_at):
            continue
        symbol, name = fields[symbol_at].strip(), fields[name_at].strip()
        if not symbol or not name or fields[test_at].strip() == "Y":
            continue
        # Over five characters is a test issue or a non-common class in
        # Nasdaq's fifth-letter scheme, neither of which is a chart.
        if len(symbol) > 5 or NOISE.search(name):
            continue
        if not re.fullmatch(r"[A-Z][A-Z0-9.\-]*", symbol):
            continue
        kind = "etf" if fields[etf_at].strip() == "Y" else "equity"
        code = fields[venue_at].strip() if venue_at is not None else ""
        venue = VENUES.get(code, "-") if code else "NASDAQ"
        # One trailing qualifier is enough; the rest is boilerplate nobody
        # reads in a picker a few characters wide.
        name = re.split(r"\s+-\s+", name)[0].strip()
        name = re.sub(r"\s+(Common Stock|Ordinary Shares?|Class [A-Z])$", "", name).strip()
        yield kind, symbol, name or symbol, venue


def banded(weights: dict[str, float]) -> dict[str, int]:
    """Rank symbols by weight and bucket them into bands 9 (best) down to 1.

    Band is a *rank* over whatever the caller measured, so the two feeds can
    use different units and still land on one comparable scale — and so a
    quiet week in the market does not shuffle the whole column.
    """
    ranked = sorted(weights, key=lambda s: (-weights[s], s))
    cuts = [int(share * len(ranked)) for share in BANDS]
    out = {}
    for position, symbol in enumerate(ranked):
        band = 9
        for cut in cuts:
            if position >= cut:
                band -= 1
        out[symbol] = band
    return out


def market_caps() -> dict[str, float]:
    """Market cap per equity, from Nasdaq's screener."""
    payload = json.loads(fetch(SCREENER))
    caps = {}
    for row in payload["data"]["rows"]:
        try:
            cap = float(row["marketCap"])
        except (KeyError, TypeError, ValueError):
            continue
        if cap > 0:
            # The screener spells share classes BRK/B where every other feed,
            # this one's listing files included, writes BRK.B.
            caps[row["symbol"].strip().replace("/", ".")] = cap
    if len(caps) < 3000:
        raise ValueError(f"screener returned only {len(caps)} market caps")
    return caps


def dollar_volumes() -> dict[str, float]:
    """Money traded per symbol today, from one Cboe venue."""
    text = fetch(CBOE).decode("utf-8", "replace")
    volumes = {}
    for row in csv.DictReader(io.StringIO(text)):
        try:
            traded = int(row["Volume"]) * float(row["Last Price"])
        except (KeyError, TypeError, ValueError):
            continue
        if traded > 0:
            volumes[row["Name"].strip()] = traded
    if len(volumes) < 3000:
        raise ValueError(f"cboe returned only {len(volumes)} volumes")
    return volumes


def popularity(kinds: dict[str, str]) -> dict[str, int]:
    """Band per symbol, from whichever feeds answered.

    Each feed ranks only the half of the inventory it suits — market cap says
    nothing about a fund, and a fund's volume is the only size it publishes —
    and each is banded over that half alone, so a band always means the same
    thing: how this stands among things of its own sort. Cboe prices equities
    too, but a share count and a company's worth are not one scale and must
    not be sorted into one list.

    A feed that is down costs its half its ranking and nothing else: the
    column goes to `-`, which scores exactly zero, and search is no worse than
    it was before any of this existed. Half a file is never worth failing a
    nightly job over.
    """
    bands = {}
    for what, kind, source in (
        ("market caps", "equity", market_caps),
        ("volumes", "etf", dollar_volumes),
    ):
        try:
            measured = source()
        except (urllib.error.URLError, OSError, ValueError, KeyError, json.JSONDecodeError) as err:
            print(f"# warning: no {what} ({err}); {kind} rows go unranked", file=sys.stderr)
            continue
        mine = {s: w for s, w in measured.items() if kinds.get(s) == kind}
        bands.update(banded(mine))
    return bands


def main() -> int:
    seen: dict[str, tuple[str, str, str, str]] = {}
    for url, sym, name, etf, test, venue in (
        (NASDAQ, 0, 1, 6, 3, None),
        (OTHER, 0, 1, 4, 6, 2),
    ):
        for row in rows(lines(url), sym, name, etf, test, venue):
            seen.setdefault(row[1], row)

    bands = popularity({symbol: row[0] for symbol, row in seen.items()})

    print(
        "# kind\tsymbol\tname\tsuffix\tcurrency\ttier\tsession_origin"
        "\tyahoo_override\texchange\tpopularity"
    )
    print("# Generated by tools/build_listings.py from Nasdaq Trader's daily")
    print("# files. Do not edit: the curated inventory is seed.tsv, and a symbol")
    print("# in both takes the curated row. Everything here is tier 2.")
    # The app keeps whichever copy was built last, the one compiled in or
    # one downloaded since, and this line is how it tells them apart.
    print(f"# built {datetime.datetime.now(datetime.timezone.utc).date().isoformat()}")
    ranked = 0
    for kind, symbol, label, venue in sorted(seen.values(), key=lambda r: r[1]):
        band = bands.get(symbol)
        if band is not None:
            ranked += 1
        print(f"{kind}\t{symbol}\t{label}\t-\tUSD\t2\t0\t-\t{venue}\t{band if band else '-'}")
    print(f"# {len(seen)} listings, {ranked} ranked", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
