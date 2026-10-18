// Golden-output generator for the backtester: runs a fixed set of strategies,
// markets and configurations on the FROZEN C++ reference (quantforge v0.6.0)
// and prints every run's fingerprints and metrics. The Rust port, run with the
// reference ledger policy (no leverage limit), must print the same
// (crates/spectra-engine/tests/parity_backtest.rs).

#include <cstdio>
#include <string>
#include <vector>

#include "quantforge/backtest.hpp"
#include "quantforge/rng.hpp"

using namespace qf;

namespace {

// Random multi-instrument market (same generator as the property tests).
std::vector<Bar> random_market(std::uint64_t seed, int instruments, int timestamps) {
  Rng rng(seed);
  std::vector<i64> last(static_cast<std::size_t>(instruments));
  for (auto& px : last) px = rng.range(20 * kScale, 300 * kScale);
  std::vector<Bar> bars;
  for (i64 t = 1; t <= timestamps; ++t) {
    for (int k = 0; k < instruments; ++k) {
      if (rng.range(0, 19) == 0) continue;
      i64& prev = last[static_cast<std::size_t>(k)];
      const i64 open = std::max<i64>(prev + rng.range(-kScale, kScale), kScale);
      const i64 close = std::max<i64>(open + rng.range(-2 * kScale, 2 * kScale), kScale);
      const i64 high = std::max(open, close) + rng.range(0, kScale);
      const i64 low = std::max<i64>(std::min(open, close) - rng.range(0, kScale), 1);
      Bar b;
      b.ts_ns = t * 60'000'000'000LL;
      b.instrument = static_cast<InstrumentId>(k);
      b.open = Price::from_raw(open);
      b.high = Price::from_raw(high);
      b.low = Price::from_raw(low);
      b.close = Price::from_raw(close);
      b.volume = rng.range(0, 9) == 0 ? Qty{} : Qty::from_raw(rng.range(1, 1000) * kScale);
      bars.push_back(b);
      prev = close;
    }
  }
  return bars;
}

class RandomStrategy : public Strategy {
 public:
  explicit RandomStrategy(std::uint64_t seed) : rng_(seed) {}
  void on_start(Context& ctx) override {
    if (rng_.range(0, 1) == 0) last_ = ctx.market(0, Side::Buy, Qty::from_raw(rng_.range(1, 10 * kScale)));
  }
  void on_bar(Context& ctx, std::span<const Bar> group) override {
    for (const Bar& b : group) {
      const i64 action = rng_.range(0, 11);
      const Side side = rng_.range(0, 1) == 0 ? Side::Buy : Side::Sell;
      const Qty qty = Qty::from_raw(rng_.range(-kScale / 10, 50 * kScale));  // occasionally malformed
      if (action < 3) {
        last_ = ctx.market(b.instrument, side, qty);
      } else if (action < 7) {
        const i64 d = std::max<i64>(b.close.raw() / 30, 1);
        const i64 px = std::max<i64>(b.close.raw() + rng_.range(-d, d), 1);
        last_ = ctx.limit(b.instrument, side, qty, Price::from_raw(px));
      } else if (action == 7 && last_ != 0) {
        static_cast<void>(ctx.cancel(last_));
      } else if (action == 8) {
        static_cast<void>(ctx.cancel(static_cast<OrderId>(rng_.range(0, 400))));
      } else if (action == 9) {
        const Qty pos = ctx.position(b.instrument);
        if (!pos.is_zero()) static_cast<void>(ctx.market(b.instrument, pos.raw() > 0 ? Side::Sell : Side::Buy, pos.abs()));
      }
    }
  }
  void on_fill(Context& ctx, const Fill& f) override {
    if (rng_.range(0, 9) == 0) last_ = ctx.limit(f.instrument, f.side == Side::Buy ? Side::Sell : Side::Buy, f.qty,
                                                 Price::from_raw(f.price.raw() + 1));
  }

 private:
  Rng rng_;
  OrderId last_ = 0;
};

class SmaCross : public Strategy {
 public:
  SmaCross(std::size_t fast, std::size_t slow) : fast_(fast), slow_(slow) {}
  void on_bar(Context& ctx, std::span<const Bar> bars) override {
    const InstrumentId id = bars[0].instrument;
    const auto h = ctx.history(id);
    if (h.size() < slow_) return;
    const auto sma = [&h](std::size_t n) {
      i128 sum = 0;
      for (std::size_t i = h.size() - n; i < h.size(); ++i) sum += h[i].close.raw();
      return sum / static_cast<i128>(n);
    };
    const bool bullish = sma(fast_) > sma(slow_);
    const Qty pos = ctx.position(id);
    if (bullish && pos.is_zero()) {
      static_cast<void>(ctx.market(id, Side::Buy, Qty::from_raw(100 * kScale)));
    } else if (!bullish && pos.raw() > 0) {
      static_cast<void>(ctx.market(id, Side::Sell, pos));
    }
  }

