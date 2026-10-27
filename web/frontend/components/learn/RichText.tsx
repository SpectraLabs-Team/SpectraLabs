import Link from 'next/link';
import katex from 'katex';
import type { ReactNode } from 'react';

/**
 * Inline lesson markup: **bold**, `code`, \( inline tex \) and [label](href).
 * Server component: KaTeX renders to static HTML at build time, so lessons ship no maths JavaScript.
 */
const TOKEN = /\*\*(.+?)\*\*|`([^`]+)`|\\\((.+?)\\\)|\[([^\]]+)\]\(([^)\s]+)\)/g;

export function texToHtml(tex: string, displayMode: boolean): string {
  return katex.renderToString(tex, { displayMode, throwOnError: false, strict: 'ignore', output: 'htmlAndMathml' });
}

export function RichText({ text }: { text: string }) {
  const out: ReactNode[] = [];
  let last = 0;
  let i = 0;
  for (const m of text.matchAll(TOKEN)) {
    const at = m.index ?? 0;
    if (at > last) out.push(text.slice(last, at));
    const key = i++;
    if (m[1] !== undefined) {
      out.push(
        <strong key={key} className="font-semibold text-ink">
          {m[1]}
        </strong>,
      );
    } else if (m[2] !== undefined) {
      out.push(
        <code key={key} className="rounded bg-ink/[0.06] px-1 py-px font-mono text-[0.86em] text-ink/90">
          {m[2]}
        </code>,
      );
    } else if (m[3] !== undefined) {
      out.push(<span key={key} dangerouslySetInnerHTML={{ __html: texToHtml(m[3], false) }} />);
    } else if (m[4] !== undefined && m[5] !== undefined) {
      const href = m[5];
      out.push(
        href.startsWith('/') ? (
          <Link key={key} href={href} className="font-medium text-accent underline-offset-2 hover:underline">
            {m[4]}
          </Link>
        ) : (
          <a key={key} href={href} target="_blank" rel="noreferrer noopener" className="font-medium text-accent underline-offset-2 hover:underline">
            {m[4]}
          </a>
        ),
      );
    }
    last = at + m[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return <>{out}</>;
}

export function Formula({ tex, caption }: { tex: string; caption?: string }) {
  return (
    <figure className="my-5 overflow-x-auto rounded-xl border border-ink/[0.08] bg-ink/[0.025] px-4 py-4 text-ink">
      <div className="min-w-fit text-[15px]" dangerouslySetInnerHTML={{ __html: texToHtml(tex, true) }} />
      {caption && <figcaption className="mt-2 text-center text-[11.5px] text-ink/50">{caption}</figcaption>}
    </figure>
  );
}
