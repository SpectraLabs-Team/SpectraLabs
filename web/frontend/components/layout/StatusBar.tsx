"use client";

import { useEffect, useState } from 'react';
import { Activity, HelpCircle } from 'lucide-react';
import { getHealth } from '@/lib/api';

export default function StatusBar() {
  const [api, setApi] = useState<'checking' | 'online' | 'offline'>('checking');
  const [engine, setEngine] = useState<boolean | null>(null);
  useEffect(() => {
    let active = true;
    const refresh = () => getHealth().then((health) => {
      if (!active) return;
      setApi(health.status === 'healthy' ? 'online' : 'offline');
      setEngine(health.cli_exists);
    }).catch(() => { if (active) { setApi('offline'); setEngine(false); } });
    refresh();
    const timer = window.setInterval(refresh, 30000);
    return () => { active = false; window.clearInterval(timer); };
  }, []);

  const online = api === 'online';
  return <div className="flex min-h-12 items-center justify-between gap-4 border-b border-border bg-surface px-4 py-2">
    <div className="flex items-center gap-2 text-sm"><span className={`h-2 w-2 rounded-full ${api === 'checking' ? 'bg-foreground-subtle' : online ? 'bg-accent-success' : 'bg-accent-danger'}`} /><span className="font-medium">API {api}</span></div>
    <div className="flex items-center gap-2 text-xs text-foreground-muted"><Activity size={14} /><span>Native engine {engine === null ? 'checking' : engine ? 'available' : 'unavailable'}</span></div>
    <div className="hidden items-center gap-1.5 text-xs text-foreground-subtle sm:flex"><HelpCircle size={14} /><span>Provider status not configured</span></div>
  </div>;
}
