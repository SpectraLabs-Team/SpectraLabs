#!/usr/bin/env python3
"""Generate ledger operation scripts for the C++ <-> Rust parity check.

Each script is a plain-text list of ledger operations. The frozen C++ engine
replays it (parity/cpp/ledger_golden.cpp) to produce a golden file; the Rust
test `crates/spectra-core/tests/parity_ledger.rs` replays the same script and
must produce byte-identical output.

Format (all amounts are raw fixed-point integers, scale 1e8):
    config <allow_short 0|1> <allow_negative_cash 0|1>
    D <amount> <ts>                      deposit
    W <amount> <ts>                      withdraw
    F <inst> <B|S> <qty> <price> <fee> <ts>   fill
    M <inst> <price>                     mark

Deterministic: rerunning this script produces identical files.
Usage: python3 parity/gen_ops.py parity/ops
"""
import os
import sys

SCALE = 100_000_000
MAX_RAW = (1 << 63) - 1


class SplitMix64:
    def __init__(self, seed):
        self.s = seed & 0xFFFFFFFFFFFFFFFF

    def next(self):
        self.s = (self.s + 0x9E3779B97F4A7C15) & 0xFFFFFFFFFFFFFFFF
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & 0xFFFFFFFFFFFFFFFF
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & 0xFFFFFFFFFFFFFFFF
        return z ^ (z >> 31)

    def range(self, lo, hi):
        return lo + self.next() % (hi - lo + 1)


def session(seed, allow_short, allow_neg, steps):
    rng = SplitMix64(seed)
    lines = [f"config {int(allow_short)} {int(allow_neg)}"]
    ts = 1_700_000_000_000_000_000
    lines.append(f"D {rng.range(1_000, 1_000_000) * SCALE} {ts}")
    prices = [rng.range(1 * SCALE, 500 * SCALE) for _ in range(4)]
    for _ in range(steps):
        ts += rng.range(1, 60_000_000_000)
        for i in range(len(prices)):
            prices[i] = max(1, prices[i] + rng.range(-2 * SCALE, 2 * SCALE))
        op = rng.range(0, 99)
        inst = rng.range(0, 3)
        if op < 70:
            mode = rng.range(0, 19)
            if mode == 0:
                qty = rng.range(1, 10)                      # dust: notional may round to 0
            elif mode == 1:
                qty = rng.range(-5, 0)                       # malformed
            elif mode == 2:
                qty = rng.range(10**15, 10**17)              # huge: overflow paths
            elif mode in (3, 4):
                qty = (2 * rng.range(0, 40) + 1) * (SCALE // 2)  # odd multiple of 0.5: forces .5 ties
            elif mode < 8:
                qty = rng.range(1, 1_000_000)
            else:
                qty = rng.range(1, 200 * SCALE)
            price = prices[inst]
            if mode in (3, 4):
                price |= 1                                   # odd raw price: notional ends in exactly .5
            pmode = rng.range(0, 29)
            if pmode == 0:
                price = rng.range(-3, 0)                     # malformed
            elif pmode == 1:
                price = rng.range(1, 3)                      # tiny prices
            fee = 0 if rng.range(0, 2) == 0 else rng.range(-1 if rng.range(0, 40) == 0 else 0, 20 * SCALE)
            side = "B" if rng.range(0, 1) == 0 else "S"
            lines.append(f"F {inst} {side} {qty} {price} {fee} {ts}")
        elif op < 78:
            amt = rng.range(-2, 100_000 * SCALE) if rng.range(0, 30) else MAX_RAW - rng.range(0, 1000)
            lines.append(f"D {amt} {ts}")
        elif op < 86:
            lines.append(f"W {rng.range(-2, 200_000 * SCALE)} {ts}")
        else:
            price = prices[inst] if rng.range(0, 20) else rng.range(-2, 0)
            lines.append(f"M {inst} {price}")
    return lines


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "parity/ops"
    os.makedirs(out, exist_ok=True)
    configs = [(False, False), (True, False), (False, True), (True, True)]
    n = 0
    for c, (s, neg) in enumerate(configs):
        for k in range(3):
            seed = 0x5EC7_0000 + c * 100 + k
            path = os.path.join(out, f"ledger_{c}{k}.ops")
            with open(path, "w", newline="\n") as fh:
                fh.write("\n".join(session(seed, s, neg, 400)) + "\n")
            n += 1
    print(f"wrote {n} scripts to {out}")


if __name__ == "__main__":
    main()
