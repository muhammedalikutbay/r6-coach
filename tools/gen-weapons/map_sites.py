#!/usr/bin/env python3
"""Builds crates/r6k-core/data/map-sites.json: bomb site names per known map.

Map ids change per season and playlist, site names do not, so an unknown map id can be identified
by its round `site` values (e.g. "2F Master Bedroom, 2F Office"). Only maps whose id is in
data/maps.json contribute; the file is merged with the existing one, never shrunk.

Usage:
    py -3 tools/gen-weapons/map_sites.py <folder with r6-dissect match JSON files> [out.json]

Produce the JSON files with the sidecar: `r6-dissect.exe <match folder> -o <name>.json` per match.
The replays hold other players' names and never go into git; the output holds map and room names only.
"""
import glob
import json
import os
import sys

ROOT = os.path.normpath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT = os.path.join(ROOT, "crates", "r6k-core", "data", "map-sites.json")


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    out = sys.argv[2] if len(sys.argv) > 2 else OUT
    with open(os.path.join(ROOT, "data", "maps.json"), encoding="utf-8") as f:
        key_of = {m["id"]: m["key"] for m in json.load(f)["maps"]}
    sites: dict[str, set[str]] = {}
    if os.path.exists(out):
        with open(out, encoding="utf-8") as f:
            for k, v in json.load(f)["maps"].items():
                sites[k] = set(v)
    seen = unknown = 0
    for path in glob.glob(os.path.join(sys.argv[1], "*.json")):
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
        for rnd in data.get("rounds", []):
            key = key_of.get(rnd.get("map", {}).get("id"))
            site = rnd.get("site", "")
            if not site:
                continue
            if key is None:
                unknown += 1
                continue
            seen += 1
            sites.setdefault(key, set()).add(site)
    doc = {
        "comment": "Bomb site names per map key, from real Y11S3 replays (tools/gen-weapons/map_sites.py).",
        "maps": {k: sorted(v) for k, v in sorted(sites.items())},
    }
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        json.dump(doc, f, indent=2, ensure_ascii=False)
        f.write("\n")
    total = sum(len(v) for v in doc["maps"].values())
    print(f"{seen} rounds on known maps, {unknown} on unknown maps; {total} sites for {len(doc['maps'])} maps -> {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
