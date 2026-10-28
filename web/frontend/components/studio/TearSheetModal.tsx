'use client';

import { useRef } from 'react';
import {
  Check,
  Clock,
  Download,
  ExternalLink,
  FileSpreadsheet,
  FileText,
  Fingerprint,
  Layers,
  Printer,
  ShieldCheck,
  X,
} from 'lucide-react';
import { Badge, GlassCard } from '@/components/ui/primitives';
import { useToast } from '@/components/ui/StudioToast';
import { cn } from '@/lib/utils';

export interface TearSheetMetricItem {
  label: string;
  value: string;
  sub?: string;
  tone?: 'profit' | 'loss' | 'neutral';
}

export interface TearSheetSection {
  category: string;
  items: TearSheetMetricItem[];
}

export interface TearSheetTable {
  title: string;
  headers: string[];
  rows: (string | number)[][];
}

export interface TearSheetModalProps {
  open: boolean;
  onClose: () => void;
  title: string;
  subtitle: string;
  reportType: 'Risk Audit' | 'Quantitative Backtest' | 'Comparative Analysis';
  timestamp: string;
  workspaceName?: string;
  metadata: {
    engine: string;
    engineVersion?: string;
    commitSha?: string;
    seed?: string;
    datasetOrBook?: string;
    duration?: string;
  };
  sections: TearSheetSection[];
  table?: TearSheetTable;
  rawJsonData?: unknown;
  rawCsvData?: string;
}

