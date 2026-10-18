//! Host telemetry for the dashboard, read from `/proc` on Linux (the Python
//! backend used psutil). On other systems the endpoints report
//! `"source": "unavailable"` instead of guessing. These are host readings:
//! they say nothing about whether a Spectra job is running.

use std::time::Duration;

use spectra_core::Json;

fn num(v: f64) -> Json {
    Json::number(v).unwrap_or(Json::Null)
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// (busy, total) jiffies per line of /proc/stat: index 0 = all CPUs, then one per CPU.
fn cpu_times() -> Option<Vec<(u64, u64)>> {
    let text = std::fs::read_to_string("/proc/stat").ok()?;
    let rows: Vec<(u64, u64)> = text
        .lines()
        .filter(|l| l.starts_with("cpu"))
        .filter_map(|l| {
            let v: Vec<u64> = l.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
            if v.len() < 4 {
                return None;
            }
            let idle = v[3] + v.get(4).copied().unwrap_or(0); // idle + iowait
            let total: u64 = v.iter().take(8).sum(); // exclude guest columns (already in user)
            Some((total - idle, total))
        })
        .collect();
    (!rows.is_empty()).then_some(rows)
}

/// Utilization percentages over a short window: `[all, cpu0, cpu1, ...]`.
fn utilization(window: Duration) -> Option<Vec<f64>> {
    let a = cpu_times()?;
    std::thread::sleep(window);
    let b = cpu_times()?;
    Some(
        a.iter()
            .zip(&b)
            .map(|(x, y)| {
                let total = y.1.saturating_sub(x.1);
                if total == 0 {
                    0.0
                } else {
                    round2(100.0 * y.0.saturating_sub(x.0) as f64 / total as f64)
                }
            })
            .collect(),
    )
}

fn physical_cores() -> Option<usize> {
    let text = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    let mut seen = std::collections::BTreeSet::new();
    let (mut phys, mut core) = (None, None);
    for line in text.lines().chain(std::iter::once("")) {
        if line.trim().is_empty() {
            if let (Some(p), Some(c)) = (phys.take(), core.take()) {
                seen.insert((p, c));
            }
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            match k.trim() {
                "physical id" => phys = Some(v.trim().to_string()),
                "core id" => core = Some(v.trim().to_string()),
                _ => {}
            }
        }
    }
    (!seen.is_empty()).then_some(seen.len())
}

fn meminfo() -> Option<(f64, f64)> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let field = |name: &str| -> Option<f64> {
        text.lines().find(|l| l.starts_with(name))?.split_whitespace().nth(1)?.parse().ok()
    };
    let total = field("MemTotal:")?;
    let avail = field("MemAvailable:")?;
    Some((total * 1024.0, (total - avail) * 1024.0))
}

fn threads() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

/// `GET /api/telemetry/cpu`.
pub fn cpu() -> Json {
    let util = utilization(Duration::from_millis(100));
    let mem = meminfo();
    let t = threads();
    let gb = |b: f64| round2(b / 1024f64.powi(3));
    Json::object()
        .with("cpu_utilization", num(util.as_ref().map_or(0.0, |u| u[0])))
        .with("cpu_cores", Json::integer(physical_cores().unwrap_or(t) as i64))
        .with("cpu_threads", Json::integer(t as i64))
        .with("memory_utilization", num(mem.map_or(0.0, |(tot, used)| round2(100.0 * used / tot))))
        .with("memory_used_gb", num(mem.map_or(0.0, |(_, used)| gb(used))))
        .with("memory_total_gb", num(mem.map_or(0.0, |(tot, _)| gb(tot))))
        .with("source", Json::string(if util.is_some() { "host" } else { "unavailable" }))
}

/// `GET /api/telemetry/workers`: per-logical-CPU utilization.
pub fn workers() -> Json {
    let util = utilization(Duration::from_millis(100)).unwrap_or_default();
    let per: Vec<f64> = util.iter().skip(1).copied().collect();
    let avg = if per.is_empty() { 0.0 } else { round2(per.iter().sum::<f64>() / per.len() as f64) };
    Json::object()
        .with(
            "workers",
            Json::Array(
                per.iter()
                    .enumerate()
                    .map(|(i, u)| {
                        Json::object().with("worker_id", Json::integer(i as i64)).with("utilization", num(*u))
                    })
                    .collect(),
            ),
        )
        .with("total_workers", Json::integer(per.len() as i64))
        .with("average_utilization", num(avg))
        .with("source", Json::string(if per.is_empty() { "unavailable" } else { "host" }))
}

fn nvidia_smi() -> Option<Vec<i64>> {
    let mut child = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu,memory.used,memory.total,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if std::time::Instant::now() > deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    let v: Vec<i64> = out.lines().next()?.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (v.len() == 4).then_some(v)
}

/// `GET /api/telemetry/gpu`: hardware readings only. This build has no CUDA
/// backend, so the engine itself never uses the GPU.
pub fn gpu() -> Json {
    match nvidia_smi() {
        Some(v) => Json::object()
            .with("available", Json::boolean(true))
            .with("utilization", Json::integer(v[0]))
            .with("memory_used_mb", Json::integer(v[1]))
            .with("memory_total_mb", Json::integer(v[2]))
            .with("temperature_c", Json::integer(v[3]))
            .with("backend", Json::string("None"))
            .with("message", Json::string("GPU detected, but this engine build has no CUDA backend.")),
        None => Json::object()
            .with("available", Json::boolean(false))
            .with("utilization", Json::integer(0))
            .with("memory_used_mb", Json::integer(0))
            .with("memory_total_mb", Json::integer(0))
            .with("temperature_c", Json::integer(0))
            .with("backend", Json::string("None"))
            .with("message", Json::string("GPU not available, and this engine build has no CUDA backend.")),
    }
}
