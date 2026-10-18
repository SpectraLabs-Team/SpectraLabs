// Golden-output generator for the research layer: sweeps, trial ledger, PBO,
// validation report, walk-forward, trade analysis, benchmark statistics and
// the full Survival Lab report, on the FROZEN C++ reference (quantforge
// v0.6.0). The Rust port must print the same (crates/spectra-research/tests/parity_research.rs).

#include <cstdio>
#include <memory>
#include <string>
#include <vector>

#include "quantforge/benchmark.hpp"
#include "quantforge/pbo.hpp"
#include "quantforge/rng.hpp"
#include "quantforge/survival.hpp"
#include "quantforge/sweep.hpp"
#include "quantforge/trades.hpp"
#include "quantforge/validation.hpp"
#include "quantforge/walkforward.hpp"

using namespace qf;

namespace {

std::vector<Bar> random_walk(std::size_t n, std::uint64_t seed, double drift, int instruments) {
  Rng rng(seed);
  std::vector<double> px(static_cast<std::size_t>(instruments), 100.0);
  std::vector<Bar> bars;
  for (std::size_t i = 0; i < n; ++i) {
    for (int k = 0; k < instruments; ++k) {
      const double open = px[static_cast<std::size_t>(k)];
      const double close = std::max(open * (1.0 + drift + 0.01 * rng.normal()), 1.0);
      Bar b;
      b.ts_ns = static_cast<i64>(i + 1);
      b.instrument = static_cast<InstrumentId>(k);
      b.open = Price::from_double(open);
      b.close = Price::from_double(close);
      b.high = Price::from_double(std::max(open, close) * 1.002);
      b.low = Price::from_double(std::min(open, close) * 0.998);
      b.volume = Qty::from_double(1'000'000);
      bars.push_back(b);
      px[static_cast<std::size_t>(k)] = close;
    }
  }
  return bars;
}

// Long/short SMA crossover on every instrument; rejects fast >= slow.
class SmaCross : public Strategy {
 public:
  SmaCross(std::size_t fast, std::size_t slow, bool shorts) : fast_(fast), slow_(slow), shorts_(shorts) {}
  void on_bar(Context& ctx, std::span<const Bar> bars) override {
    for (const Bar& b : bars) {
      const auto h = ctx.history(b.instrument);
      if (h.size() < slow_) continue;
      const auto sma = [&h](std::size_t n) {
        i128 sum = 0;
        for (std::size_t i = h.size() - n; i < h.size(); ++i) sum += h[i].close.raw();
        return sum / static_cast<i128>(n);
      };
      const bool bullish = sma(fast_) > sma(slow_);
      const Qty pos = ctx.position(b.instrument);
      const Qty unit = Qty::from_double(100);
      if (bullish && pos.raw() <= 0) {
        static_cast<void>(ctx.market(b.instrument, Side::Buy, pos.raw() < 0 ? unit + pos.abs() : unit));
      } else if (!bullish && pos.raw() > 0) {
        static_cast<void>(ctx.market(b.instrument, Side::Sell, shorts_ ? pos + unit : pos));
      } else if (!bullish && shorts_ && pos.is_zero()) {
        static_cast<void>(ctx.market(b.instrument, Side::Sell, unit));
      }
    }
  }

