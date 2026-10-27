/**
 * Learning Center search: a small ranked, all-terms-must-match search over the lesson index.
 * Pure and dependency-free so it runs in the browser and in scripts/test-learn.mjs.
 */

export interface Searchable {
  title: string;
  summary: string;
  keywords: string;
  body: string;
  categoryTitle: string;
}

const norm = (s: string) =>
  s
    .toLowerCase()
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '')
    .replace(/&/g, ' and ')
    .replace(/[^a-z0-9%+.\- ]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();

/** Common short forms people type, mapped to words that appear in lessons. */
const SYNONYMS: Record<string, string[]> = {
  mdd: ['drawdown'],
  dd: ['drawdown'],
  sr: ['sharpe'],
  pnl: ['p and l', 'profit'],
  'p and l': ['profit'],
  cvar: ['expected shortfall'],
  es: ['expected shortfall'],
  pf: ['profit factor'],
  vol: ['volatility'],
  roi: ['return'],
  ma: ['moving average'],
  sma: ['moving average'],
  ema: ['exponential'],
  tif: ['time in force'],
};

function terms(query: string): string[][] {
  const q = norm(query);
  if (!q) return [];
  return q
    .split(' ')
    .filter((t) => t.length > 0)
    .map((t) => [t, ...(SYNONYMS[t] ?? [])]);
}

function scoreField(field: string, alternatives: string[], weightWord: number, weightAny: number): number {
  let best = 0;
  for (const t of alternatives) {
    const i = field.indexOf(t);
    if (i < 0) continue;
    const wordStart = i === 0 || field[i - 1] === ' ' || field[i - 1] === '-';
    best = Math.max(best, wordStart ? weightWord : weightAny);
  }
  return best;
}

/** Entries matching every term of `query`, best first. An empty query returns nothing. */
export function searchLessons<T extends Searchable>(entries: T[], query: string, limit = 20): T[] {
  const ts = terms(query);
  if (ts.length === 0) return [];
  const phrase = norm(query);
  const scored: { e: T; s: number }[] = [];
  for (const e of entries) {
    const title = norm(e.title);
    const keywords = norm(e.keywords);
    const summary = norm(e.summary);
    const body = norm(e.body);
    const cat = norm(e.categoryTitle);
    let total = 0;
    let all = true;
    for (const alt of ts) {
      const s = Math.max(
        scoreField(title, alt, 40, 20),
        scoreField(keywords, alt, 30, 15),
        scoreField(summary, alt, 15, 8),
        scoreField(cat, alt, 6, 3),
        scoreField(body, alt, 5, 2),
      );
      if (s === 0) {
        all = false;
        break;
      }
      total += s;
    }
    if (!all) continue;
    if (title === phrase) total += 200;
    else if (title.startsWith(phrase)) total += 120;
    else if (title.includes(phrase)) total += 80;
    else if (keywords.includes(phrase)) total += 40;
    scored.push({ e, s: total });
  }
  // Ties: the shorter (more general) title first, then alphabetical.
  scored.sort((a, b) => b.s - a.s || a.e.title.length - b.e.title.length || a.e.title.localeCompare(b.e.title));
  return scored.slice(0, limit).map((x) => x.e);
}
