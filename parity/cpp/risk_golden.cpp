// Golden-output generator for the risk engine, on the FROZEN C++ reference
// (the PCPROJECT risk engine, CPU backend). The Rust port must print the same
// (crates/spectra-risk/tests/parity_risk.rs):
//   * the GBM terminal-loss kernel (mt19937_64 + libstdc++ normal_distribution),
//     path by path, bit for bit (%.17g round-trips a double exactly);
//   * every distribution metric (VaR, CVaR, volatility, downside/semi deviation,
//     skewness, kurtosis) on fixed and simulated loss vectors;
//   * full RiskEngine runs on the CPU backend;
//   * the seed mixer, batch planner, backend selector, convergence check and
//     validation error messages.
// Wall-clock timings are not compared.
//
// Usage: risk_golden <scratch dir>   (writes small CSV fixtures there)

#include <cstdio>
#include <exception>
#include <filesystem>
#include <fstream>
#include <string>
#include <vector>

#include "quantforge/quantforge.hpp"
#include "core/rng_manager.hpp"
#include "cpu/reference_kernels.hpp"
#include "execution/backend_selector.hpp"
#include "execution/batch_planner.hpp"
#include "risk/convergence.hpp"

using namespace quantforge;
namespace fs = std::filesystem;

namespace {

void metrics(const char* label, const std::vector<double>& v, double c) {
  std::printf("%s c=%.17g var %.17g cvar %.17g vol %.17g dd %.17g semi %.17g", label, c, compute(VaR{}, v, c),
              compute(CVaR{}, v, c), compute(Volatility{}, v, c), compute(DownsideDeviation{}, v, c),
              compute(SemiDeviation{}, v, c));
  if (v.size() >= 3) std::printf(" skew %.17g", compute(Skewness{}, v, c));
  if (v.size() >= 4) std::printf(" kurt %.17g", compute(Kurtosis{}, v, c));
  std::printf("\n");
}

template <typename Metric>
void engine_run(const char* label, const Portfolio& p, RiskConfig cfg) {
  try {
    RiskEngine engine;
    const RiskReport r = engine.calculate<Metric>(p, cfg);
    std::printf("engine %s %s value %.17g sims %llu seed %llu backend %s conf %.17g\n", label, r.metric_name.c_str(),
                r.value(), static_cast<unsigned long long>(r.simulations_run),
                static_cast<unsigned long long>(r.seed), to_string(r.backend_used).c_str(), r.confidence);
  } catch (const std::exception& e) {
    std::printf("engine %s error: %s\n", label, e.what());
  }
}

}  // namespace

