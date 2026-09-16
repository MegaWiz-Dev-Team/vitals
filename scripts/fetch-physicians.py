#!/usr/bin/env python3
"""Fetch the one open series the globe colours countries by, and put it where the page reads it.

World Bank WDI indicator SH.MED.PHYS.ZS — physicians per 1,000 people — which is WHO's Global Health
Workforce Statistics republished under CC BY 4.0. Written two places from one download, so they cannot
drift: `crates/vitals-web/data/physicians.json` (the committed data file, for anyone who wants the
numbers without the page) and the `<script id="physicians" type="application/json">` block inside
`crates/vitals-web/static/world/index.html` (what the page actually reads — inlined the way the atlas
is, so the page stays one file that `include_str!` ships whole). A test holds the two equal.

Re-runnable:  python3 scripts/fetch-physicians.py            (fetches, writes both)
              python3 scripts/fetch-physicians.py --check    (fetches nothing; re-inlines the data file)

Shape of the file (developer-16's reference of 16 Sep 2026, reproduced exactly): top-level "indicator",
"source", "licence", "fetched", "note", and "countries" — one key per World Bank code (ISO 3166-1 alpha-3
for countries; aggregates such as `WLD`, the regions and the income groups keep their World Bank codes,
and `WLD` is what the mission line reads) — each `{"name": …, "series": [[year, per_1000], …]}`
ascending by year, values only, three decimals.
"""
import json, sys, urllib.request, datetime, pathlib, re

ROOT = pathlib.Path(__file__).resolve().parent.parent
DATA = ROOT / "crates/vitals-web/data/physicians.json"
PAGE = ROOT / "crates/vitals-web/static/world/index.html"
INDICATOR = "SH.MED.PHYS.ZS"
URL = f"https://api.worldbank.org/v2/country/all/indicator/{INDICATOR}?format=json&per_page=20000&date=2000:2024"
MARK_OPEN = '<script id="physicians" type="application/json">'
MARK_CLOSE = "</script>"


def fetch():
    with urllib.request.urlopen(URL, timeout=120) as r:
        meta, rows = json.load(r)
    if meta.get("pages", 1) != 1:
        sys.exit(f"the API paged ({meta['pages']} pages) — raise per_page")
    out = {
        "indicator": f"{INDICATOR} — {rows[0]['indicator']['value'] if rows else 'Physicians (per 1,000 people)'}",
        "source": "World Bank, World Development Indicators (data from WHO Global Health Workforce Statistics, OECD, "
                  f"national sources), https://data.worldbank.org/indicator/{INDICATOR}",
        "licence": "CC BY 4.0",
        "fetched": datetime.date.today().isoformat(),
        "note": "people per doctor = 1000 / physicians_per_1000. Aggregates (WLD, regions, income groups) keep their "
                "World Bank codes; countries are ISO 3166-1 alpha-3.",
        "countries": {},
    }
    series = {}
    names = {}
    for r in rows:
        code = (r.get("countryiso3code") or "").strip()
        if not re.fullmatch(r"[A-Z]{3}", code):
            continue
        names.setdefault(code, r["country"]["value"])
        v = r.get("value")
        if v is None:
            continue
        series.setdefault(code, []).append([int(r["date"]), round(float(v), 3)])
    for code in sorted(series):
        out["countries"][code] = {"name": names[code], "series": sorted(series[code])}
    return out


def inline(data: dict):
    html = PAGE.read_text(encoding="utf-8")
    block = MARK_OPEN + json.dumps(data, ensure_ascii=False, separators=(",", ":")) + MARK_CLOSE
    if MARK_OPEN in html:
        start = html.index(MARK_OPEN)
        end = html.index(MARK_CLOSE, start) + len(MARK_CLOSE)
        html = html[:start] + block + html[end:]
    else:
        anchor = '<script id="globe">'
        html = html.replace(anchor, block + "\n" + anchor, 1)
    PAGE.write_text(html, encoding="utf-8")


def main():
    if "--check" in sys.argv:
        data = json.loads(DATA.read_text(encoding="utf-8"))
    else:
        data = fetch()
        DATA.write_text(json.dumps(data, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    inline(data)
    countries = list(data["countries"])
    wld = data["countries"].get("WLD", {}).get("series", [])
    print(f"{len(countries)} codes with values, WLD latest {wld[-1] if wld else None}, written {DATA.relative_to(ROOT)} and inlined into {PAGE.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
