'use client';

import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { Search, X } from 'lucide-react';
import type { SearchEntry } from '@/lib/learn';
import { searchLessons } from '@/lib/learn/search';
import { cn } from '@/lib/utils';

/** Search box over every lesson. Press "/" anywhere on the page to focus it. */
export function LearnSearch({ index, autoFocus = false }: { index: SearchEntry[]; autoFocus?: boolean }) {
  const router = useRouter();
  const [q, setQ] = useState('');
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listId = useId();
  const results = useMemo(() => searchLessons(index, q, 12), [index, q]);
  const open = q.trim().length > 0;

  useEffect(() => setActive(0), [q]);
  useEffect(() => {
    if (autoFocus) inputRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      const typing = t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable);
      if (e.key === '/' && !typing) {
        e.preventDefault();
        inputRef.current?.focus();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [autoFocus]);

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (!open) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setActive((a) => Math.min(a + 1, results.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setActive((a) => Math.max(a - 1, 0));
    } else if (e.key === 'Enter' && results[active]) {
      e.preventDefault();
      router.push(results[active].href);
    } else if (e.key === 'Escape') {
      setQ('');
    }
  };

  return (
    <div className="relative">
      <div className="relative">
        <Search className="pointer-events-none absolute left-4 top-1/2 h-4 w-4 -translate-y-1/2 text-ink/40" aria-hidden />
        <input
          ref={inputRef}
          type="search"
          role="combobox"
          aria-expanded={open}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={open && results[active] ? `${listId}-${active}` : undefined}
          aria-label="Search lessons"
          placeholder="Search lessons, metrics and concepts (e.g. Sharpe, slippage, limit order)"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={onKeyDown}
          className="h-12 w-full rounded-xl border border-ink/12 bg-panel/80 pl-11 pr-20 text-[14.5px] text-ink shadow-panel placeholder:text-ink/40 focus:border-accent/60 focus:outline-none focus:ring-2 focus:ring-accent/20 [&::-webkit-search-cancel-button]:hidden"
        />
        {q ? (
          <button type="button" onClick={() => setQ('')} className="absolute right-3 top-1/2 -translate-y-1/2 rounded-md p-1 text-ink/45 hover:text-ink" aria-label="Clear search">
            <X className="h-4 w-4" />
          </button>
        ) : (
          <kbd className="kbd absolute right-3.5 top-1/2 hidden -translate-y-1/2 sm:inline">/</kbd>
        )}
      </div>
      {open && (
        <div className="absolute inset-x-0 top-full z-30 mt-2 overflow-hidden rounded-xl border border-ink/10 bg-panel shadow-2xl">
          <p className="sr-only" role="status" aria-live="polite">
            {results.length} result{results.length === 1 ? '' : 's'}
          </p>
          {results.length === 0 ? (
            <p className="px-4 py-4 text-sm text-ink/55">No lessons match &ldquo;{q}&rdquo;. Try a broader term such as &ldquo;risk&rdquo; or &ldquo;orders&rdquo;.</p>
          ) : (
            <ul id={listId} role="listbox" aria-label="Matching lessons" className="max-h-[420px] overflow-y-auto py-1">
              {results.map((r, i) => (
                <li key={r.key} id={`${listId}-${i}`} role="option" aria-selected={i === active}>
                  <Link
                    href={r.href}
                    onMouseEnter={() => setActive(i)}
                    className={cn('block px-4 py-2.5 transition', i === active ? 'bg-accent/10' : 'hover:bg-ink/[0.04]')}
                  >
                    <span className="flex flex-wrap items-baseline gap-x-2">
                      <span className="text-[14px] font-medium text-ink">{r.title}</span>
                      <span className="font-mono text-[10px] uppercase tracking-wider text-ink/40">{r.categoryTitle}</span>
                    </span>
                    <span className="mt-0.5 block truncate text-[12px] text-ink/55">{r.summary}</span>
                  </Link>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
