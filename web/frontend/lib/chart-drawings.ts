/**
 * M36: Chart drawings, symbol comparison, and saved layout templates client library.
 */

export type DrawingToolType = 'select' | 'trendline' | 'horizontal_level' | 'fibonacci' | 'text';

export interface ChartDrawingPoint {
  time?: number; // Unix timestamp in seconds or nanoseconds
  price?: number;
  ts_ns?: number;
}

export interface ChartDrawingStyle {
  color?: string;
  lineWidth?: number;
  lineStyle?: 'solid' | 'dashed' | 'dotted';
  fontSize?: number;
  text?: string;
  showLabels?: boolean;
}

export interface ChartDrawing {
  id: string;
  chart_id?: string;
  dataset_version_id?: string;
  tool: DrawingToolType;
  points: ChartDrawingPoint[];
  style?: ChartDrawingStyle;
  created_at?: string;
  updated_at?: string;
}

export interface ComparisonSymbolSeries {
  symbol: string;
  base_price: number;
  values: (number | null)[];
}

export interface ComparisonResponse {
  primary: ComparisonSymbolSeries;
  secondary: ComparisonSymbolSeries;
  base_symbol?: string;
  compare_symbols?: string[];
  series?: Record<string, (number | null)[]>;
  timestamps: number[];
  count: number;
  message?: string | null;
}

export interface ChartTemplate {
  id: string;
  name: string;
  version: number;
  indicators: any[];
  drawings: any[];
  comparisons: string[];
  dataset_versions: Record<string, string> | any[];
  retired_versions?: string[];
  retired_dataset_versions?: string[];
  created_at?: string;
  updated_at?: string;
}

export const FIBONACCI_LEVELS = [
  { ratio: 0.0, label: '0.0%', color: '#787B86' },
  { ratio: 0.236, label: '23.6%', color: '#F23645' },
  { ratio: 0.382, label: '38.2%', color: '#FF9800' },
  { ratio: 0.5, label: '50.0%', color: '#4CAF50' },
  { ratio: 0.618, label: '61.8%', color: '#089981' },
  { ratio: 0.786, label: '78.6%', color: '#00BCD4' },
  { ratio: 1.0, label: '100.0%', color: '#787B86' },
];

export const DRAWING_COLOR_PALETTE = [
  '#2962FF', // Blue (default)
  '#089981', // Teal / Bullish
  '#F23645', // Red / Bearish
  '#FF9800', // Orange
  '#9C27B0', // Purple
  '#FFEB3B', // Yellow
  '#E0E0E0', // Light grey / white
];

/**
 * Fetches active drawings for a chart.
 */
export async function fetchDrawings(chartId: string): Promise<ChartDrawing[]> {
  const res = await fetch(`/api/v1/charts/${encodeURIComponent(chartId)}/drawings`);
  if (!res.ok) {
    if (res.status === 404) return [];
    throw new Error(`Failed to fetch drawings: ${res.statusText}`);
  }
  const data = await res.json();
  return data.items || [];
}

/**
 * Creates a new drawing for a chart.
 */
export async function saveDrawing(
  chartId: string,
  drawing: Omit<ChartDrawing, 'id'>
): Promise<ChartDrawing> {
  const res = await fetch(`/api/v1/charts/${encodeURIComponent(chartId)}/drawings`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(drawing),
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    throw new Error(err.message || `Failed to create drawing: ${res.statusText}`);
  }
  return res.json();
}

/**
 * Updates an existing drawing's coordinates or style.
 */
export async function updateDrawing(
  id: string,
  updates: { points?: ChartDrawingPoint[]; style?: ChartDrawingStyle }
): Promise<ChartDrawing> {
  const res = await fetch(`/api/v1/drawings/${encodeURIComponent(id)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(updates),
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    throw new Error(err.message || `Failed to update drawing: ${res.statusText}`);
  }
  return res.json();
}

/**
 * Soft deletes a drawing.
 */
export async function deleteDrawing(id: string): Promise<void> {
  const res = await fetch(`/api/v1/drawings/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    throw new Error(err.message || `Failed to delete drawing: ${res.statusText}`);
  }
}

/**
 * Fetches symbol comparison data rebased to 100 on the first bar of the full dataset.
 */
export async function fetchSymbolComparison(params: {
  dataset_version_id: string;
  base_symbol: string;
  compare_symbols: string | string[];
  timeframe: string;
}): Promise<ComparisonResponse> {
  const compStr = Array.isArray(params.compare_symbols)
    ? params.compare_symbols.join(',')
    : params.compare_symbols;

  const q = new URLSearchParams({
    dataset_version_id: params.dataset_version_id,
    primary: params.base_symbol,
    secondary: compStr,
    timeframe: params.timeframe,
  });

  const res = await fetch(`/api/v1/charts/compare?${q.toString()}`);
  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    throw new Error(err.message || `Comparison request failed (${res.status})`);
  }
  return res.json();
}

/**
 * Lists saved layout templates for the active workspace.
 */
export async function fetchTemplates(): Promise<ChartTemplate[]> {
  const res = await fetch('/api/v1/chart-templates');
  if (!res.ok) {
    throw new Error(`Failed to fetch templates: ${res.statusText}`);
  }
  const data = await res.json();
  return data.items || [];
}

/**
 * Saves a new chart layout template.
 */
export async function saveTemplate(template: {
  name: string;
  version?: number;
  indicators?: any[];
  drawings?: any[];
  comparisons?: string[];
  dataset_versions?: Record<string, string> | any[];
}): Promise<ChartTemplate> {
  const res = await fetch('/api/v1/chart-templates', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(template),
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    throw new Error(err.message || `Failed to save template: ${res.statusText}`);
  }
  return res.json();
}

/**
 * Retrieves a single template with retired dataset version checks.
 */
export async function getTemplate(id: string): Promise<ChartTemplate> {
  const res = await fetch(`/api/v1/chart-templates/${encodeURIComponent(id)}`);
  if (!res.ok) {
    throw new Error(`Failed to get template: ${res.statusText}`);
  }
  return res.json();
}

/**
 * Deletes a chart template.
 */
export async function deleteTemplate(id: string): Promise<void> {
  const res = await fetch(`/api/v1/chart-templates/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    throw new Error(err.message || `Failed to delete template: ${res.statusText}`);
  }
}
