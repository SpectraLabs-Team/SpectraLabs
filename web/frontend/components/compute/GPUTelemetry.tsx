"use client";

import { useEffect, useState } from 'react';
import { Activity, Cpu, MemoryStick, Thermometer } from 'lucide-react';
import { getGpuTelemetry } from '@/lib/api';

type Gpu = Awaited<ReturnType<typeof getGpuTelemetry>>;

export default function GPUTelemetry() {
  const [gpu, setGpu] = useState<Gpu | null>(null);
  const [error, setError] = useState('');
  useEffect(() => {
    let active = true;
    const refresh = () => getGpuTelemetry().then((data) => { if (active) { setGpu(data); setError(''); } }).catch((e: Error) => { if (active) setError(e.message); });
    refresh();
    const timer = window.setInterval(refresh, 5000);
    return () => { active = false; window.clearInterval(timer); };
  }, []);
  const available = gpu?.available ?? false;
  const stats = [
    { title: 'GPU utilization', value: available ? `${gpu!.utilization}%` : '—', icon: Activity },
    { title: 'GPU memory', value: available ? `${(gpu!.memory_used_mb / 1024).toFixed(1)} / ${(gpu!.memory_total_mb / 1024).toFixed(1)} GB` : '—', icon: MemoryStick },
    { title: 'Temperature', value: available ? `${gpu!.temperature_c}°C` : '—', icon: Thermometer },
  ];
  return <section className="space-y-4">
    <div><h2 className="text-xl font-semibold">GPU telemetry</h2><p className="mt-1 text-sm text-foreground-muted">Hardware readings from NVIDIA system telemetry. This engine build has no CUDA backend, so the GPU is never used for simulation.</p></div>
    {error && <p role="status" className="rounded-lg border border-accent-warning/30 bg-accent-warning/10 p-3 text-sm text-foreground-muted">{error}</p>}
    {!error && gpu && !available && <p className="rounded-lg border border-border bg-surface p-4 text-sm text-foreground-muted">No NVIDIA GPU telemetry was detected on the API host.</p>}
    <div className="grid gap-3 md:grid-cols-3">{stats.map(({ title, value, icon: Icon }) => <article key={title} className="rounded-xl border border-border bg-surface p-5"><div className="flex items-center gap-2 text-sm text-foreground-muted"><Icon size={16} />{title}</div><p className="mt-4 text-2xl font-semibold tabular-nums">{value}</p></article>)}</div>
    <div className="flex items-center gap-2 text-xs text-foreground-subtle"><Cpu size={14} /> Hardware telemetry only · No kernel timeline or job counters are exposed by the API.</div>
  </section>;
}
