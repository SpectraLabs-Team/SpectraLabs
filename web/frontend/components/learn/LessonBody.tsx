import { AlertTriangle, Info, Lightbulb } from 'lucide-react';
import type { Block } from '@/lib/learn/types';
import { cn } from '@/lib/utils';
import { Diagram } from './Diagrams';
import { Interactive } from './Interactive';
import { Formula, RichText } from './RichText';

export const headingId = (text: string) =>
  text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '');

const CALLOUT = {
  info: { icon: Info, cls: 'border-accent/25 bg-accent/[0.05]', iconCls: 'text-accent' },
  tip: { icon: Lightbulb, cls: 'border-profit/25 bg-profit/[0.05]', iconCls: 'text-profit' },
  warning: { icon: AlertTriangle, cls: 'border-caution/30 bg-caution/[0.06]', iconCls: 'text-caution' },
} as const;

function BlockView({ b }: { b: Block }) {
  switch (b.type) {
    case 'p':
      return (
        <p className="mt-4 text-[15px] leading-7 text-ink/80">
          <RichText text={b.text} />
        </p>
      );
    case 'h':
      return (
        <h2 id={headingId(b.text)} className="mt-10 scroll-mt-24 text-lg font-semibold tracking-tight text-ink">
          {b.text}
        </h2>
      );
    case 'list': {
      const Tag = b.ordered ? 'ol' : 'ul';
      return (
        <Tag className={cn('mt-4 space-y-2 pl-5 text-[15px] leading-7 text-ink/80', b.ordered ? 'list-decimal marker:font-mono marker:text-[13px] marker:text-ink/45' : 'list-disc marker:text-accent/70')}>
          {b.items.map((it, i) => (
            <li key={i} className="pl-1">
              <RichText text={it} />
            </li>
          ))}
        </Tag>
      );
    }
    case 'formula':
      return <Formula tex={b.tex} caption={b.caption} />;
    case 'callout': {
      const c = CALLOUT[b.tone];
      return (
        <aside className={cn('mt-5 flex gap-3 rounded-xl border px-4 py-3.5', c.cls)}>
          <c.icon className={cn('mt-0.5 h-4 w-4 shrink-0', c.iconCls)} aria-hidden />
          <div className="min-w-0 text-[14px] leading-6 text-ink/80">
            {b.title && <p className="mb-0.5 font-semibold text-ink">{b.title}</p>}
            <RichText text={b.text} />
          </div>
        </aside>
      );
    }
    case 'diagram':
      return <Diagram id={b.id} caption={b.caption} />;
    case 'interactive':
      return <Interactive id={b.id} />;
    case 'table':
      return (
        <div className="mt-5 overflow-x-auto rounded-xl border border-ink/[0.08]">
          <table className="w-full min-w-[480px] text-left text-[13.5px]">
            <thead className="bg-ink/[0.03]">
              <tr>
                {b.head.map((h) => (
                  <th key={h} scope="col" className="px-3.5 py-2.5 text-[11px] font-semibold uppercase tracking-wider text-ink/55">
                    {h}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {b.rows.map((r, i) => (
                <tr key={i} className="border-t border-ink/[0.06] align-top">
                  {r.map((c, j) => (
                    <td key={j} className={cn('px-3.5 py-2.5 leading-6 text-ink/80', j === 0 && 'font-medium text-ink')}>
                      <RichText text={c} />
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
    case 'code':
      return (
        <figure className="mt-5">
          <pre className="overflow-x-auto rounded-xl border border-ink/[0.08] bg-ink/[0.035] p-4 font-mono text-[12.5px] leading-6 text-ink/85">
            <code>{b.text}</code>
          </pre>
          {b.caption && <figcaption className="mt-1.5 text-[11.5px] text-ink/50">{b.caption}</figcaption>}
        </figure>
      );
  }
}

export function LessonBody({ blocks }: { blocks: Block[] }) {
  return (
    <div>
      {blocks.map((b, i) => (
        <BlockView key={i} b={b} />
      ))}
    </div>
  );
}
