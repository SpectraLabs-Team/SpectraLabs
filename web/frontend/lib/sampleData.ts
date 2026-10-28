import { followIngest, ingest, uploadFile, type IngestRejection, type IngestResult, type IngestSettings } from '@/lib/datasets';
import type { Job } from '@/lib/jobs';
import { buildSampleCsv, FIRST_DAY, SAMPLE_COLUMNS, SAMPLE_DATASET_NAME, SAMPLE_FILENAME } from '@/lib/sampleCsv';

export { SAMPLE_DATASET_NAME } from '@/lib/sampleCsv';

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
): Promise<Job<IngestResult | IngestRejection> | undefined> {
  const file = new File([buildSampleCsv()], SAMPLE_FILENAME, { type: 'text/csv', lastModified: FIRST_DAY });
  onPhase('upload');
  const upload = await uploadFile(file, () => undefined, signal);
  onPhase('ingest');
  const job = await ingest({ upload_id: upload.id, dataset_name: SAMPLE_DATASET_NAME, settings: SAMPLE_SETTINGS });
  return followIngest(job.id, () => undefined, signal);
}
