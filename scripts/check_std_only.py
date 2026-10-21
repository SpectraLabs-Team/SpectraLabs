#!/usr/bin/env python3
"""Fails if an engine crate's default build depends on non-workspace crates.

Optional integrations may add dependencies behind explicit features (for example the Postgres
adapter in spectra-registry). Parity and engine code build without enabling those features.

    cargo metadata --format-version 1 --no-deps | python3 scripts/check_std_only.py
"""
import json
import sys

STD_ONLY = {
    "spectra-core", "spectra-data", "spectra-engine", "spectra-research",
    "spectra-registry", "spectra-risk", "spectra-cli",
}

packages = json.load(sys.stdin)["packages"]
bad = [
    f"{p['name']} -> {d['name']}"
    for p in packages if p["name"] in STD_ONLY
    for d in p["dependencies"] if d.get("path") is None and not d.get("optional", False)
]
missing = STD_ONLY - {p["name"] for p in packages}
if missing:
    sys.exit(f"unknown crates in STD_ONLY (renamed?): {sorted(missing)}")
if bad:
    sys.exit("third-party dependencies in standard-library-only crates: " + ", ".join(bad))
print("engine crates are standard-library only")
