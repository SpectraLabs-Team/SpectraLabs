// Golden-output generator for the registry and holdout vault, on the FROZEN
// C++ reference (quantforge v0.6.0). It runs the research-workflow demo with a
// deterministic clock and writes:
//   <out>/registry.jsonl   the complete hash-chained log
//   stdout                 a transcript of every number the workflow reports
// The Rust port must produce a byte-identical log and transcript
// (crates/spectra-registry/tests/parity_registry.rs).
//
// Usage: registry_golden <work_dir>

#include <algorithm>
#include <cstdio>
#include <filesystem>
#include <memory>
#include <string>
#include <vector>

#include "quantforge/rng.hpp"
#include "quantforge/sha256.hpp"
#include "quantforge/survival.hpp"
#include "quantforge/validation.hpp"
#include "quantforge/vault.hpp"

using namespace qf;
namespace fs = std::filesystem;

namespace {

std::string noise_csv(std::size_t n, std::uint64_t seed) {
  Rng rng(seed);
  std::string csv = "timestamp,symbol,open,high,low,close,volume\n";
  double px = 100.0;
  for (std::size_t i = 0; i < n; ++i) {
    const double open = px;
    const double close = std::max(open * (1.0 + 0.01 * rng.normal()), 1.0);
    const i64 ts = (1'577'836'800LL + static_cast<i64>(i) * 86'400) * 1'000'000'000LL;
    csv += format_timestamp(ts) + ",DEMO," + Price::from_double(open).to_string() + "," +
           Price::from_double(std::max(open, close) * 1.002).to_string() + "," +
           Price::from_double(std::min(open, close) * 0.998).to_string() + "," + Price::from_double(close).to_string() +
           ",1000000\n";
    px = close;
  }
  return csv;
}

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
      static_cast<void>(ctx.market(id, Side::Buy, Qty::from_double(100)));
    } else if (!bullish && pos.raw() > 0) {
      static_cast<void>(ctx.market(id, Side::Sell, pos));
    }
  }

 private:
  std::size_t fast_;
  std::size_t slow_;
};

}  // namespace

