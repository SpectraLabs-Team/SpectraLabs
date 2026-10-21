'use client';

import { useEffect, useState } from 'react';
import { RotateCcw } from 'lucide-react';
import type { BuiltinStrategy, ConfigField, ConfigSchema, ParamValues, UserConfig } from '@/lib/backtests';
import { cn } from '@/lib/utils';

/** A label, the control, and a hint or error line under it. */
export function Field({ label, hint, error, changed, children, className }: { label: string; hint?: string; error?: string; changed?: boolean; children: React.ReactNode; className?: string }) {
  return (
    <label className={cn('block min-w-0', className)}>
      <span className="mb-1 flex items-center gap-1.5 text-[11.5px] font-medium text-ink/60">
        <span className="truncate font-mono">{label}</span>
        {changed && <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-accent" title="Changed from the preset" />}
      </span>
      {children}
      {error ? (
        <span role="alert" className="mt-1 block text-[11px] leading-4 text-loss">
          {error}
        </span>
      ) : hint ? (
        <span className="mt-1 block text-[10.5px] leading-4 text-ink/45">{hint}</span>
      ) : null}
    </label>
  );
}

/**
 * A number input that keeps what is typed (so "0." or "-" can be edited). It reports only valid
 * numbers; while the text is not one (or out of range) it is marked invalid and the last valid
 * value stands.
 */
export function NumberInput({ value, onChange, min, max, integer, disabled, invalid, ariaLabel }: { value: number | undefined; onChange: (v: number) => void; min?: number; max?: number; integer?: boolean; disabled?: boolean; invalid?: boolean; ariaLabel?: string }) {
  const [text, setText] = useState(value === undefined ? '' : String(value));
  const [bad, setBad] = useState(false);
  useEffect(() => {
    // Follow outside changes (a preset switch, a reset) without clobbering what is being typed.
    setText((t) => (t.trim() !== '' && Number(t) === value ? t : value === undefined ? '' : String(value)));
    setBad(false);
  }, [value]);
  return (
    <input
      type="text"
      inputMode={integer ? 'numeric' : 'decimal'}
      aria-label={ariaLabel}
      aria-invalid={invalid || bad || undefined}
      disabled={disabled}
      className={cn('input num py-1.5 text-[13px]', (invalid || bad) && 'border-loss/60 focus:border-loss/70 focus:ring-loss/20')}
      value={text}
      placeholder={min !== undefined && max !== undefined ? `${min} – ${max}` : undefined}
      onChange={(e) => {
        const t = e.target.value;
        setText(t);
        const n = Number(t);
        const ok = t.trim() !== '' && Number.isFinite(n) && (!integer || Number.isInteger(n)) && (min === undefined || n >= min) && (max === undefined || n <= max);
        setBad(!ok);
        if (ok) onChange(n);
      }}
    />
  );
}

export function Toggle({ checked, onChange, label, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn('relative inline-flex h-5 w-9 shrink-0 items-center rounded-full border transition', checked ? 'border-accent/50 bg-accent/80' : 'border-ink/15 bg-ink/10', disabled && 'opacity-50')}
    >
      <span className={cn('inline-block h-3.5 w-3.5 rounded-full bg-white shadow transition-transform', checked ? 'translate-x-[18px]' : 'translate-x-[3px]')} />
    </button>
  );
}

/** Inputs for a strategy's parameters. `prefix` is the error-key prefix (e.g. "params." or "strategies[1].params."). */
export function ParamFields({ strategy, values, onChange, errors, prefix, disabled }: { strategy: BuiltinStrategy; values: ParamValues; onChange: (v: ParamValues) => void; errors: Record<string, string>; prefix: string; disabled?: boolean }) {
  if (!strategy.params.length) return <p className="text-[11.5px] text-ink/45">This strategy has no parameters.</p>;
  return (
    <div className="grid gap-3 sm:grid-cols-2">
      {strategy.params.map((p) => {
        const v = values[p.name];
        const err = errors[`${prefix}${p.name}`];
        const changed = v !== undefined && v !== p.default;
        if (p.type === 'bool') {
          return (
            <Field key={p.name} label={p.name} hint={p.description} error={err} changed={changed}>
              <Toggle label={p.name} checked={Boolean(v ?? p.default)} disabled={disabled} onChange={(b) => onChange({ ...values, [p.name]: b })} />
            </Field>
          );
        }
        return (
          <Field key={p.name} label={p.name} hint={`${p.description} (${p.min}–${p.max})`} error={err} changed={changed}>
            <NumberInput
              ariaLabel={p.name}
              value={typeof v === 'number' ? v : undefined}
              min={p.min}
              max={p.max}
              integer={p.type === 'int'}
              disabled={disabled}
              invalid={Boolean(err)}
              onChange={(n) => onChange({ ...values, [p.name]: n })}
            />
          </Field>
        );
      })}
    </div>
  );
}

/** The configuration form fields, grouped. Fields not listed here still show under "Other". */
export const CONFIG_GROUPS: { title: string; fields: string[] }[] = [
  { title: 'Capital and policy', fields: ['initial_cash', 'allow_short', 'allow_negative_cash', 'max_gross_leverage'] },
  { title: 'Costs', fields: ['commission_per_unit', 'commission_bps', 'commission_min', 'slippage_bps', 'impact_coefficient', 'impact_window'] },
  { title: 'Execution', fields: ['delay_bars', 'max_participation', 'exchange'] },
  { title: 'Financing (annual rates, 0.03 = 3%)', fields: ['borrow_fee_rate', 'cash_interest_rate', 'margin_interest_rate'] },
  { title: 'Reporting and seed', fields: ['periods_per_year', 'risk_free_rate', 'seed'] },
];

