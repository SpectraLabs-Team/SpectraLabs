import { engine } from './content/engine';
import { fundamentals } from './content/fundamentals';
import { benchmarkMetrics } from './content/metrics-benchmark';
import { modelMetrics } from './content/metrics-models';
import { returnMetrics } from './content/metrics-returns';
import { riskMetrics } from './content/metrics-risk';
import { tradeMetrics } from './content/metrics-trades';
import { paper } from './content/paper';
import { simulations } from './content/simulations';
import { strategies } from './content/strategies';
import type { Block, Category, CategoryId, Lesson } from './types';

export type { Block, Category, CategoryId, Lesson, Level } from './types';

export const CATEGORIES: Category[] = [
  { id: 'fundamentals', title: 'Trading Fundamentals', description: 'Orders, prices, spreads, liquidity and the mechanics every trader needs.', icon: 'BookOpen' },
  { id: 'engine', title: 'Trading Engine Concepts', description: 'How the Spectra engine turns decisions into orders, fills, positions and risk checks.', icon: 'Cpu' },
  { id: 'simulations', title: 'Trading Simulations', description: 'Backtests, Monte Carlo and paper trading: how they work and how to read them.', icon: 'FlaskConical' },
  { id: 'metrics', title: 'Performance Metrics', description: 'Every metric in your results, with its exact formula, an example and how to read it.', icon: 'Gauge' },
  { id: 'strategies', title: 'Trading Strategies', description: 'The logic, assumptions and limits of the strategies you can run on the platform.', icon: 'Workflow' },
  { id: 'paper-trading', title: 'Paper Trading', description: 'Practise on live markets with an Alpaca paper account and simulated funds.', icon: 'TrendingUp' },
];

/** Every lesson, grouped by category in reading order. */
export const LESSONS: Lesson[] = [
  ...fundamentals,
  ...engine,
  ...simulations,
  ...returnMetrics,
  ...riskMetrics,
  ...tradeMetrics,
  ...benchmarkMetrics,
  ...modelMetrics,
  ...strategies,
  ...paper,
];

export const lessonKey = (l: Pick<Lesson, 'category' | 'slug'>) => `${l.category}/${l.slug}`;
export const lessonPath = (l: Pick<Lesson, 'category' | 'slug'>) => `/learn/${l.category}/${l.slug}`;

const BY_KEY = new Map(LESSONS.map((l) => [lessonKey(l), l]));

export function getCategory(id: string): Category | undefined {
  return CATEGORIES.find((c) => c.id === id);
}

export function getLesson(category: string, slug: string): Lesson | undefined {
  return BY_KEY.get(`${category}/${slug}`);
}

export function getLessonByKey(key: string): Lesson | undefined {
  return BY_KEY.get(key);
}

export function lessonsIn(category: CategoryId): Lesson[] {
  return LESSONS.filter((l) => l.category === category);
}

/** Previous and next lesson in reading order (across categories). */
export function neighbours(lesson: Lesson): { prev?: Lesson; next?: Lesson } {
  const i = LESSONS.findIndex((l) => lessonKey(l) === lessonKey(lesson));
  return { prev: i > 0 ? LESSONS[i - 1] : undefined, next: i >= 0 && i < LESSONS.length - 1 ? LESSONS[i + 1] : undefined };
}

export function relatedLessons(lesson: Lesson): Lesson[] {
  return lesson.related.map((k) => BY_KEY.get(k)).filter((l): l is Lesson => !!l);
}

/** Plain text of a lesson (markup stripped), for search and reading time. */
export function lessonText(lesson: Lesson): string {
  const strip = (s: string) => s.replace(/\*\*|`|\\\(.*?\\\)/g, ' ').replace(/\[([^\]]+)\]\([^)]+\)/g, '$1');
  const parts: string[] = [lesson.title, lesson.summary];
  const add = (b: Block) => {
    switch (b.type) {
      case 'p':
      case 'h':
        parts.push(strip(b.text));
        break;
      case 'list':
        parts.push(...b.items.map(strip));
        break;
      case 'callout':
        parts.push(strip(b.text));
        break;
      case 'table':
        parts.push(...b.head, ...b.rows.flat().map(strip));
        break;
      case 'code':
        parts.push(b.text);
        break;
      case 'formula':
      case 'diagram':
      case 'interactive':
        if ('caption' in b && b.caption) parts.push(b.caption);
        break;
    }
  };
  lesson.blocks.forEach(add);
  return parts.join(' ');
}

/** Estimated reading time in minutes (about 200 words a minute, rounded up). */
export function readingMinutes(lesson: Lesson): number {
  const words = lessonText(lesson).split(/\s+/).filter(Boolean).length;
  return Math.max(1, Math.ceil(words / 200));
}

/** A compact record per lesson for client-side search. */
export interface SearchEntry {
  key: string;
  href: string;
  title: string;
  summary: string;
  category: CategoryId;
  categoryTitle: string;
  level: Lesson['level'];
  minutes: number;
  keywords: string;
  body: string;
}

export function searchIndex(): SearchEntry[] {
  return LESSONS.map((l) => ({
    key: lessonKey(l),
    href: lessonPath(l),
    title: l.title,
    summary: l.summary,
    category: l.category,
    categoryTitle: getCategory(l.category)?.title ?? l.category,
    level: l.level,
    minutes: readingMinutes(l),
    keywords: (l.keywords ?? []).join(' '),
    body: lessonText(l).toLowerCase().slice(0, 1500),
  }));
}

export { searchLessons } from './search';
