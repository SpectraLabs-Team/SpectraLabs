/**
 * Learning Center content model.
 *
 * Lessons are data, not pages: every lesson is rendered by the same components
 * (app/learn/[category]/[slug]/page.tsx), so adding a lesson never means adding a layout.
 *
 * Inline text in `p`, list items, callouts and table cells supports a small markup:
 * `**bold**`, `` `code` ``, `\(tex\)` (inline maths, rendered with KaTeX) and `[label](/href)`.
 * (Inline maths uses \( \) rather than $ so dollar amounts in the text stay plain text.)
 */

export type CategoryId = 'fundamentals' | 'engine' | 'simulations' | 'metrics' | 'strategies' | 'paper-trading';

export type Level = 'Beginner' | 'Intermediate' | 'Advanced';

/** Diagrams drawn as inline SVG (components/learn/Diagrams.tsx). */
export type DiagramId = 'order-book' | 'order-lifecycle' | 'engine-pipeline' | 'simulation-loop' | 'drawdown' | 'sma-crossover' | 'mean-reversion' | 'candlestick' | 'spread';

/** Client-side widgets (components/learn/Interactive.tsx). */
export type InteractiveId = 'equity-metrics' | 'trade-stats' | 'spread-cost' | 'position-sizing';

export type Block =
  | { type: 'p'; text: string }
  | { type: 'h'; text: string }
  | { type: 'list'; items: string[]; ordered?: boolean }
  | { type: 'formula'; tex: string; caption?: string }
  | { type: 'callout'; tone: 'info' | 'warning' | 'tip'; title?: string; text: string }
  | { type: 'diagram'; id: DiagramId; caption?: string }
  | { type: 'interactive'; id: InteractiveId }
  | { type: 'table'; head: string[]; rows: string[][] }
  | { type: 'code'; text: string; caption?: string };

/** A link from a lesson into the product (always an existing route). */
export interface PlatformLink {
  label: string;
  href: string;
}

export interface Lesson {
  slug: string;
  category: CategoryId;
  title: string;
  level: Level;
  /** One or two sentences: shown on cards and in search results. */
  summary: string;
  /** Extra search terms (synonyms, abbreviations). */
  keywords?: string[];
  blocks: Block[];
  /** Other lessons, as "category/slug". */
  related: string[];
  platformLinks?: PlatformLink[];
  /** Featured on the Learning Center homepage. */
  featured?: boolean;
}

export interface Category {
  id: CategoryId;
  title: string;
  description: string;
  /** lucide-react icon name, resolved in components/learn/icons.tsx. */
  icon: 'BookOpen' | 'Cpu' | 'FlaskConical' | 'Gauge' | 'Workflow' | 'TrendingUp';
}
