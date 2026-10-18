#!/usr/bin/env python3
"""Generate Data Studio parity cases (CSV files + options) and decimal/timestamp
parse tables. Deterministic: rerunning produces identical files.

Each case is <name>.csv plus <name>.opts with one `key=value` per line:
  skip=0|1  delim=<char>  offset=<minutes>  tf=iso|s|ms|us|ns  symbol=<default>
  ts= open= high= low= close= volume= sym=   (explicit column names; may be empty)
Usage: python3 parity/gen_data.py parity/data
"""
import os
import random
import sys

H = "timestamp,symbol,open,high,low,close,volume\n"
CASES = {
    "messy": ("﻿Date,Symbol,Close,Open,High,Low,Volume\r\n2024-01-03,\"MSFT\",101.5,100.5,102,100,2000\r\n"
              "2024-01-02,AAPL,10.5,10,11,9.5,1000\r\n\r\n2024-01-02,\"MSFT\",100.5,100,101,99.5,1500\r\n"
              "2024-01-03,AAPL,10.75,10.5,11,10.25,900\r\n", {}),
    "explicit_cols": ("when;px_o;px_h;px_l;px_c;qty\n2024-01-01T00:00:00Z;1;2;0.5;1.5;10\n",
                      {"delim": ";", "ts": "WHEN", "open": "px_o", "high": "px_h", "low": "px_l", "close": "px_c",
                       "volume": "qty", "symbol": "BTC-USD"}),
    "no_volume": ("timestamp,open,high,low,close\n2024-01-01T00:00:00Z,1,2,0.5,1.5\n", {}),
    "quoted": (H + "2024-01-01T00:00:00Z,\"A,B\",1,2,0.5,1.5,10\n2024-01-01T00:00:00Z,\"say \"\"hi\"\"\",1,2,0.5,1.5,10\n", {}),
    "strict_bad_number": (H + "2024-01-01T00:00:00Z,X,1,2,0.5,1.5,10\n2024-01-02T00:00:00Z,X,1,2,0.5,abc,10\n", {}),
    "strict_bad_ts": (H + "garbage,X,1,2,0.5,1.5,10\n", {}),
    "strict_ohlc": (H + "2024-01-01T00:00:00Z,X,1,0.9,0.5,1.5,10\n", {}),
    "strict_nonpos": (H + "2024-01-01T00:00:00Z,X,0,2,0.5,1.5,10\n", {}),
    "strict_negvol": (H + "2024-01-01T00:00:00Z,X,1,2,0.5,1.5,-1\n", {}),
    "strict_colcount": (H + "2024-01-01T00:00:00Z,X,1,2,0.5,1.5\n", {}),
    "strict_empty_sym": (H + "2024-01-01T00:00:00Z,,1,2,0.5,1.5,10\n", {}),
    "strict_9dp": (H + "2024-01-01T00:00:00Z,X,1,2,0.5,1.123456789,10\n", {}),
    "empty": ("", {}),
    "missing_close": ("timestamp,open,high,low\n2024-01-01,1,2,3\n", {}),
    "header_only": ("timestamp,open,high,low,close\n", {}),
    "unterminated": ("timestamp,open,high,low,close\n2024-01-01,1,2,0.5,\"1.5\n", {}),
    "explicit_missing": ("timestamp,open,high,low,close\n2024-01-01,1,2,0.5,1.5\n", {"close": "nonexistent"}),
    "skip_all_bad": ("timestamp,open,high,low,close\n2024-01-01,x,2,0.5,1.5\n", {"skip": "1"}),
    "skip_reasons": (H + "2024-01-01T00:00:00Z,X,1,2,0.5,1.5,10\nnot-a-time,X,1,2,0.5,1.5,10\n"
                     "2024-01-02T00:00:00Z,X,1,0.9,0.5,1.5,10\n2024-01-03T00:00:00Z,X,1,2,0.5,1.5,-3\n"
                     "2024-01-04T00:00:00Z,X,-1,2,0.5,1.5,10\n2024-01-05T00:00:00Z,X,1,2\n"
                     "2024-01-06T00:00:00Z,,1,2,0.5,1.5,10\n2024-01-07T00:00:00Z,X,1,2,0.5,zzz,10\n", {"skip": "1"}),
    "dup_conflict_strict": (H + "2024-01-01T00:00:00Z,X,1,2,0.5,1.5,10\n2024-01-01T00:00:00Z,X,7,8,6,7.5,99\n"
                            "2024-01-02T00:00:00Z,X,1,2,0.5,1.5,10\n", {}),
    "dup_conflict_skip": (H + "2024-01-01T00:00:00Z,X,1,2,0.5,1.5,10\n2024-01-01T00:00:00Z,X,7,8,6,7.5,99\n"
                          "2024-01-02T00:00:00Z,X,1,2,0.5,1.5,10\n", {"skip": "1"}),
    "dup_exact_skip": (H + "2024-01-02T00:00:00Z,X,2,3,1.5,2.5,20\n2024-01-01T00:00:00Z,X,1,2,0.5,1.5,10\n"
                       "2024-01-01T00:00:00Z,X,1,2,0.5,1.5,10\n", {"skip": "1"}),
    "naive_count": ("timestamp,open,high,low,close\n2024-01-01,1,2,0.5,1.5\n2024-01-01,1,2,0.5,1.5\n"
                    "2024-01-02T00:00:00Z,1,2,0.5,1.5\n2024-01-03,1,2,0.5,1.5\n", {"skip": "1"}),
    "naive_offset": ("timestamp,open,high,low,close\n2020-01-01T05:30:00,1,2,0.5,1.5\n", {"offset": "330"}),
    "naive_negative_offset": ("timestamp,open,high,low,close\n2020-01-01T05:30:00,1,2,0.5,1.5\n", {"offset": "-300"}),
    "epoch_ms": ("ts,o,h,l,c,v\n1700000000000,1,2,0.5,1.5,3\n1700000060000,1.5,2,1,1.75,4\n", {"tf": "ms"}),
    "epoch_s_neg": ("ts,o,h,l,c,v\n-86400,1,2,0.5,1.5,3\n0,1.5,2,1,1.75,4\n", {"tf": "s"}),
    "tab_delim": ("timestamp\tsymbol\topen\thigh\tlow\tclose\tvolume\n2024-01-01\tZ\t1\t2\t0.5\t1.5\t1e3\n",
                  {"delim": "\t"}),
}


