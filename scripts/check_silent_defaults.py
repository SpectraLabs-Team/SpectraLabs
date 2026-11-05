#!/usr/bin/env python3
"""Fails on a new silent default in the model crates, or a literal cost in spectra-api.

A silent default turns a missing or failed value into a number (`.unwrap_or(0.0)`,
`.unwrap_or_default()`, `.map_or(1, ..)`) without saying so. Each approved site is listed in
scripts/silent_defaults_allowlist.txt, matched on its file and code text (not its line number),
with a category and a reason; see docs/models/INVENTORY.md, "Silent defaults check".

    python3 scripts/check_silent_defaults.py              check the workspace
    python3 scripts/check_silent_defaults.py --self-test  check the checker on a small fixture

Exit code 1 on a hit missing from the allowlist, or an allowlist entry that no longer matches.
"""
import re
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ALLOWLIST = ROOT / "scripts" / "silent_defaults_allowlist.txt"

MODEL_CRATES = [
    "spectra-core", "spectra-data", "spectra-engine", "spectra-research", "spectra-risk",
    "spectra-backtest",
]
COST_CRATES = ["spectra-api"]
CATEGORIES = {"defined", "parity", "unreachable", "follow-up"}

NUMBER = r"-?\d[\d_]*(?:\.[\d_]*)?(?:e-?\d+)?(?:_?[iuf](?:8|16|32|64|128|size))?"
CONSTANT = r"(?:ZERO|ONE|MAX|MIN|INFINITY|NEG_INFINITY|NAN|EPSILON)"
SILENT_DEFAULT = re.compile(
    r"\.unwrap_or\(\s*(?:" + NUMBER + r"|f(?:32|64)::\w+|(?:[A-Z]\w*|[iu](?:8|16|32|64|128|size))::" + CONSTANT + r")\s*\)"
    r"|\.unwrap_or_default\(\)"
    r"|\.map_or\(\s*" + NUMBER + r"\s*,"
    r"|\.ok\(\)\s*\.unwrap_or"
)
LITERAL_COST = re.compile(r"\b(?:Bps|Money)::from_f64\(\s*" + NUMBER + r"\s*\)")
NAMED_CONST = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const|static)\s")


def code_lines(path):
    """(line number, stripped text) of non-test code: stops at the first `#[cfg(test)]`."""
    out = []
    for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        stripped = line.strip()
        if stripped.startswith("#[cfg(test)]"):
            break
        if stripped.startswith("//"):
            continue
        out.append((n, stripped))
    return out


def rust_files(root, crates):
    for crate in crates:
        src = root / "crates" / crate
        if not src.is_dir():
            sys.exit(f"unknown crate {crate} (renamed?)")
        for f in sorted(src.rglob("*.rs")):
            rel = f.relative_to(src).parts
            if "tests" in rel or "benches" in rel or "examples" in rel or "target" in rel:
                continue
            yield f


def find_hits(root):
    """Every hit as (file relative to root, line number, code text)."""
    hits = []
    for f in rust_files(root, MODEL_CRATES):
        for n, text in code_lines(f):
            if SILENT_DEFAULT.search(text):
                hits.append((f.relative_to(root).as_posix(), n, text))
    for f in rust_files(root, COST_CRATES):
        for n, text in code_lines(f):
            if LITERAL_COST.search(text) and not NAMED_CONST.match(text):
                hits.append((f.relative_to(root).as_posix(), n, text))
    return hits


def load_allowlist(path):
    """Entries `file | category | code text | reason`, one per line; `#` starts a comment.

    The code text may contain ` | ` (closures); the reason may not.
    """
    entries, errors = [], []
    for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        head = line.split(" | ", 2)
        parts = head[:2] + head[2].rsplit(" | ", 1) if len(head) == 3 else head
        parts = [p.strip() for p in parts]
        if len(parts) != 4 or not all(parts):
            errors.append(f"{path.name}:{n}: expected 'file | category | code | reason'")
            continue
        file, category, code, reason = parts
        if category not in CATEGORIES:
            errors.append(f"{path.name}:{n}: unknown category {category!r} (use {', '.join(sorted(CATEGORIES))})")
            continue
        entries.append({"line": n, "file": file, "category": category, "code": code, "reason": reason})
    return entries, errors