const types = (f: ConfigField) => (Array.isArray(f.type) ? f.type : [f.type]);
const lower = (f: ConfigField) => f.minimum ?? f.exclusiveMinimum;

/**
 * Inputs for every configuration field. `base` is the preset's configuration; `overrides` holds
 * only the fields changed from it (what is sent with the preset).
 */
export function ConfigFields({ schema, base, overrides, onChange, errors, disabled }: { schema: ConfigSchema; base: UserConfig; overrides: UserConfig; onChange: (o: UserConfig) => void; errors: Record<string, string>; disabled?: boolean }) {
  const set = (name: string, v: UserConfig[string] | undefined) => {
    const next = { ...overrides };
    if (v === undefined || v === base[name]) delete next[name];
    else next[name] = v;
    onChange(next);
  };
  const listed = new Set(CONFIG_GROUPS.flatMap((g) => g.fields));
  const other = Object.keys(schema.properties).filter((k) => k !== 'preset' && !listed.has(k));
  const groups = other.length ? [...CONFIG_GROUPS, { title: 'Other', fields: other }] : CONFIG_GROUPS;

  return (
    <div className="space-y-4">
      {groups.map((g) => {
        const fields = g.fields.filter((name) => schema.properties[name]);
        if (!fields.length) return null;
        return (
          <fieldset key={g.title}>
            <legend className="mb-2 font-mono text-[10px] uppercase tracking-[0.16em] text-ink/40">{g.title}</legend>
            <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
              {fields.map((name) => {
                const f = schema.properties[name];
                const t = types(f);
                const value = name in overrides ? overrides[name] : base[name];
                const err = errors[`config.${name}`];
                const changed = name in overrides;
                if (t.includes('boolean')) {
                  return (
                    <Field key={name} label={name} hint={f.description} error={err} changed={changed}>
                      <Toggle label={name} checked={Boolean(value)} disabled={disabled} onChange={(b) => set(name, b)} />
                    </Field>
                  );
                }
                if (t.includes('null')) {
                  // max_gross_leverage: a number, or null for no limit.
                  const off = value === null;
                  return (
                    <Field key={name} label={name} hint={f.description} error={err} changed={changed}>
                      <div className="flex items-center gap-2">
                        <NumberInput ariaLabel={name} value={off ? undefined : (value as number)} disabled={disabled || off} invalid={Boolean(err)} onChange={(n) => set(name, n)} />
                        <span className="flex shrink-0 items-center gap-1.5 text-[11px] text-ink/55">
                          <Toggle label={`${name}: no limit`} checked={off} disabled={disabled} onChange={(b) => set(name, b ? null : typeof base[name] === 'number' ? base[name] : 1)} />
                          no limit
                        </span>
                      </div>
                    </Field>
                  );
                }
                if (t.includes('string')) {
                  if (name === 'exchange') {
                    return (
                      <Field key={name} label={name} hint={f.description} error={err} changed={changed}>
                        <select
                          className={cn('input py-1.5 text-[13px]', err && 'border-loss/60')}
                          aria-label={name}
                          disabled={disabled}
                          value={value == null ? '' : String(value)}
                          onChange={(e) => {
                            const s = e.target.value.trim();
                            set(name, s === '' ? undefined : s);
                          }}
                        >
                          <option value="">None (all bars)</option>
                          <option value="XNYS">XNYS (NYSE / NASDAQ)</option>
                          <option value="XCME">XCME (CME Globex)</option>
                          <option value="XLON">XLON (London Stock Exchange)</option>
                          <option value="XTKS">XTKS (Tokyo Stock Exchange)</option>
                          <option value="XNSE">XNSE (National Stock Exchange of India)</option>
                          <option value="24/7">24/7 (Continuous)</option>
                        </select>
                      </Field>
                    );
                  }
                  // seed: a whole number, sent as a string when it is too large for JSON numbers.
                  return (
                    <Field key={name} label={name} hint={f.description} error={err} changed={changed}>
                      <input
                        className={cn('input num py-1.5 text-[13px]', err && 'border-loss/60')}
                        inputMode="numeric"
                        aria-label={name}
                        disabled={disabled}
                        value={value == null ? '' : String(value)}
                        onChange={(e) => {
                          const s = e.target.value.trim();
                          if (s === '') return set(name, undefined);
                          const n = Number(s);
                          set(name, /^[0-9]+$/.test(s) && Number.isSafeInteger(n) ? n : s);
                        }}
                      />
                    </Field>
                  );
                }
                const integer = t.includes('integer');
                return (
                  <Field key={name} label={name} hint={`${f.description}${lower(f) !== undefined && f.maximum !== undefined ? ` (${lower(f)}–${f.maximum})` : ''}`} error={err} changed={changed}>
                    <NumberInput ariaLabel={name} value={typeof value === 'number' ? value : undefined} min={lower(f)} max={f.maximum} integer={integer} disabled={disabled} invalid={Boolean(err)} onChange={(n) => set(name, n)} />
                  </Field>
                );
              })}
            </div>
          </fieldset>
        );
      })}
      {Object.keys(overrides).length > 0 && (
        <button type="button" className="btn-ghost" onClick={() => onChange({})} disabled={disabled}>
          <RotateCcw className="h-3.5 w-3.5" /> Reset to the preset ({Object.keys(overrides).length} changed)
        </button>
      )}
    </div>
  );
}
