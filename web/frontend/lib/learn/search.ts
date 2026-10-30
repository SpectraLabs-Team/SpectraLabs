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

const isBreak = (c: string | undefined) => c === undefined || c === ' ' || c === '-';

/** Whether `t` occurs in `field` starting at a word boundary (any occurrence, not just the first). */
function startsWord(field: string, t: string): boolean {
  for (let i = field.indexOf(t); i >= 0; i = field.indexOf(t, i + 1)) if (isBreak(field[i - 1])) return true;
  return false;
}

/** Whether `phrase` occurs in `field` as whole words ("es" is not in "stress testing"). */
function hasWords(field: string, phrase: string): boolean {
  for (let i = field.indexOf(phrase); i >= 0; i = field.indexOf(phrase, i + 1)) {
    if (isBreak(field[i - 1]) && isBreak(field[i + phrase.length])) return true;
  }
  return false;
}

function scoreField(field: string, alternatives: string[], weightWord: number, weightAny: number): number {
  let best = 0;
  for (const t of alternatives) {
    if (field.indexOf(t) < 0) continue;
    best = Math.max(best, startsWord(field, t) ? weightWord : weightAny);
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
    else if (title.startsWith(phrase) && isBreak(title[phrase.length])) total += 120;
    else if (hasWords(title, phrase)) total += 80;
    else if (hasWords(keywords, phrase)) total += 40;
    scored.push({ e, s: total });
  }
  // Ties: the shorter (more general) title first, then alphabetical.
  scored.sort((a, b) => b.s - a.s || a.e.title.length - b.e.title.length || a.e.title.localeCompare(b.e.title));
  return scored.slice(0, limit).map((x) => x.e);
}