def check(root, allowlist):
    """Problems found, and the per-category count of allowed hits."""
    hits = find_hits(root)
    entries, problems = load_allowlist(allowlist)
    used = [0] * len(entries)
    counts = dict.fromkeys(sorted(CATEGORIES), 0)
    for file, n, text in hits:
        match = next((i for i, e in enumerate(entries) if e["file"] == file and e["code"] in text), None)
        if match is None:
            problems.append(f"{file}:{n}: silent default not in the allowlist: {text}")
        else:
            used[match] += 1
            counts[entries[match]["category"]] += 1
    for e, u in zip(entries, used):
        if u == 0:
            problems.append(f"{allowlist.name}:{e['line']}: stale entry, no longer matches: {e['file']} | {e['code']}")
    return problems, counts, len(hits)


def self_test():
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        for crate in MODEL_CRATES + COST_CRATES:
            (root / "crates" / crate / "src").mkdir(parents=True)
        lib = root / "crates" / "spectra-core" / "src" / "lib.rs"
        lib.write_text(
            "fn a(m: &Map) -> f64 { m.get(&1).copied().unwrap_or(0.0) }\n"
            "fn b(x: Option<u32>) -> u32 { x.unwrap_or_default() }\n"
            "fn g(v: &[u8]) -> i64 { v.first().map_or(0, |b| i64::from(*b)) }\n"
            "// a comment: .unwrap_or(0)\n"
            "#[cfg(test)]\n"
            "mod tests { fn c() { None.unwrap_or(1); } }\n"
        )
        (root / "crates" / "spectra-core" / "tests").mkdir()
        (root / "crates" / "spectra-core" / "tests" / "t.rs").write_text("fn d() { None.unwrap_or(2); }\n")
        (root / "crates" / "spectra-api" / "src" / "lib.rs").write_text(
            "const FEE: f64 = 1.0;\n"
            "pub(crate) const SLIP: Bps = Bps::from_f64(5.0);\n"
            "fn e() { Money::from_f64(FEE) }\n"
            "fn f() { Bps::from_f64(2.5) }\n"
        )
        allow = root / "allow.txt"
        allow.write_text(
            "# fixture\n"
            "crates/spectra-core/src/lib.rs | defined | .copied().unwrap_or(0.0) | absent = zero\n"
            "crates/spectra-core/src/lib.rs | defined | v.first().map_or(0, |b| i64::from(*b)) | empty = 0\n"
            "crates/spectra-core/src/lib.rs | follow-up | .unwrap_or(42) | gone\n"
        )
        problems, counts, n_hits = check(root, allow)
        expected = [
            "crates/spectra-core/src/lib.rs:2: silent default not in the allowlist",
            "crates/spectra-api/src/lib.rs:4: silent default not in the allowlist",
            "allow.txt:4: stale entry",
        ]
        ok = n_hits == 4 and counts["defined"] == 2 and len(problems) == len(expected) and all(
            p.startswith(want) for p, want in zip(problems, expected)
        )
        allow.write_text("x | made-up | y | z\n")
        bad, _, _ = check(root, allow)
        ok = ok and any("unknown category" in p for p in bad)
    if not ok:
        print("self-test failed:", n_hits, counts, *problems, *bad, sep="\n  ")
        return 1
    print("check_silent_defaults self-test passed")
    return 0


def main(argv):
    if argv[1:] == ["--self-test"]:
        return self_test()
    if argv[1:]:
        print(__doc__, file=sys.stderr)
        return 2
    problems, counts, n_hits = check(ROOT, ALLOWLIST)
    summary = ", ".join(f"{k} {v}" for k, v in counts.items())
    if problems:
        print(*problems, sep="\n")
        print(f"\n{len(problems)} problem(s); {n_hits} hits ({summary}).")
        print("Fix the default, or add an entry to scripts/silent_defaults_allowlist.txt "
              "(see docs/models/INVENTORY.md, 'Silent defaults check').")
        return 1
    print(f"no new silent defaults: {n_hits} hits, all allowed ({summary})")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
