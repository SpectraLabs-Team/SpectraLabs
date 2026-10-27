import type { Metadata } from 'next';
import Link from 'next/link';
import { notFound } from 'next/navigation';
import { ArrowLeft, ArrowRight, ExternalLink } from 'lucide-react';
import { Breadcrumbs, LearnFrame, LessonCard, LevelBadge, ReadingTime, WithLearnNav } from '@/components/learn/LearnChrome';
import { LessonBody } from '@/components/learn/LessonBody';
import { RichText } from '@/components/learn/RichText';
import { LESSONS, getCategory, getLesson, lessonPath, neighbours, readingMinutes, relatedLessons } from '@/lib/learn';

type Props = { params: Promise<{ category: string; slug: string }> };

export const dynamicParams = false;

export function generateStaticParams() {
  return LESSONS.map((l) => ({ category: l.category, slug: l.slug }));
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { category, slug } = await params;
  const l = getLesson(category, slug);
  return l ? { title: `${l.title} · Learning Center`, description: l.summary } : {};
}

export default async function LessonPage({ params }: Props) {
  const { category, slug } = await params;
  const lesson = getLesson(category, slug);
  const cat = getCategory(category);
  if (!lesson || !cat) notFound();
  const { prev, next } = neighbours(lesson);
  const related = relatedLessons(lesson);
  const isMetric = lesson.category === 'metrics';

  return (
    <LearnFrame>
      <WithLearnNav current={cat.id} currentSlug={lesson.slug}>
        <article className="max-w-3xl" aria-labelledby="lesson-title">
          <Breadcrumbs items={[{ label: 'Learn', href: '/learn' }, { label: cat.title, href: `/learn/${cat.id}` }, { label: lesson.title }]} />
          <header className="mt-4">
            <h1 id="lesson-title" className="text-balance text-[28px] font-bold leading-tight tracking-tight text-ink sm:text-[34px]">
              {lesson.title}
            </h1>
            <div className="mt-3 flex flex-wrap items-center gap-3">
              <LevelBadge level={lesson.level} />
              <ReadingTime minutes={readingMinutes(lesson)} />
              <span className="font-mono text-[10.5px] uppercase tracking-[0.14em] text-ink/40">{cat.title}</span>
            </div>
            <p className="mt-5 text-[16.5px] leading-7 text-ink/75">
              <RichText text={lesson.summary} />
            </p>
          </header>

          <LessonBody blocks={lesson.blocks} />

          {lesson.platformLinks && lesson.platformLinks.length > 0 && (
            <section aria-labelledby="try-it" className="mt-12 rounded-xl border border-accent/25 bg-accent/[0.04] p-5">
              <h2 id="try-it" className="text-sm font-semibold text-ink">
                Try it in Spectra
              </h2>
              <div className="mt-3 flex flex-wrap gap-2">
                {lesson.platformLinks.map((p) => (
                  <Link key={p.href} href={p.href} className="btn-secondary px-3 py-1.5 text-[13px]">
                    {p.label} <ExternalLink className="h-3.5 w-3.5 text-ink/45" aria-hidden />
                  </Link>
                ))}
              </div>
            </section>
          )}

          {related.length > 0 && (
            <section aria-labelledby="related" className="mt-12">
              <h2 id="related" className="text-lg font-semibold text-ink">
                {isMetric ? 'Related metrics & lessons' : 'Related lessons'}
              </h2>
              <div className="mt-4 grid gap-3 sm:grid-cols-2">
                {related.map((r) => (
                  <LessonCard key={`${r.category}/${r.slug}`} lesson={r} showCategory={r.category !== lesson.category} />
                ))}
              </div>
            </section>
          )}

          <nav aria-label="Previous and next lesson" className="mt-12 grid gap-3 border-t border-ink/[0.07] pt-6 sm:grid-cols-2">
            {prev ? (
              <Link href={lessonPath(prev)} rel="prev" className="group rounded-xl border border-ink/[0.08] p-4 transition hover:border-accent/35">
                <span className="flex items-center gap-1 text-[11.5px] text-ink/45">
                  <ArrowLeft className="h-3 w-3" aria-hidden /> Previous
                </span>
                <span className="mt-1 block text-[14px] font-medium text-ink group-hover:text-accent">{prev.title}</span>
              </Link>
            ) : (
              <span />
            )}
            {next && (
              <Link href={lessonPath(next)} rel="next" className="group rounded-xl border border-ink/[0.08] p-4 text-right transition hover:border-accent/35">
                <span className="flex items-center justify-end gap-1 text-[11.5px] text-ink/45">
                  Next <ArrowRight className="h-3 w-3" aria-hidden />
                </span>
                <span className="mt-1 block text-[14px] font-medium text-ink group-hover:text-accent">{next.title}</span>
              </Link>
            )}
          </nav>
        </article>
      </WithLearnNav>
    </LearnFrame>
  );
}
