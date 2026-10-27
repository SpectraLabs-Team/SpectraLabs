import Link from 'next/link';
import { BookOpen, ChevronRight, Clock, Cpu, FlaskConical, Gauge, TrendingUp, Workflow } from 'lucide-react';
import type { ReactNode } from 'react';
import { Footer } from '@/components/landing/Footer';
import { Header } from '@/components/landing/Header';
import { CATEGORIES, lessonPath, lessonsIn, readingMinutes, type Category, type CategoryId, type Lesson, type Level } from '@/lib/learn';
import { cn } from '@/lib/utils';

export const CATEGORY_ICONS = { BookOpen, Cpu, FlaskConical, Gauge, Workflow, TrendingUp } satisfies Record<Category['icon'], unknown>;

export function CategoryIcon({ category, className }: { category: Category; className?: string }) {
  const Icon = CATEGORY_ICONS[category.icon];
  return <Icon className={className} aria-hidden />;
}

const LEVEL_TONE: Record<Level, string> = {
  Beginner: 'border-profit/30 bg-profit/10 text-profit',
  Intermediate: 'border-accent/30 bg-accent/10 text-accent',
  Advanced: 'border-violet/30 bg-violet/10 text-violet',
};

export function LevelBadge({ level, className }: { level: Level; className?: string }) {
  return <span className={cn('inline-flex items-center rounded-full border px-2 py-0.5 text-[10.5px] font-semibold uppercase tracking-wider', LEVEL_TONE[level], className)}>{level}</span>;
}

export function ReadingTime({ minutes, className }: { minutes: number; className?: string }) {
  return (
    <span className={cn('inline-flex items-center gap-1 text-[11.5px] text-ink/50', className)}>
      <Clock className="h-3 w-3" aria-hidden /> {minutes} min read
    </span>
  );
}

/** Page frame for every /learn page: the site header and footer around the content. */
export function LearnFrame({ children }: { children: ReactNode }) {
  return (
    <>
      <Header />
      <div className="relative min-h-screen">
        <div aria-hidden className="bg-dot-grid pointer-events-none absolute inset-x-0 top-0 h-[480px] opacity-50 mask-fade-b" />
        <main id="learn-main" className="relative mx-auto max-w-[1320px] px-4 pb-20 pt-24 sm:px-8">
          {children}
        </main>
      </div>
      <Footer />
    </>
  );
}

export function Breadcrumbs({ items }: { items: { label: string; href?: string }[] }) {
  return (
    <nav aria-label="Breadcrumb" className="text-[12.5px] text-ink/50">
      <ol className="flex flex-wrap items-center gap-1">
        {items.map((it, i) => (
          <li key={it.label} className="flex items-center gap-1">
            {i > 0 && <ChevronRight className="h-3 w-3 text-ink/30" aria-hidden />}
            {it.href ? (
              <Link href={it.href} className="transition hover:text-ink">
                {it.label}
              </Link>
            ) : (
              <span aria-current="page" className="text-ink/75">
                {it.label}
              </span>
            )}
          </li>
        ))}
      </ol>
    </nav>
  );
}

export function LessonCard({ lesson, showCategory = false }: { lesson: Lesson; showCategory?: boolean }) {
  const cat = CATEGORIES.find((c) => c.id === lesson.category);
  return (
    <Link
      href={lessonPath(lesson)}
      className="group flex h-full flex-col rounded-xl border border-ink/[0.08] bg-panel/60 p-4 transition hover:border-accent/35 hover:bg-accent/[0.03] focus-visible:border-accent/50"
    >
      <div className="flex flex-wrap items-center gap-2">
        <LevelBadge level={lesson.level} />
        <ReadingTime minutes={readingMinutes(lesson)} />
      </div>
      <h3 className="mt-2.5 text-[14.5px] font-semibold leading-snug text-ink group-hover:text-accent">{lesson.title}</h3>
      <p className="mt-1.5 line-clamp-3 text-[12.5px] leading-5 text-ink/60">{lesson.summary}</p>
      {showCategory && cat && <p className="mt-auto pt-3 font-mono text-[10px] uppercase tracking-[0.14em] text-ink/40">{cat.title}</p>}
    </Link>
  );
}

/** Left navigation: every category, with the current category's lessons expanded. */
export function LearnNav({ current, currentSlug }: { current?: CategoryId; currentSlug?: string }) {
  return (
    <nav aria-label="Learning Center" className="space-y-1 text-[13px]">
      <Link href="/learn" className={cn('mb-2 flex items-center gap-2 rounded-lg px-2.5 py-1.5 font-semibold transition', !current ? 'bg-accent/10 text-ink' : 'text-ink/70 hover:bg-ink/[0.04] hover:text-ink')}>
        Learning Center
      </Link>
      {CATEGORIES.map((c) => {
        const open = c.id === current;
        return (
          <div key={c.id}>
            <Link
              href={`/learn/${c.id}`}
              aria-current={open && !currentSlug ? 'page' : undefined}
              className={cn('flex items-center gap-2 rounded-lg px-2.5 py-1.5 transition', open ? 'text-ink' : 'text-ink/60 hover:bg-ink/[0.04] hover:text-ink', open && !currentSlug && 'bg-accent/10')}
            >
              <CategoryIcon category={c} className={cn('h-3.5 w-3.5 shrink-0', open ? 'text-accent' : 'text-ink/40')} />
              <span className="font-medium">{c.title}</span>
            </Link>
            {open && (
              <ul className="my-1 ml-[18px] space-y-px border-l border-ink/[0.08] pl-2">
                {lessonsIn(c.id).map((l) => {
                  const active = l.slug === currentSlug;
                  return (
                    <li key={l.slug}>
                      <Link
                        href={lessonPath(l)}
                        aria-current={active ? 'page' : undefined}
                        className={cn('block rounded-md px-2 py-1 text-[12.5px] leading-snug transition', active ? 'bg-accent/10 font-medium text-accent' : 'text-ink/55 hover:bg-ink/[0.04] hover:text-ink')}
                      >
                        {l.title}
                      </Link>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        );
      })}
    </nav>
  );
}

/** Desktop: sticky side nav. Mobile: a collapsible "Browse lessons" panel above the content. */
export function WithLearnNav({ current, currentSlug, children }: { current?: CategoryId; currentSlug?: string; children: ReactNode }) {
  return (
    <div className="lg:grid lg:grid-cols-[250px_minmax(0,1fr)] lg:gap-10">
      <aside className="hidden lg:block">
        <div className="sticky top-24 max-h-[calc(100vh-7rem)] overflow-y-auto pb-6 pr-1">
          <LearnNav current={current} currentSlug={currentSlug} />
        </div>
      </aside>
      <div className="min-w-0">
        <details className="mb-6 rounded-xl border border-ink/[0.08] bg-panel/60 lg:hidden">
          <summary className="cursor-pointer select-none px-4 py-3 text-sm font-medium text-ink">Browse lessons</summary>
          <div className="border-t border-ink/[0.06] px-2 py-3">
            <LearnNav current={current} currentSlug={currentSlug} />
          </div>
        </details>
        {children}
      </div>
    </div>
  );
}