 private:
  std::size_t fast_;
  std::size_t slow_;
  bool shorts_;
};

void g(const char* label, double v) { std::printf("%s %.17g\n", label, v); }

void scenario(const char* title, const std::vector<Bar>& bars, bool shorts, std::size_t wf_train,
              std::size_t wf_test, std::size_t embargo, WalkForwardMode mode) {
  std::printf("==== %s\n", title);
  const ParamGrid grid = ParamGrid().add("fast", {2, 3, 4, 5, 6, 8, 10, 12}).add("slow", {5, 15, 20, 25, 30, 40, 60, 80});
  const std::vector<ParamSet> params = grid.all();
  const StrategyFactory factory = [shorts](const ParamSet& p) -> std::unique_ptr<Strategy> {
    const auto fast = p.get_int("fast");
    const auto slow = p.get_int("slow");
    if (fast >= slow) throw std::invalid_argument("fast must be below slow");
    return std::make_unique<SmaCross>(static_cast<std::size_t>(fast), static_cast<std::size_t>(slow), shorts);
  };
  // Grid includes invalid (fast >= slow) points; the sweep only uses valid ones.
  std::vector<ParamSet> valid;
  for (const ParamSet& p : params) {
    if (p.get_int("fast") < p.get_int("slow")) valid.push_back(p);
  }
  BacktestConfig cfg;
  cfg.initial_cash = Money::from_double(1'000'000);
  cfg.ledger.allow_short = shorts;
  cfg.costs.slippage = Bps::from_double(2);
  cfg.costs.commission.bps_of_notional = Bps::from_double(1);

  TrialLedger ledger;
  const SweepResult sweep = run_sweep(bars, factory, cfg, valid, objective_sharpe, {3}, ledger.next_trial_id());
  ledger.add("full-sample sweep", sweep);
  std::printf("trials %zu best %zu %s\n", sweep.trials.size(), sweep.best, sweep.best_trial()->params.canonical().c_str());
  for (const Trial& t : sweep.trials) {
    std::printf("trial %llu %s %s %.17g %zu\n", static_cast<unsigned long long>(t.id), t.params.canonical().c_str(),
                t.run.result_sha256.c_str(), t.objective, t.returns.size());
  }
  g("sharpe_variance", ledger.sharpe_variance());
  std::vector<std::vector<double>> all;
  for (const Trial& t : sweep.trials) all.push_back(t.returns);
  for (const std::size_t blocks : {std::size_t{8}, std::size_t{16}}) {
    const PboResult pbo = cscv_pbo(all, blocks);
    std::printf("pbo %zu %.17g %.17g %.17g %zu %zu %zu\n", blocks, pbo.pbo, pbo.prob_oos_loss, pbo.mean_logit,
                pbo.combinations, pbo.strategies, pbo.observations_used);
  }
  const PboResult pbo = cscv_pbo(all, 16);
  const ValidationReport rep = validate_selection(*sweep.best_trial(), ledger, cfg.periods_per_year, &pbo);
  std::printf("%s", format_report(rep).c_str());
  g("dsr", rep.dsr);
  g("psr", rep.psr_vs_zero);
  g("mintrl", rep.min_track_record_obs);

  WalkForwardConfig wf;
  wf.train = wf_train;
  wf.test = wf_test;
  wf.embargo = embargo;
  wf.mode = mode;
  const WalkForwardResult w = run_walk_forward(bars, factory, cfg, valid, wf, objective_sharpe, {2}, &ledger);
  std::printf("walk-forward folds %zu trials %zu ledger %zu\n", w.folds.size(), w.total_trials, ledger.count());
  for (const FoldResult& f : w.folds) {
    std::printf("fold %zu %zu %zu %zu %s %.17g %.17g %s\n", f.fold.train_begin, f.fold.train_end, f.fold.test_begin,
                f.fold.test_end, f.best_params.canonical().c_str(), f.in_sample_objective, f.out_of_sample_objective,
                f.oos_run.result_sha256.c_str());
  }
  g("wf_mean_is", w.mean_in_sample_objective);
  g("wf_mean_oos", w.mean_out_of_sample_objective);
  g("wf_oos_sharpe", w.oos_metrics.sharpe);
  g("wf_oos_return", w.oos_metrics.total_return);
  const ValidationReport rep2 = validate_selection(*sweep.best_trial(), ledger, cfg.periods_per_year, nullptr, 0.9);
  std::printf("%s", format_report(rep2).c_str());

  SurvivalConfig sc;
  sc.integer_params = {"fast", "slow"};
  sc.bootstrap_resamples = 500;
  const SurvivalReport sr = run_survival_lab(bars, factory, cfg, sweep.best_trial()->params, sc);
  std::printf("%s", format_survival_report(sr).c_str());
  const TradeStats& ts = sr.trades.stats;
  std::printf("tradestats %zu %zu %zu %.17g %.17g %.17g %.17g %.17g %.17g %.17g %.17g %.17g %.17g %.17g\n", ts.n,
              ts.wins, ts.losses, ts.win_rate, ts.avg_win, ts.avg_loss, ts.payoff_ratio, ts.expectancy,
              ts.profit_factor, ts.total_net, ts.best, ts.worst, ts.top_trade_share, ts.avg_holding_ns);
  const TradeResampling& rs = sr.trades.resampling;
  std::printf("resampling %d %.17g %.17g %.17g %.17g %.17g %.17g %.17g\n", rs.valid ? 1 : 0, rs.observed_max_drawdown,
              rs.bootstrap_terminal_pnl.p05, rs.bootstrap_terminal_pnl.p95, rs.bootstrap_max_drawdown.p50,
              rs.bootstrap_prob_loss, rs.reshuffle_max_drawdown.p95, rs.reshuffle_prob_worse_drawdown);
  const BenchmarkMetrics& bm = sr.benchmark.metrics;
  std::printf("benchmark %d %.17g %.17g %.17g %.17g %.17g %.17g %.17g %.17g\n", bm.valid ? 1 : 0, bm.beta,
              bm.alpha_annualized, bm.correlation, bm.tracking_error, bm.information_ratio, bm.up_capture,
              bm.down_capture, sr.benchmark.average_net_exposure);
  for (const auto& b : sr.regimes.buckets) {
    std::printf("regime %s %zu %.17g %.17g %.17g\n", b.name.c_str(), b.periods, b.share, b.mean_return, b.sharpe);
  }
  std::printf("risk %.17g %.17g %zu %.17g %.17g %.17g\n", sr.risk.drawdown.max_drawdown, sr.risk.drawdown.ulcer_index,
              sr.risk.drawdown.longest_underwater, sr.risk.tail.var, sr.risk.tail.es, sr.risk.tail.worst);
  std::printf("stability skipped %zu neighbours %zu\n", sr.stability.skipped, sr.stability.neighbours.size());
}

}  // namespace

int main() {
  // Random search sampling and grid enumeration.
  const ParamGrid big = ParamGrid().add("a", {1, 2, 3, 4, 5, 6, 7}).add("b", {0.5, 1.5, 2.5}).add("c", {10, 20, 30, 40, 50});
  for (const std::uint64_t seed : {1ULL, 2ULL, 99ULL}) {
    std::printf("sample %llu:", static_cast<unsigned long long>(seed));
    for (const ParamSet& p : big.sample(9, seed)) std::printf(" %s", p.canonical().c_str());
    std::printf("\n");
  }
  scenario("noise, long only", random_walk(1000, 12345, 0.0, 1), false, 300, 100, 0, WalkForwardMode::Rolling);
  scenario("drift, long/short, two instruments", random_walk(600, 777, 0.001, 2), true, 200, 80, 5,
           WalkForwardMode::Anchored);
  scenario("short sample", random_walk(160, 4242, 0.0005, 1), true, 60, 30, 0, WalkForwardMode::Rolling);
  return 0;
}