int main(int argc, char** argv) {
  if (argc < 2) {
    std::fprintf(stderr, "usage: registry_golden <work_dir>\n");
    return 2;
  }
  const fs::path work = argv[1];
  fs::remove_all(work);
  fs::create_directories(work);

  auto tick = std::make_shared<i64>(0);
  const Clock clock = [tick]() { return 1'700'000'000'000'000'000LL + (++*tick) * 1'000'000LL; };

  DataStore store(work / "data");
  Registry registry(work / "registry", clock);
  Vault vault(registry, store);

  const std::string kStrategy = "sma-cross";
  const std::string kSource = sha256_hex("SmaCross: long while SMA(fast) > SMA(slow), fixed 100 units, v1");
  const StrategyFactory factory = [](const ParamSet& p) {
    return std::make_unique<SmaCross>(static_cast<std::size_t>(p.get_int("fast")),
                                      static_cast<std::size_t>(p.get_int("slow")));
  };
  BacktestConfig cfg;
  cfg.initial_cash = Money::from_double(1'000'000);
  cfg.costs.slippage = Bps::from_double(2);
  cfg.costs.commission.bps_of_notional = Bps::from_double(1);
  cfg.seed = 2026;

  const Dataset research = load_csv(noise_csv(1000, 31337));
  const Dataset hold = load_csv(noise_csv(500, 424242));
  store.put(research);
  store.put(hold);
  std::printf("datasets %s %s\n", research.info.id.c_str(), hold.info.id.c_str());

  vault.seal("final-check", hold.info.id, 2, "one honest look before any paper trading");
  try {
    vault.seal("again", hold.info.id, 1, "");
  } catch (const VaultError& e) {
    std::printf("seal refused: %s\n", e.what());
  }

  const std::string exp1 = registry.create_experiment(
      "sma-premium", "A simple moving-average crossover earns a risk premium", {"trend", "sma"});
  ParamGrid grid;
  grid.add("fast", {2, 3, 4, 5, 6, 8, 10, 12}).add("slow", {15, 20, 25, 30, 40, 50, 60, 80});
  const SweepResult sweep1 = run_sweep(research.bars, factory, cfg, grid.all(), objective_sharpe, {4});
  const std::vector<std::string> ids1 = registry.record_sweep(exp1, research.info.id, kStrategy, kSource, cfg, sweep1);
  const Trial best1 = *sweep1.best_trial();
  std::printf("exp1 %s best %s %.17g\n", exp1.c_str(), best1.params.canonical().c_str(), best1.metrics.sharpe);

  const ValidationReport v1 = validate_selection(best1, registry.trial_ledger(), cfg.periods_per_year);
  std::printf("v1 %zu %.17g %.17g\n", v1.n_trials, v1.psr_vs_zero, v1.dsr);

  const std::string exp2 = registry.fork_experiment(exp1, "sma-premium-v2", "Maybe a different grid works", {"trend"});
  ParamGrid grid2;
  grid2.add("fast", {1, 2, 3, 4, 5, 7, 9, 11}).add("slow", {10, 14, 18, 22, 28, 35, 45, 70});
  const SweepResult sweep2 = run_sweep(research.bars, factory, cfg, grid2.all(), objective_sharpe, {4});
  static_cast<void>(registry.record_sweep(exp2, research.info.id, kStrategy, kSource, cfg, sweep2));
  const ValidationReport v2 = validate_selection(best1, registry.trial_ledger(), cfg.periods_per_year);
  std::printf("v2 %zu %.17g %.17g\n", v2.n_trials, v2.psr_vs_zero, v2.dsr);
  std::printf("lineage");
  for (const std::string& id : registry.lineage(exp2)) std::printf(" %s", registry.experiment(id).name.c_str());
  std::printf("\n");

  registry.add_note(exp1, "first grid looked promising");
  registry.add_note(ids1[3], "inspected by hand");

  SurvivalConfig sc;
  sc.integer_params = {"fast", "slow"};
  const SurvivalReport survival = run_survival_lab(research.bars, factory, cfg, best1.params, sc);
  Json summary = Json::object();
  summary.set("worst_verdict", Json::string(to_string(survival.worst)));
  const std::string report_text = format_survival_report(survival);
  const std::string art =
      registry.record_artifact(exp1, ids1[0], "survival_report", report_text, "(not saved)", summary);
  std::printf("artifact %s %s\n", art.c_str(), to_string(survival.worst));

  try {
    static_cast<void>(vault.checked_get(hold.info.id));
  } catch (const VaultError& e) {
    std::printf("refused: %s\n", e.what());
  }
  std::printf("checked_get research bars %zu\n", vault.checked_get(research.info.id).bars.size());
  for (int k = 0; k < 3; ++k) {
    try {
      const HoldoutResult h = vault.evaluate("final-check", factory, kStrategy, kSource, best1.params, cfg);
      std::printf("holdout %zu/%zu rem %zu sharpe %.17g ret %.17g sr %.17g n %zu psr %.17g dsr %.17g ems %.17g %s\n",
                  h.evaluation_no, h.budget, h.remaining, h.metrics.sharpe, h.metrics.total_return,
                  h.sharpe_per_period, h.n_obs, h.psr_vs_zero, h.dsr, h.expected_max_sharpe, h.result_sha256.c_str());
      for (const std::string& w : h.warnings) std::printf("  warning: %s\n", w.c_str());
    } catch (const VaultError& e) {
      std::printf("evaluate refused: %s\n", e.what());
    }
  }
  try {
    static_cast<void>(vault.evaluate("nope", factory, kStrategy, kSource, best1.params, cfg));
  } catch (const VaultError& e) {
    std::printf("evaluate refused: %s\n", e.what());
  }
  for (const HoldoutStatus& st : vault.list()) {
    std::printf("status %s %s budget %zu started %zu completed %zu failed %zu denied %zu remaining %zu\n",
                st.name.c_str(), st.dataset_id.c_str(), st.budget, st.started, st.completed, st.failed, st.denied,
                st.remaining());
  }

  const RunComparison c = registry.compare(ids1[0], ids1[9]);
  std::printf("compare %d %d %d %d %d %d %.17g %.17g\n", c.same_dataset, c.same_strategy_source, c.same_params,
              c.same_config, c.same_engine, c.same_result, c.sharpe_delta, c.total_return_delta);
  for (const std::string& d : c.differences) std::printf("  diff: %s\n", d.c_str());

  RunFilter f;
  f.experiment_id = exp1;
  f.min_sharpe = 0.0;
  std::printf("runs exp1 sharpe>=0: %zu of %zu; trials %zu\n", registry.runs(f).size(), registry.runs().size(),
              registry.trial_count());
  const VerifyReport vr = registry.verify();
  std::printf("verify %d %zu %s\n", vr.ok, vr.events, vr.last_hash.c_str());
  std::string best_run_id;
  for (const RunInfo& r : registry.runs(f)) {
    if (r.result_sha256 == best1.run.result_sha256) best_run_id = r.id;
  }
  const ReproductionReport rr = reproduce_run(registry, store, best_run_id, factory, kSource);
  std::printf("reproduce %s %d %s\n", best_run_id.c_str(), rr.reproduced, rr.actual_result_sha256.c_str());
  for (const std::string& n : rr.notes) std::printf("  note: %s\n", n.c_str());
  return 0;
}
