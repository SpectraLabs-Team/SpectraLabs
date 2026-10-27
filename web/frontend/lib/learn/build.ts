import type { Block, CategoryId, DiagramId, InteractiveId, Lesson, Level, PlatformLink } from './types';

/**
 * Every metric lesson has the same eight parts (name, explanation, why it matters, formula,
 * example, high/low interpretation, misconceptions, related metrics) plus a note on exactly how
 * the Spectra engine computes it. This builder turns that structure into lesson blocks so the
 * lessons stay consistent and none of the parts can be forgotten.
 */
export interface MetricLessonSpec {
  slug: string;
  title: string;
  level: Level;
  summary: string;
  keywords?: string[];
  what: string[];
  why: string[];
  formulas?: { tex: string; caption?: string }[];
  /** "where" list under the formula. */
  where?: string[];
  /** How the engine computes it: source of truth for the formula above. */
  engine: string;
  example: { intro: string; steps: string[]; result: string };
  high: string;
  low: string;
  misconceptions: string[];
  /** Metric slugs, or "category/slug" for lessons in other categories. */
  related: string[];
  platformLinks?: PlatformLink[];
  interactive?: InteractiveId;
  diagram?: { id: DiagramId; caption?: string };
  featured?: boolean;
}

const ref = (r: string) => (r.includes('/') ? r : `metrics/${r}`);

export function metricLesson(s: MetricLessonSpec): Lesson {
  const blocks: Block[] = [
    { type: 'h', text: 'What it measures' },
    ...s.what.map((text): Block => ({ type: 'p', text })),
    { type: 'h', text: 'Why it matters' },
    ...s.why.map((text): Block => ({ type: 'p', text })),
  ];
  if (s.diagram) blocks.push({ type: 'diagram', id: s.diagram.id, caption: s.diagram.caption });
  if (s.formulas?.length) {
    blocks.push({ type: 'h', text: 'Formula' });
    for (const f of s.formulas) blocks.push({ type: 'formula', tex: f.tex, caption: f.caption });
    if (s.where?.length) blocks.push({ type: 'list', items: s.where });
  }
  blocks.push({ type: 'callout', tone: 'info', title: 'How Spectra calculates it', text: s.engine });
  blocks.push(
    { type: 'h', text: 'Worked example' },
    { type: 'p', text: s.example.intro },
    { type: 'list', ordered: true, items: s.example.steps },
    { type: 'p', text: s.example.result },
  );
  if (s.interactive) blocks.push({ type: 'interactive', id: s.interactive });
  blocks.push(
    { type: 'h', text: 'Reading high and low values' },
    { type: 'table', head: ['Value', 'What it usually suggests'], rows: [['Higher', s.high], ['Lower', s.low]] },
    { type: 'h', text: 'Common misconceptions' },
    { type: 'list', items: s.misconceptions },
  );
  return {
    slug: s.slug,
    category: 'metrics',
    title: s.title,
    level: s.level,
    summary: s.summary,
    keywords: s.keywords,
    blocks,
    related: s.related.map(ref),
    platformLinks: s.platformLinks ?? [{ label: 'Open Backtests', href: '/studio?tab=backtests' }],
    featured: s.featured,
  };
}

/** A plain lesson: the content is written as blocks directly. */
export function lesson(category: CategoryId, l: Omit<Lesson, 'category'>): Lesson {
  return { ...l, category };
}
