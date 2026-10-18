// Golden-output generator for the Data Studio: runs every case in parity/data
// through the FROZEN C++ reference (quantforge v0.6.0) and prints what it
// produced. The Rust port must print the same (crates/spectra-data/tests/parity_data.rs).
//
//   data_golden <parity/data dir> <out dir>
// writes <out dir>/data.golden and, for every successfully loaded case, the
// C++ store files <out dir>/<case>.qfb and <case>.meta.json.

#include <algorithm>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <map>
#include <sstream>
#include <string>
#include <vector>

#include "quantforge/data.hpp"
#include "quantforge/datastore.hpp"

using namespace qf;
namespace fs = std::filesystem;

namespace {

std::map<std::string, std::string> read_opts(const fs::path& p) {
  std::map<std::string, std::string> m;
  std::ifstream in(p);
  std::string line;
  while (std::getline(in, line)) {
    const auto eq = line.find('=');
    if (eq != std::string::npos) m[line.substr(0, eq)] = line.substr(eq + 1);
  }
  return m;
}

TimeFormat tf_of(const std::string& s) {
  if (s == "s") return TimeFormat::EpochSeconds;
  if (s == "ms") return TimeFormat::EpochMillis;
  if (s == "us") return TimeFormat::EpochMicros;
  if (s == "ns") return TimeFormat::EpochNanos;
  return TimeFormat::Iso8601;
}

}  // namespace

int main(int argc, char** argv) {
  if (argc != 3) {
    std::cerr << "usage: data_golden <cases dir> <out dir>\n";
    return 2;
  }
  const fs::path dir = argv[1];
  const fs::path out_dir = argv[2];
  fs::create_directories(out_dir);
  std::ofstream out(out_dir / "data.golden", std::ios::binary);

  std::vector<std::string> names;
  for (const auto& e : fs::directory_iterator(dir)) {
    if (e.path().extension() == ".csv") names.push_back(e.path().stem().string());
  }
  std::sort(names.begin(), names.end());

  const fs::path tmp = fs::temp_directory_path() / "qf_data_golden_store";
  fs::remove_all(tmp);
  DataStore store(tmp);

  for (const std::string& name : names) {
    const auto o = read_opts(dir / (name + ".opts"));
    CsvOptions opt;
    opt.on_invalid = o.at("skip") == "1" ? InvalidRowPolicy::Skip : InvalidRowPolicy::Reject;
    opt.delimiter = o.at("delim") == "TAB" ? '\t' : o.at("delim")[0];
    opt.time.naive_utc_offset_minutes = std::stoi(o.at("offset"));
    opt.time.format = tf_of(o.at("tf"));
    opt.default_symbol = o.at("symbol");
    opt.timestamp_column = o.at("ts");
    opt.open_column = o.at("open");
    opt.high_column = o.at("high");
    opt.low_column = o.at("low");
    opt.close_column = o.at("close");
    opt.volume_column = o.at("volume");
    opt.symbol_column = o.at("sym");
    out << "=== " << name << "\n";
    try {
      const Dataset d = load_csv(read_file(dir / (name + ".csv")), opt);
      out << format_quality_report(d);
      out << "source " << d.info.source_sha256 << "\n";
      for (std::size_t i = 0; i < d.bars.size() && i < 5; ++i) {
        const Bar& b = d.bars[i];
        out << "bar " << b.ts_ns << ' ' << b.instrument << ' ' << b.open.raw() << ' ' << b.high.raw() << ' '
            << b.low.raw() << ' ' << b.close.raw() << ' ' << b.volume.raw() << "\n";
      }
      store.put(d);
      fs::copy_file(tmp / d.info.id / "bars.qfb", out_dir / (name + ".qfb"), fs::copy_options::overwrite_existing);
      fs::copy_file(tmp / d.info.id / "meta.json", out_dir / (name + ".meta.json"),
                    fs::copy_options::overwrite_existing);
      const Dataset back = store.get(d.info.id);
      out << "reloaded " << back.info.id << ' ' << back.quality.rows_total << ' ' << back.quality.rows_rejected << "\n";
      out << format_quality_report(back);
    } catch (const DataError& e) {
      out << "ERROR " << e.what() << "\n";
    }
  }

  std::ifstream ff(dir / "parse_fixed.txt");
  std::string line;
  while (std::getline(ff, line)) {
    const auto v = parse_fixed(line);
    out << "F [" << line << "] " << (v ? std::to_string(*v) : std::string("none")) << "\n";
  }
  std::ifstream tf(dir / "parse_time.txt");
  const std::vector<std::pair<std::string, TimeFormat>> fmts = {{"iso", TimeFormat::Iso8601},
      {"s", TimeFormat::EpochSeconds}, {"ms", TimeFormat::EpochMillis}, {"us", TimeFormat::EpochMicros},
      {"ns", TimeFormat::EpochNanos}};
  while (std::getline(tf, line)) {
    for (const auto& [fname, fmt] : fmts) {
      for (const int off : {0, 330, -90}) {
        TimeOptions t;
        t.format = fmt;
        t.naive_utc_offset_minutes = off;
        const auto p = parse_timestamp(line, t);
        out << "T " << fname << ' ' << off << " [" << line << "] ";
        if (p) {
          out << p->ts_ns << (p->naive ? " naive" : " zoned") << ' ' << format_timestamp(p->ts_ns) << "\n";
        } else {
          out << "none\n";
        }
      }
    }
  }
  fs::remove_all(tmp);
  return 0;
}
