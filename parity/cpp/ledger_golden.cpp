// Golden-output generator: replays a ledger operation script on the FROZEN C++
// reference ledger (quantforge v0.6.0) and prints the resulting state after
// every operation. The Rust port must print byte-identical output for the same
// script (crates/spectra-core/tests/parity_ledger.rs).
//
// Build against the C++ reference checkout, e.g.:
//   g++ -std=c++20 -O2 -I<quantforge>/include parity/cpp/ledger_golden.cpp \
//       <quantforge>/build/release/libqf_core.a -o ledger_golden
//   ./ledger_golden parity/ops/ledger_00.ops > parity/golden/ledger_00.golden
//
// Output format (raw fixed-point integers):
//   <op#> <status> cash=<> net=<> real=<> fees=<> eq=<> unr=<>
//   POS <id> <qty> <cost> <realized> <fees>          (end, ascending id)
//   J <seq> <type> <ts> <amount> <inst> <side> <qty> <price> <fee> <realized> <cash_after>
//   INV <ok|detail>

#include <cstdio>
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>

#include "quantforge/ledger.hpp"

using namespace qf;

namespace {

std::string state(const Ledger& l) {
  std::ostringstream o;
  o << "cash=" << l.cash().raw() << " net=" << l.net_deposits().raw() << " real=" << l.realized_pnl().raw()
    << " fees=" << l.total_fees().raw();
  // Evaluate before printing so an overflow never leaves a half-written field.
  std::string eq = "Overflow";
  std::string unr = "Overflow";
  try {
    eq = std::to_string(l.equity().raw());
  } catch (const OverflowError&) {
  }
  try {
    unr = std::to_string(l.unrealized_pnl().raw());
  } catch (const OverflowError&) {
  }
  o << " eq=" << eq << " unr=" << unr;
  return o.str();
}

const char* type_name(EntryType t) {
  switch (t) {
    case EntryType::Deposit: return "D";
    case EntryType::Withdrawal: return "W";
    case EntryType::Fill: return "F";
  }
  return "?";
}

}  // namespace

int main(int argc, char** argv) {
  if (argc != 2) {
    std::cerr << "usage: ledger_golden <script.ops>\n";
    return 2;
  }
  std::ifstream in(argv[1]);
  std::string line;
  std::getline(in, line);
  std::istringstream hdr(line);
  std::string word;
  int s = 0;
  int n = 0;
  hdr >> word >> s >> n;
  Ledger l(LedgerConfig{.allow_short = s != 0, .allow_negative_cash = n != 0});

  long op = 0;
  while (std::getline(in, line)) {
    if (line.empty()) continue;
    std::istringstream is(line);
    std::string kind;
    is >> kind;
    const char* status = "Ok";
    try {
      Status st = Status::Ok;
      if (kind == "D" || kind == "W") {
        long long amt = 0;
        long long ts = 0;
        is >> amt >> ts;
        st = kind == "D" ? l.deposit(Money::from_raw(amt), ts) : l.withdraw(Money::from_raw(amt), ts);
      } else if (kind == "F") {
        Fill f;
        unsigned inst = 0;
        std::string side;
        long long qty = 0;
        long long price = 0;
        long long fee = 0;
        long long ts = 0;
        is >> inst >> side >> qty >> price >> fee >> ts;
        f.instrument = inst;
        f.side = side == "B" ? Side::Buy : Side::Sell;
        f.qty = Qty::from_raw(qty);
        f.price = Price::from_raw(price);
        f.fee = Money::from_raw(fee);
        f.ts_ns = ts;
        st = l.apply_fill(f);
      } else if (kind == "M") {
        unsigned inst = 0;
        long long price = 0;
        is >> inst >> price;
        st = l.mark(inst, Price::from_raw(price));
      }
      status = to_string(st);
    } catch (const OverflowError&) {
      status = "Overflow";
    }
    std::cout << op++ << ' ' << status << ' ' << state(l) << '\n';
  }

  for (const auto& [id, p] : l.positions()) {
    std::cout << "POS " << id << ' ' << p.qty.raw() << ' ' << p.cost.raw() << ' ' << p.realized.raw() << ' '
              << p.fees.raw() << '\n';
  }
  for (const JournalEntry& e : l.journal()) {
    std::cout << "J " << e.seq << ' ' << type_name(e.type) << ' ' << e.ts_ns << ' ' << e.amount.raw() << ' '
              << e.fill.instrument << ' ' << (e.fill.side == Side::Buy ? 'B' : 'S') << ' ' << e.fill.qty.raw()
              << ' ' << e.fill.price.raw() << ' ' << e.fill.fee.raw() << ' ' << e.realized.raw() << ' '
              << e.cash_after.raw() << '\n';
  }
  const InvariantReport r = l.check_invariants();
  std::cout << "INV " << (r.ok ? "ok" : r.detail) << '\n';
  return 0;
}
