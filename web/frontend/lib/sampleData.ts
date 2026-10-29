import { followIngest, ingest, uploadFile, type IngestRejection, type IngestResult, type IngestSettings } from '@/lib/datasets';
import type { Job } from '@/lib/jobs';
import { buildSampleCsvFor, FIRST_DAY, SAMPLE_COLUMNS, sampleInfo, type SampleKind } from '@/lib/sampleCsv';

export { SAMPLE_DATASET_NAME, SAMPLE_DATASETS, sampleInfo, type SampleKind } from '@/lib/sampleCsv';

/** Ingest settings matching {@link buildSampleCsv} exactly, so no preview step is needed. */
export const SAMPLE_SETTINGS: IngestSettings = {
  delimiter: ',',
  time_format: 'iso8601',
  utc_offset_minutes: 0,
  timezone: 'UTC',
  exchange: null,
  columns: { ...SAMPLE_COLUMNS },
  default_symbol: null,
  skip_bad_rows: false,
  quality: { gap_factor: 5, outlier_z: 10, stale_run: 5 },
};

export type SamplePhase = 'upload' | 'ingest';

/**
 * Uploads and ingests the sample dataset through the normal pipeline (same checks as any upload).
 * Resolves with the finished ingest job; its status says whether it succeeded.
 */
export async function loadSampleDataset(
  onPhase: (phase: SamplePhase) => void,
  signal?: AbortSignal,
  kind: SampleKind = 'trend',
): Promise<Job<IngestResult | IngestRejection> | undefined> {
  const info = sampleInfo(kind);
  const file = new File([buildSampleCsvFor(kind)], info.filename, { type: 'text/csv', lastModified: FIRST_DAY });
  onPhase('upload');
  const upload = await uploadFile(file, () => undefined, signal);
  onPhase('ingest');
  const job = await ingest({ upload_id: upload.id, dataset_name: info.name, settings: SAMPLE_SETTINGS });
  return followIngest(job.id, () => undefined, signal);
}
