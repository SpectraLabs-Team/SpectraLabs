import type { Metadata } from 'next';
import Link from 'next/link';
import { ArrowRight, Gauge } from 'lucide-react';
import { CategoryIcon, LearnFrame, LessonCard, WithLearnNav } from '@/components/learn/LearnChrome';
import { LearnSearch } from '@/components/learn/LearnSearch';
import { TourLauncher } from '@/components/onboarding/TourLauncher';
import { CATEGORIES, LESSONS, lessonsIn, searchIndex } from '@/lib/learn';
import { METRIC_CATEGORY_LABELS, metricDefinitions, type MetricCategory } from '@/lib/learn/metrics';

export const metadata: Metadata = {
  title: 'Learning Center',
  description: 'Learn trading fundamentals, how the Spectra engine and simulations work, and what every performance metric in your results means.',
};

/** The metric library: every metric shown in the Studio, grouped, linking to its lesson. */
function metricLibrary() {
  const groups = new Map<MetricCategory, { id: string; name: string; href: string }[]>();
  const seen = new Set<string>();
  for (const [id, d] of Object.entries(metricDefinitions)) {
    const key = `${d.category}:${d.name}`;
    if (seen.has(key)) continue;
    seen.add(key);
    const list = groups.get(d.category) ?? [];
    list.push({ id, name: d.name, href: d.learnPath });
    groups.set(d.category, list);
  }
  return [...groups.entries()];
}

export default function LearnHome() {
  const featured = LESSONS.filter((l) => l.featured).slice(0, 6);
  const beginner = LESSONS.filter((l) => l.level === 'Beginner' && ['fundamentals', 'simulations', 'paper-trading'].includes(l.category)).slice(0, 8);
  const index = searchIndex();

  return (
    <LearnFrame>
      <WithLearnNav>
      <section className="mx-auto max-w-3xl pt-6 text-center">
        <p className="eyebrow">Learning Center</p>
        <h1 className="mt-3 text-balance text-3xl font-bold tracking-tight text-ink sm:text-[40px] sm:leading-[1.15]">Learn Trading &amp; Understand Your Results</h1>
        <p className="mx-auto mt-4 max-w-2xl text-[15px] leading-7 text-ink/65">
          {LESSONS.length} lessons on trading mechanics, the Spectra engine, simulations and strategies, plus an explanation of every metric in your backtest, risk and paper-trading results.
        </p>
        <div className="mx-auto mt-7 max-w-2xl text-left">
          <LearnSearch index={index} />
        </div>
        <div className="mt-4 flex flex-wrap items-center justify-center gap-3 text-[13px]">
          <TourLauncher />
          <Link href="/learn/metrics" className="btn-ghost text-[13px]">
            <Gauge className="h-3.5 w-3.5" /> Metric library
          </Link>
        </div>
      </section>

      <section aria-labelledby="featured" className="mt-16">
        <h2 id="featured" className="text-lg font-semibold text-ink">
          Featured topics
        </h2>
        <div className="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {featured.map((l) => (
            <LessonCard key={`${l.category}/${l.slug}`} lesson={l} showCategory />
          ))}
        </div>
      </section>

      <section aria-labelledby="categories" className="mt-14">
        <h2 id="categories" className="text-lg font-semibold text-ink">
          Topics
        </h2>
        <div className="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {CATEGORIES.map((c) => (
            <Link key={c.id} href={`/learn/${c.id}`} className="group glass flex flex-col p-5 transition hover:border-accent/35">
              <span className="flex h-9 w-9 items-center justify-center rounded-lg border border-accent/25 bg-accent/10 text-accent">
                <CategoryIcon category={c} className="h-4 w-4" />
              </span>
              <h3 className="mt-3 text-[15px] font-semibold text-ink group-hover:text-accent">{c.title}</h3>
              <p className="mt-1 text-[13px] leading-5 text-ink/60">{c.description}</p>
              <p className="mt-auto flex items-center gap-1 pt-4 text-[12px] font-medium text-ink/50 group-hover:text-accent">
                {lessonsIn(c.id).length} lessons <ArrowRight className="h-3 w-3" />
              </p>
            </Link>
          ))}
        </div>
      </section>

      <section aria-labelledby="beginner" className="mt-14">
        <div className="flex flex-wrap items-baseline justify-between gap-2">
          <h2 id="beginner" className="text-lg font-semibold text-ink">
            Recommended for beginners
          </h2>
          <p className="text-[12.5px] text-ink/50">Start here if you are new to trading or to simulations.</p>
        </div>
        <ol className="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
          {beginner.map((l) => (
            <li key={`${l.category}/${l.slug}`}>
              <LessonCard lesson={l} showCategory />
            </li>
          ))}
        </ol>
      </section>

      <section aria-labelledby="metrics" className="mt-14">
        <div className="flex flex-wrap items-baseline justify-between gap-2">
          <h2 id="metrics" className="text-lg font-semibold text-ink">
            Metric explanation library
          </h2>
          <p className="text-[12.5px] text-ink/50">Every metric with an ⓘ in the Studio links to one of these.</p>
        </div>
        <div className="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {metricLibrary().map(([cat, items]) => (
            <div key={cat} className="glass-subtle p-4">
              <h3 className="font-mono text-[10.5px] uppercase tracking-[0.16em] text-ink/45">{METRIC_CATEGORY_LABELS[cat]}</h3>
              <ul className="mt-2.5 flex flex-wrap gap-1.5">
                {items.map((m) => (
                  <li key={m.id}>
                    <Link href={m.href} className="inline-flex rounded-md border border-ink/[0.08] bg-panel/60 px-2 py-1 text-[12px] text-ink/75 transition hover:border-accent/40 hover:text-accent">
                      {m.name}
                    </Link>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      </section>
      </WithLearnNav>
    </LearnFrame>
  );
}