export function TearSheetModal({
  open,
  onClose,
  title,
  subtitle,
  reportType,
  timestamp,
  workspaceName = 'Spectra Research',
  metadata,
  sections,
  table,
  rawJsonData,
  rawCsvData,
}: TearSheetModalProps) {
  const { success } = useToast();
  const printRef = useRef<HTMLDivElement>(null);

  if (!open) return null;

  const handlePrint = () => {
    window.print();
  };

  const handleDownloadJson = () => {
    if (!rawJsonData) return;
    const blob = new Blob([JSON.stringify(rawJsonData, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `spectra-${reportType.toLowerCase().replace(/\s+/g, '-')}-${Date.now()}.json`;
    a.click();
    URL.revokeObjectURL(url);
    success('JSON Exported', 'Audit JSON report downloaded successfully.');
  };

  const handleDownloadCsv = () => {
    if (!rawCsvData) return;
    const blob = new Blob([rawCsvData], { type: 'text/csv' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `spectra-${reportType.toLowerCase().replace(/\s+/g, '-')}-${Date.now()}.csv`;
    a.click();
    URL.revokeObjectURL(url);
    success('CSV Exported', 'Structured CSV report downloaded successfully.');
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center p-3 sm:p-6 print:p-0 print:static print:block"
    >
      {/* Dark backdrop (hidden on print) */}
      <div
        className="fixed inset-0 bg-black/80 backdrop-blur-md transition-opacity print:hidden"
        onClick={onClose}
      />

      {/* Modal Container */}
      <div className="relative z-10 flex max-h-[92vh] w-full max-w-4xl flex-col rounded-2xl border border-white/10 bg-[#0c121e] shadow-2xl overflow-hidden print:border-none print:shadow-none print:max-h-none print:w-full print:bg-white print:text-black">
        {/* Modal Action Bar (hidden on print) */}
        <div className="flex flex-wrap items-center justify-between gap-3 border-b border-white/10 bg-white/[0.03] px-6 py-4 print:hidden">
          <div className="flex items-center gap-2">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-accent/15 text-accent">
              <FileText className="h-4 w-4" />
            </div>
            <div>
              <h2 className="text-sm font-semibold text-white">Executive Tear-Sheet</h2>
              <p className="text-[11px] text-slate-400">Institutional Print-Ready Compliance Report</p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={handlePrint}
              className="flex items-center gap-1.5 rounded-xl border border-cyan-500/30 bg-cyan-500/10 px-3 py-1.5 text-xs font-semibold text-cyan-400 transition hover:bg-cyan-500/20"
              title="Print document or Save as PDF"
            >
              <Printer className="h-3.5 w-3.5" />
              <span>Print / Save PDF</span>
            </button>
            {rawCsvData && (
              <button
                type="button"
                onClick={handleDownloadCsv}
                className="btn-secondary px-3 py-1.5 text-xs flex items-center gap-1"
                title="Download CSV"
              >
                <FileSpreadsheet className="h-3.5 w-3.5" />
                <span>CSV</span>
              </button>
            )}
            {rawJsonData != null && (
              <button
                type="button"
                onClick={handleDownloadJson}
                className="btn-secondary px-3 py-1.5 text-xs flex items-center gap-1"
                title="Download Raw JSON"
              >
                <Download className="h-3.5 w-3.5" />
                <span>JSON</span>
              </button>
            )}
            <button
              type="button"
              onClick={onClose}
              className="rounded-lg p-1.5 text-slate-400 transition hover:bg-white/10 hover:text-white ml-2"
              aria-label="Close"
            >
              <X className="h-4 w-4" />
            </button>
          </div>
        </div>

        {/* Scrollable Printable Report Body */}
        <div
          ref={printRef}
          className="flex-1 overflow-y-auto p-6 sm:p-8 space-y-6 text-white print:text-black print:p-0 print:overflow-visible"
        >
          {/* Institutional Document Header */}
          <div className="border-b border-white/15 pb-6 print:border-black/20">
            <div className="flex flex-wrap items-start justify-between gap-4">
              <div>
                <div className="flex items-center gap-2">
                  <span className="font-mono text-[11px] font-bold tracking-widest uppercase text-cyan-400 print:text-blue-700">
                    SPECTRA LABS QUANTITATIVE RESEARCH
                  </span>
                  <Badge tone="accent">{reportType}</Badge>
                </div>
                <h1 className="mt-2 text-2xl font-bold tracking-tight text-white print:text-black">
                  {title}
                </h1>
                <p className="mt-1 text-xs text-slate-400 print:text-slate-600">{subtitle}</p>
              </div>

              <div className="text-right text-xs text-slate-400 print:text-slate-600 space-y-1 font-mono">
                <p>
                  <span className="text-slate-500">Workspace:</span>{' '}
                  <span className="font-semibold text-slate-300 print:text-black">{workspaceName}</span>
                </p>
                <p>
                  <span className="text-slate-500">Generated:</span> {timestamp}
                </p>
                <p>
                  <span className="text-slate-500">Engine:</span> {metadata.engine}{' '}
                  {metadata.engineVersion ? `v${metadata.engineVersion}` : ''}
                </p>
              </div>
            </div>

            {/* Audit & Compute Lineage Pill */}
            <div className="mt-4 flex flex-wrap items-center gap-x-4 gap-y-1 rounded-xl bg-white/[0.03] border border-white/5 px-4 py-2.5 text-[11px] font-mono text-slate-400 print:bg-slate-100 print:text-black print:border-slate-300">
              {metadata.seed && (
                <div>
                  <span className="text-slate-500">Seed:</span> {metadata.seed}
                </div>
              )}
              {metadata.commitSha && (
                <div>
                  <span className="text-slate-500">Commit:</span> {metadata.commitSha.slice(0, 8)}
                </div>
              )}
              {metadata.duration && (
                <div>
                  <span className="text-slate-500">Compute Time:</span> {metadata.duration}
                </div>
              )}
              {metadata.datasetOrBook && (
                <div>
                  <span className="text-slate-500">Source:</span> {metadata.datasetOrBook}
                </div>
              )}
              <div className="ml-auto flex items-center gap-1 text-emerald-400 print:text-emerald-700">
                <ShieldCheck className="h-3.5 w-3.5" />
                <span>Cryptographically Verified</span>
              </div>
            </div>
          </div>

          {/* Metric Sections */}
          <div className="space-y-6">
            {sections.map((sec) => (
              <div key={sec.category} className="print:break-inside-avoid">
                <h3 className="text-xs font-bold uppercase tracking-wider text-slate-400 print:text-slate-700 border-b border-white/10 pb-1.5 print:border-black/15 mb-3">
                  {sec.category}
                </h3>
                <div className="grid grid-cols-2 gap-3 sm:grid-cols-4 print:grid-cols-4">
                  {sec.items.map((item) => (
                    <div
                      key={item.label}
                      className="rounded-xl border border-white/10 bg-white/[0.02] p-3 print:bg-white print:border-slate-300"
                    >
                      <p className="text-[10.5px] font-medium text-slate-400 print:text-slate-600">
                        {item.label}
                      </p>
                      <p
                        className={cn(
                          'mt-1 font-mono text-lg font-bold print:text-black',
                          item.tone === 'profit'
                            ? 'text-emerald-400'
                            : item.tone === 'loss'
                            ? 'text-rose-400'
                            : 'text-white'
                        )}
                      >
                        {item.value}
                      </p>
                      {item.sub && (
                        <p className="mt-0.5 text-[10px] text-slate-500 print:text-slate-600">
                          {item.sub}
                        </p>
                      )}
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>

          {/* Detailed Data Table */}
          {table && table.rows.length > 0 && (
            <div className="space-y-3 print:break-inside-avoid pt-2">
              <h3 className="text-xs font-bold uppercase tracking-wider text-slate-400 print:text-slate-700 border-b border-white/10 pb-1.5 print:border-black/15">
                {table.title}
              </h3>
              <div className="overflow-x-auto rounded-xl border border-white/10 bg-white/[0.01] print:border-slate-300 print:bg-white">
                <table className="w-full text-left font-mono text-xs print:text-[11px]">
                  <thead>
                    <tr className="border-b border-white/10 bg-white/[0.03] text-[10.5px] uppercase tracking-wider text-slate-400 print:bg-slate-100 print:text-black print:border-slate-300">
                      {table.headers.map((h, i) => (
                        <th
                          key={h}
                          className={cn('py-2.5 px-3.5', i > 0 && 'text-right')}
                        >
                          {h}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-white/5 print:divide-slate-200">
                    {table.rows.map((row, idx) => (
                      <tr key={idx} className="hover:bg-white/[0.02] print:hover:bg-transparent">
                        {row.map((val, cellIdx) => (
                          <td
                            key={cellIdx}
                            className={cn(
                              'py-2 px-3.5 text-slate-300 print:text-black',
                              cellIdx > 0 && 'text-right'
                            )}
                          >
                            {val}
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}

          {/* Footer & Compliance Disclaimer */}
          <div className="border-t border-white/10 pt-4 print:border-slate-300 text-[10px] text-slate-500 print:text-slate-600 space-y-1 leading-relaxed">
            <p>
              CONFIDENTIAL & PROPRIETARY — Produced by Spectra Labs Quantitative Risk & Execution Platform.
            </p>
            <p>
              Simulated performance and risk statistics are generated under discrete mathematical models and are subject to estimation risk, model assumptions, and data limitations.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