def series(rng, n, symbols, quirks):
    rows = []
    for s in symbols:
        px = rng.uniform(5, 500)
        t = 1_704_067_200  # 2024-01-01
        stale = 0
        for i in range(n):
            t += 60
            if quirks and rng.random() < 0.02:
                t += 60 * rng.randint(10, 500)  # gap
            if quirks and rng.random() < 0.01:
                stale = rng.randint(5, 9)
            if stale > 0:
                stale -= 1
                o = h = l = c = px
            else:
                o = px
                r = rng.gauss(0, 0.002)
                if quirks and rng.random() < 0.005:
                    r = rng.choice([-0.4, 0.5])  # bad tick
                c = max(0.01, o * (1 + r))
                h = max(o, c) * (1 + abs(rng.gauss(0, 0.001)))
                l = min(o, c) * (1 - abs(rng.gauss(0, 0.001)))
            vol = 0 if quirks and rng.random() < 0.01 else rng.randint(1, 100000)
            rows.append((t, s, f"{o:.4f}", f"{h:.4f}", f"{l:.4f}", f"{c:.4f}", str(vol)))
            px = float(f"{c:.4f}")
    return rows


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "parity/data"
    os.makedirs(out, exist_ok=True)
    rng = random.Random(20261018)
    gen = dict(CASES)
    for k, (n, syms, quirks, shuffle) in enumerate([(300, ["AAA"], False, False), (400, ["AAA", "BBB", "C"], True, True),
                                                    (250, ["X", "Y"], True, False)]):
        rows = series(rng, n, syms, quirks)
        if shuffle:
            rng.shuffle(rows)
        body = "".join(f"{t},{s},{o},{h},{l},{c},{v}\n" for (t, s, o, h, l, c, v) in rows)
        gen[f"random_{k}"] = ("time,ticker,open,high,low,close,vol\n" + body, {"tf": "s"})
    for name, (csv, opts) in sorted(gen.items()):
        with open(os.path.join(out, name + ".csv"), "wb") as f:
            f.write(csv.encode("utf-8"))
        with open(os.path.join(out, name + ".opts"), "w", newline="\n") as f:
            for key in ["skip", "delim", "offset", "tf", "symbol", "ts", "open", "high", "low", "close", "volume", "sym"]:
                default = {"skip": "0", "delim": ",", "offset": "0", "tf": "iso", "symbol": "DEFAULT"}.get(key, "")
                v = opts.get(key, default)
                f.write(f"{key}={'TAB' if v == chr(9) else v}\n")
    fixed = ["190.25", " -0.00000001 ", "1.5e2", "+3", "0.000000001", "1.000000010", "abc", "1.2.3", "1e", ".",
             "99999999999", "92233720368.54775807", "92233720368.54775808", "-92233720368.54775807", "0e99999",
             "0", "00012.5000", "1E-8", "1e-9", "123456789012345678901234567890", "1234567890123456789012345678901",
             "5.", ".5", "- 1", "1 2", "", "  ", "0.1e1", "12e+3", "7e-0008"]
    times = ["1970-01-01", "2020-01-01T00:00:00Z", "2020-01-01T05:30:00+05:30", "2020-01-01 05:30+0530",
             "2020-01-01T00:00:00.5Z", "2021-02-29", "2020-02-29", "2020-13-01", "2020-01-01T24:00", "2020-01-01T23:59:59",
             "2020-01-01T00:00:00.1234567891Z", "2020-01-01T00:00:00.123456789-08", "2020-01-01T10:00-0130",
             "1900-03-01", "2262-04-11T23:47:16.854775807Z", "2262-04-11T23:47:16.854775808Z", "0001-01-01",
             "2020-1-01", "2020-01-01X", "2020-01-01T10", "2020-01-01T10:00:", "2020-01-01T10:00:00.",
             "2020-01-01T10:00+24:00", "2020-01-01T10:00+05:60", " 2020-01-01T10:00Z ", "1577836800", "-5", "+7",
             "12.5", "9223372036", "9223372037", "", "--1"]
    with open(os.path.join(out, "parse_fixed.txt"), "w", newline="\n") as f:
        f.write("\n".join(fixed) + "\n")
    with open(os.path.join(out, "parse_time.txt"), "w", newline="\n") as f:
        f.write("\n".join(times) + "\n")
    print(f"wrote {len(gen)} CSV cases to {out}")


if __name__ == "__main__":
    main()
