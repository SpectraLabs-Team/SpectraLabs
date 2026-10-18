"use client";

import { useEffect, useState } from 'react';
import { Activity, Cpu, MemoryStick } from 'lucide-react';
import { getCpuTelemetry, getWorkerTelemetry } from '@/lib/api';

type Telemetry = Awaited<ReturnType<typeof getCpuTelemetry>>;
type Workers = Awaited<ReturnType<typeof getWorkerTelemetry>>;

export default function OpenMPTelemetry() {
  const [host, setHost] = useState<Telemetry | null>(null);
  const [workers, setWorkers] = useState<Workers | null>(null);
  const [error, setError] = useState('');
  useEffect(() => {
    let active = true;
    const refresh = async () => {
      try {
        const [cpu, workerData] = await Promise.all([getCpuTelemetry(), getWorkerTelemetry()]);
        if (active) { setHost(cpu); setWorkers(workerData); setError(''); }
      } catch (e) { if (active) setError(e instanceof Error ? e.message : 'Host telemetry unavailable'); }
    };
    refresh();
    const timer = window.setInterval(refresh, 5000);
    return () => { active = false; window.clearInterval(timer); };
  }, []);
  const stats = [
    { title: 'Host CPU utilization', value: host ? `${host.cpu_utilization.toFixed(1)}%` : '—', detail: host ? `${host.cpu_cores} physical cores · ${host.cpu_threads} threads` : 'Waiting for host telemetry', icon: Cpu },
    { title: 'Host memory', value: host ? `${host.memory_utilization.toFixed(1)}%` : '—', detail: host ? `${host.memory_used_gb} of ${host.memory_total_gb} GB used` : 'Waiting for host telemetry', icon: MemoryStick },
    { title: 'Worker samples', value: workers ? String(workers.total_workers) : '—', detail: 'Per-core host utilization; not simulation progress', icon: Activity },
  ];
  return <section className="space-y-4">
    <div><h2 className="text-xl font-semibold">Host telemetry</h2><p className="mt-1 text-sm text-foreground-muted">System load reported by the API host. This does not indicate active Spectra jobs.</p></div>
    {error && <p role="status" className="rounded-lg border border-accent-warning/30 bg-accent-warning/10 p-3 text-sm text-foreground-muted">{error}</p>}
    <div className="grid gap-3 md:grid-cols-3">{stats.map(({ title, value, detail, icon: Icon }) => <article key={title} className="rounded-xl border border-border bg-surface p-5"><div className="flex items-center gap-2 text-sm text-foreground-muted"><Icon size={16} />{title}</div><p className="mt-4 text-2xl font-semibold tabular-nums">{value}</p><p className="mt-1 text-xs text-foreground-subtle">{detail}</p></article>)}</div>
    {workers && <div className="rounded-xl border border-border bg-surface p-5"><h3 className="mb-4 text-sm font-medium">Per-core host utilization</h3><div className="space-y-2">{workers.workers.map((worker) => <div key={worker.worker_id} className="flex items-center gap-3 text-xs"><span className="w-16 text-foreground-subtle">Core {worker.worker_id + 1}</span><div className="h-2 flex-1 overflow-hidden rounded-full bg-background"><div className="h-full rounded-full bg-accent-primary" style={{ width: `${Math.min(100, Math.max(0, worker.utilization))}%` }} /></div><span className="w-12 text-right tabular-nums text-foreground-muted">{worker.utilization.toFixed(1)}%</span></div>)}</div></div>}
  </section>;
}