int main(int argc, char** argv) {
  if (argc < 2) {
    std::fprintf(stderr, "usage: risk_golden <scratch dir>\n");
    return 2;
  }
  const fs::path dir = argv[1];
  fs::create_directories(dir);

  // 1. Seed mixer.
  for (std::uint64_t base : {0ULL, 42ULL, 7ULL, 0xFFFFFFFFFFFFFFFFULL}) {
    core::RngManager m(base);
    std::printf("seed %llu:", static_cast<unsigned long long>(base));
    for (std::uint64_t b = 0; b < 4; ++b) std::printf(" %llu", static_cast<unsigned long long>(m.seed_for_batch(b)));
    std::printf("\n");
  }

  // 2. The kernel, path by path.
  struct K { std::uint64_t seed, n; double drift, vol, init; };
  const std::vector<K> kernels = {
      {42, 9, 0.0, 0.20, 1'000'000.0}, {0, 5, 0.05, 0.35, 12'345.678}, {7, 4, -0.1, 0.0, 100.0},
      {123456789, 6, 0.0, 1.5, -5'000.0}, {1, 3, 0.0, 0.2, 0.0},
  };
  for (const K& k : kernels) {
    SimulationTask t;
    t.num_paths = k.n; t.seed = k.seed; t.drift = k.drift; t.volatility = k.vol; t.initial_value = k.init;
    const SimulationResult r = cpu::run_monte_carlo_reference(t);
    std::printf("kernel seed %llu:", static_cast<unsigned long long>(k.seed));
    for (double l : r.terminal_losses) std::printf(" %.17g", l);
    std::printf("\n");
  }
  SimulationTask big;
  big.num_paths = 200'001; big.seed = 99; big.volatility = 0.25; big.initial_value = 2'500'000.0;
  const std::vector<double> big_losses = cpu::run_monte_carlo_reference(big).terminal_losses;
  std::printf("big:");
  for (std::size_t i = 0; i < big_losses.size(); i += 9'973) std::printf(" %.17g", big_losses[i]);
  std::printf(" last %.17g\n", big_losses.back());

  // 3. Metrics.
  std::vector<double> ramp;
  for (int i = 0; i < 100; ++i) ramp.push_back(static_cast<double>(i));
  const std::vector<double> small = {3.5, -1.25, 7.0, 0.0, 2.0};
  const std::vector<double> ties = {1.0, 1.0, 1.0, 2.0, 2.0, 5.0};
  const std::vector<double> one = {4.0};
  const std::vector<double> flat = {2.0, 2.0, 2.0, 2.0};
  for (double c : {0.5, 0.9, 0.95, 0.99, 0.999, 0.01}) {
    metrics("ramp", ramp, c);
    metrics("small", small, c);
    metrics("ties", ties, c);
    metrics("big", big_losses, c);
  }
  metrics("one", one, 0.99);
  metrics("flat", flat, 0.99);
  try {
    static_cast<void>(compute(VaR{}, std::vector<double>{}, 0.99));
  } catch (const std::exception& e) {
    std::printf("empty var: %s\n", e.what());
  }
  try {
    static_cast<void>(compute(Skewness{}, std::vector<double>{1, 2}, 0.99));
  } catch (const std::exception& e) {
    std::printf("short skew: %s\n", e.what());
  }

  // 4. Convergence.
  for (double target : {0.001, 0.01, 0.05}) {
    for (std::size_t n : {std::size_t{150}, std::size_t{5'000}, std::size_t{200'001}}) {
      const std::vector<double> v(big_losses.begin(), big_losses.begin() + static_cast<long>(n));
      const auto cr = risk::check_convergence(v, 0.99, target);
      std::printf("convergence n=%zu target=%.17g converged %d rse %.17g\n", n, target, cr.converged,
                  cr.relative_std_error);
    }
  }
  {
    const auto cr = risk::check_convergence(std::vector<double>(500, 3.0), 0.95, 0.01);
    std::printf("convergence flat converged %d rse %.17g\n", cr.converged, cr.relative_std_error);
  }

  // 5. Batch planner and backend selector.
  execution::BatchPlanner planner;
  for (auto [total, cap] : std::vector<std::pair<std::uint64_t, std::uint64_t>>{
           {10, 0}, {10, 3}, {9, 3}, {5, 10}, {2'000'001, 2'000'000}}) {
    std::printf("plan %llu %llu:", static_cast<unsigned long long>(total), static_cast<unsigned long long>(cap));
    for (std::uint64_t b : planner.plan(total, cap)) std::printf(" %llu", static_cast<unsigned long long>(b));
    std::printf("\n");
  }
  execution::BackendSelector selector;
  for (std::uint64_t n : {1ULL, 49'999ULL, 50'000ULL, 999'999ULL, 1'000'000ULL}) {
    execution::WorkloadProfile prof{n, 3, n * 8, false};
    std::printf("select %llu auto %s cpu %s\n", static_cast<unsigned long long>(n),
                to_string(selector.select(Backend::Auto, prof)).c_str(),
                to_string(selector.select(Backend::CPU, prof)).c_str());
  }

  // 6. Engine runs on the CPU backend (the trusted reference).
  Portfolio p;
  p.add_asset("AAPL", 1000, 190.0);
  p.add_asset("MSFT", 500, 420.0);
  p.add_asset("GOOGL", 300, 175.0);
  p.add_asset("NVDA", 200, 130.0);
  Portfolio single;
  single.add_asset("SYN", 1, 100.0);
  RiskConfig cfg;
  cfg.backend = Backend::CPU;
  cfg.simulations = 20'000;
  engine_run<VaR>("p99", p, cfg);
  engine_run<CVaR>("p99", p, cfg);
  engine_run<Volatility>("p99", p, cfg);
  engine_run<DownsideDeviation>("p99", p, cfg);
  engine_run<Skewness>("p99", p, cfg);
  engine_run<Kurtosis>("p99", p, cfg);
  cfg.confidence = 0.95;
  cfg.seed = 7;
  cfg.simulations = 33'333;
  engine_run<VaR>("p95", p, cfg);
  engine_run<CVaR>("p95", p, cfg);
  cfg.simulations = 1;
  engine_run<VaR>("one", single, cfg);
  cfg.simulations = 49'999;
  cfg.backend = Backend::Auto;
  engine_run<VaR>("auto", single, cfg);

  // 7. Validation errors.
  RiskConfig bad;
  bad.backend = Backend::CPU;
  engine_run<VaR>("empty", Portfolio{}, bad);
  bad.confidence = 1.5;
  engine_run<VaR>("conf", single, bad);
  bad.confidence = 0.99;
  bad.simulations = 0;
  engine_run<VaR>("sims", single, bad);
  bad.simulations = 10;
  bad.adaptive = true;
  bad.target_rel_std_err = 0.0;
  engine_run<VaR>("adaptive", single, bad);
  bad.adaptive = false;
  bad.verify_against_cpu = true;
  bad.verify_tolerance = -1.0;
  engine_run<VaR>("verify", single, bad);
  Portfolio neg;
  neg.add_asset("X", 1, -2.0);
  engine_run<VaR>("negprice", neg, RiskConfig{});
  Portfolio nosym;
  nosym.add_asset("", 1, 2.0);
  engine_run<VaR>("nosym", nosym, RiskConfig{});

  // 8. CSV loading (well-formed files and the loader's own errors).
  const auto write = [&](const char* name, const char* text) {
    std::ofstream(dir / name, std::ios::binary) << text;
    return (dir / name).string();
  };
  const std::vector<std::string> files = {
      write("ok.csv", "symbol,quantity,price\nAAPL,1000,190.00\nMSFT,500,420.00\n\nGOOGL,300,175.00\n"),
      write("short.csv", "symbol,quantity,price\nAAPL,-10,190.5\nBND,2000,72.25\n"),
      write("malformed.csv", "symbol,quantity,price\nAAPL,1000\n"),
      write("text.csv", "symbol,quantity,price\nAAPL,lots,190\n"),
  };
  for (const std::string& f : files) {
    try {
      const Portfolio q = Portfolio::from_csv(f);
      std::printf("csv %s: %zu assets value %.17g\n", fs::path(f).filename().c_str(), q.size(), q.total_value());
    } catch (const std::exception& e) {
      std::printf("csv %s error: %s\n", fs::path(f).filename().c_str(), e.what());
    }
  }
  try {
    static_cast<void>(Portfolio::from_csv((dir / "missing.csv").string()));
  } catch (const std::exception&) {
    std::printf("csv missing.csv error\n");
  }
  return 0;
}
