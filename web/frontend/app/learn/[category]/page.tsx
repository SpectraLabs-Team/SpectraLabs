import type { Metadata } from 'next';
import { notFound } from 'next/navigation';
import { Breadcrumbs, CategoryIcon, LearnFrame, LessonCard, WithLearnNav } from '@/components/learn/LearnChrome';
import { CATEGORIES, getCategory, lessonsIn, type Level } from '@/lib/learn';

type Props = { params: Promise<{ category: string }> };

export const dynamicParams = false;

export function generateStaticParams() {
  return CATEGORIES.map((c) => ({ category: c.id }));
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { category } = await params;
  const c = getCategory(category);
  return c ? { title: `${c.title} · Learning Center`, description: c.description } : {};
}

const LEVELS: Level[] = ['Beginner', 'Intermediate', 'Advanced'];

export default async function CategoryPage({ params }: Props) {
  const { category } = await params;
  const c = getCategory(category);
  if (!c) notFound();
  const lessons = lessonsIn(c.id);

  return (
    <LearnFrame>
      <WithLearnNav current={c.id}>
        <Breadcrumbs items={[{ label: 'Learn', href: '/learn' }, { label: c.title }]} />
        <header className="mt-4 flex items-start gap-4">
          <span className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl border border-accent/25 bg-accent/10 text-accent">
            <CategoryIcon category={c} className="h-5 w-5" />
          </span>
          <div>
            <h1 className="text-2xl font-bold tracking-tight text-ink sm:text-3xl">{c.title}</h1>
            <p className="mt-1.5 max-w-2xl text-[14.5px] leading-6 text-ink/60">{c.description}</p>
          </div>
        </header>
        {LEVELS.map((level) => {
          const at = lessons.filter((l) => l.level === level);
          if (!at.length) return null;
          return (
            <section key={level} aria-labelledby={`level-${level}`} className="mt-10">
              <h2 id={`level-${level}`} className="font-mono text-[11px] uppercase tracking-[0.16em] text-ink/45">
                {level} · {at.length}
              </h2>
              <div className="mt-3 grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
                {at.map((l) => (
                  <LessonCard key={l.slug} lesson={l} />
                ))}
              </div>
            </section>
          );
        })}
      </WithLearnNav>
    </LearnFrame>
  );
}