 private:
  std::size_t fast_;
  std::size_t slow_;
};

void print(const char* name, const BacktestResult& r) {
  std::size_t fills = 0;
  for (const JournalEntry& e : r.ledger.journal()) fills += e.type == EntryType::Fill ? 1U : 0U;
  int states[5] = {0, 0, 0, 0, 0};
  int reasons[5] = {0, 0, 0, 0, 0};
  for (const Order& o : r.orders) {
    ++states[static_cast<int>(o.state)];
    ++reasons[static_cast<int>(o.reject_reason)];
  }
  std::printf("%s\n  dataset %s\n  config  %s\n  result  %s\n", name, r.run.dataset_sha256.c_str(),
              r.run.config_sha256.c_str(), r.run.result_sha256.c_str());
  std::printf("  bars %zu fills %zu orders %zu states %d %d %d %d %d reasons %d %d %d %d %d\n", r.run.bar_count, fills,
              r.orders.size(), states[0], states[1], states[2], states[3], states[4], reasons[0], reasons[1],
              reasons[2], reasons[3], reasons[4]);
  std::printf("  equity %s cash %s fees %s realized %s\n", r.ledger.equity().to_string().c_str(),
              r.ledger.cash().to_string().c_str(), r.ledger.total_fees().to_string().c_str(),
              r.ledger.realized_pnl().to_string().c_str());
  const Metrics& m = r.metrics;
  std::printf("  metrics %d %d %.17g %.17g %.17g %.17g %.17g\n", m.valid ? 1 : 0, m.periods, m.total_return,
              m.annualized_return, m.annualized_volatility, m.sharpe, m.max_drawdown);
}

}  // namespace

int main() {
  // 1. The SMA demo exactly as shipped.
  {
    Rng rng(2026);
    std::vector<Bar> bars;
    i64 px = 100 * kScale;
    for (std::size_t i = 0; i < 1000; ++i) {
      const i64 open = px;
      const i64 close = std::max<i64>(open + rng.range(-kScale * 3 / 2, kScale * 3 / 2 + kScale / 20), kScale);
      Bar b;
      b.ts_ns = static_cast<i64>(i + 1) * 86'400'000'000'000LL;
      b.instrument = 1;
      b.open = Price::from_raw(open);
      b.close = Price::from_raw(close);
      b.high = Price::from_raw(std::max(open, close) + rng.range(0, kScale / 2));
      b.low = Price::from_raw(std::max<i64>(std::min(open, close) - rng.range(0, kScale / 2), 1));
      b.volume = Qty::from_raw(rng.range(100'000, 1'000'000) * kScale);
      bars.push_back(b);
      px = close;
    }
    for (const auto [fast, slow] : {std::pair<std::size_t, std::size_t>{10, 30}, {3, 15}, {20, 80}}) {
      BacktestConfig cfg;
      cfg.initial_cash = Money::from_raw(100'000 * kScale);
      cfg.costs.slippage = Bps::from_double(2);
      cfg.costs.commission.bps_of_notional = Bps::from_double(1);
      cfg.seed = 2026;
      SmaCross s(fast, slow);
      char name[64];
      std::snprintf(name, sizeof name, "sma %zu/%zu", fast, slow);
      print(name, run_backtest(bars, s, cfg));
    }
  }
  // 2. Random strategies on random markets across 12 configurations.
  for (int c = 0; c < 12; ++c) {
    BacktestConfig cfg;
    cfg.initial_cash = Money::from_raw((50'000 + 25'000 * c) * kScale);
    cfg.ledger.allow_short = c % 2 == 1;
    cfg.ledger.allow_negative_cash = c % 4 >= 2;
    cfg.costs.slippage = Bps::from_raw(c * 2 * kScale + 12345);
    cfg.costs.commission.per_unit = Money::from_raw(c % 3 == 0 ? 1'000'000 : 0);
    cfg.costs.commission.bps_of_notional = Bps::from_raw(c % 3 == 1 ? 3 * kScale : 0);
    cfg.costs.commission.min_fee = Money::from_raw(c % 5 == 0 ? kScale : 0);
    cfg.exec.max_participation = Qty::from_raw(c % 3 == 2 ? kScale / 20 : 0);
    cfg.exec.delay_bars = static_cast<std::size_t>(c % 4);
    cfg.seed = static_cast<std::uint64_t>(c) * 977 + 3;
    cfg.periods_per_year = c % 2 == 0 ? 252.0 : 98280.0;
    cfg.risk_free_rate = c % 3 == 0 ? 0.0 : 0.035;
    for (std::uint64_t seed = 1; seed <= 3; ++seed) {
      const std::vector<Bar> bars = random_market(seed * 101 + static_cast<std::uint64_t>(c), 1 + c % 4, 300);
      RandomStrategy s(seed * 7 + static_cast<std::uint64_t>(c));
      char name[64];
      std::snprintf(name, sizeof name, "random config %d seed %llu", c, static_cast<unsigned long long>(seed));
      print(name, run_backtest(bars, s, cfg));
    }
  }
  return 0;
}
