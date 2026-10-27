'use client';

import Link from 'next/link';
import { useEffect, useState } from 'react';
import { BookOpen, ChevronDown } from 'lucide-react';
import type { StudioTab } from './StudioShell';
import { WORKSPACES } from '@/lib/workspaces/registry';
import { cn } from '@/lib/utils';

const KEY = 'spectra-workspace-guide-open';

function readOpen(): Record<string, boolean> {
  try {
    return JSON.parse(localStorage.getItem(KEY) || '{}') as Record<string, boolean>;
  } catch {
    return {};
  }
}

/**
 * "How to use this workspace": purpose, steps, inputs, controls, outputs, pitfalls and lesson links,
 * from lib/workspaces/registry.ts. Collapsed by default; the open state is remembered per workspace
 * on this device (a convenience only, nothing depends on it).
 */
export function WorkspaceGuide({ tab }: { tab: StudioTab }) {
  const guide = WORKSPACES[tab];
  const [open, setOpen] = useState(false);
  useEffect(() => setOpen(!!readOpen()[tab]), [tab]);

  const toggle = () => {
    const next = !open;
    setOpen(next);
    try {
      localStorage.setItem(KEY, JSON.stringify({ ...readOpen(), [tab]: next }));
    } catch {
      /* not remembered */
    }
  };

  const section = (title: string, items: string[], ordered = false) => (
    <div>
      <h3 className="font-mono text-[10.5px] uppercase tracking-[0.16em] text-ink/45">{title}</h3>
      {ordered ? (
        <ol className="mt-2 list-decimal space-y-1.5 pl-5 text-[13px] leading-6 text-ink/75">
          {items.map((i) => (
            <li key={i}>{i}</li>
          ))}
        </ol>
      ) : (
        <ul className="mt-2 space-y-1.5 text-[13px] leading-6 text-ink/75">
          {items.map((i) => (
            <li key={i} className="flex gap-2">
              <span aria-hidden className="mt-2.5 h-1 w-1 shrink-0 rounded-full bg-accent/70" />
              <span>{i}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );

  return (
    <section aria-label="How to use this workspace" className="mb-6 rounded-xl border border-ink/[0.08] bg-ink/[0.02]">
      <button
        type="button"
        onClick={toggle}
        aria-expanded={open}
        aria-controls={`guide-${tab}`}
        className="flex w-full items-center gap-3 px-4 py-3 text-left"
      >
        <BookOpen className="h-4 w-4 shrink-0 text-accent" />
        <span className="min-w-0 flex-1">
          <span className="block text-[13px] font-semibold text-ink">How to use this workspace</span>
          <span className="block truncate text-[12px] text-ink/55">{guide.purpose}</span>
        </span>
        <ChevronDown className={cn('h-4 w-4 shrink-0 text-ink/50 transition-transform', open && 'rotate-180')} />
      </button>

      {open && (
        <div id={`guide-${tab}`} className="grid gap-6 border-t border-ink/[0.06] p-4 md:grid-cols-2">
          <div className="space-y-5">
            {section('Steps', guide.steps, true)}
            {section('What you provide', guide.inputs)}
            {section('Controls', guide.controls)}
          </div>
          <div className="space-y-5">
            {section('What you get', guide.outputs)}
            {section('Watch out for', guide.pitfalls)}
            <div>
              <h3 className="font-mono text-[10.5px] uppercase tracking-[0.16em] text-ink/45">Learn more</h3>
              <ul className="mt-2 flex flex-wrap gap-1.5">
                {guide.lessons.map((l) => (
                  <li key={l.href}>
                    <Link href={l.href} className="inline-flex rounded-md border border-ink/[0.08] bg-panel/60 px-2 py-1 text-[12px] text-ink/75 transition hover:border-accent/40 hover:text-accent">
                      {l.title}
                    </Link>
                  </li>
                ))}
              </ul>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
