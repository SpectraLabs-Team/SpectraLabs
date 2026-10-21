/**
 * Typed client for the engine's job queue (`/api/v1/jobs`, M20 steps 6 and 13).
 *
 * Long computations (benchmarks, large risk and stress runs) go through the queue instead of a
 * synchronous request: submit a job, poll it, then read its report and artifact. Calls go through
 * the authenticated proxy like every other engine call; viewers can read jobs, editors can submit
 * and cancel them (lib/server/engineRoutes.ts).
 *
 * The shapes mirror `JobView`, `JobPage` and `CreateJob` in crates/spectra-api/src/jobs_api.rs;
 * lib/generated/engine-api.ts (from `just api-client`) is the generated reference.
 */
import { ApiError, request } from './api';

export type JobKind = 'benchmark' | 'risk' | 'stress' | 'survival';
export type JobStatus = 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled';

/** The stored report, addressed by its SHA-256. */
export interface JobArtifact {
  sha256: string;
  /** Path inside the artifact store: sha256/<2 hex>/<2 hex>/<hash>. */
  path: string;
  size: number;
}

/** What a succeeded job leaves in `result`. */
export interface JobResult<Report = unknown> {
  run_id: string;
  report: Report;
  /** Null when the report could not be stored (see `artifact_error`); the report is still here. */
  artifact: JobArtifact | null;
  artifact_error: string | null;
}

export interface Job<Report = unknown> {
  id: string;
  kind: JobKind;
  status: JobStatus;
  /** -10..10; higher runs first. */
  priority: number;
  attempts: number;
  max_attempts: number;
  /** 0..1. */
  progress: number;
  cancel_requested: boolean;
  error: string | null;
  result: JobResult<Report> | null;
  /** UTC, RFC 3339. */
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  run_after: string;
}

export interface JobPage {
  /** Newest first. */
  items: Job[];
  /** Pass as `cursor` for the next page; null on the last. */
  next_cursor: string | null;
}

export interface CreateJobRequest<Payload = Record<string, unknown>> {
  kind: JobKind;
  /** The body the matching synchronous route takes (e.g. RiskRequest for `risk`). */
  payload: Payload;
  priority?: number;
}

export const TERMINAL_STATUSES: readonly JobStatus[] = ['succeeded', 'failed', 'cancelled'];

export const isFinished = (job: Pick<Job, 'status'>) => TERMINAL_STATUSES.includes(job.status);

/** Queue a job. The payload is validated now (422 on a bad one), not when it runs. */
export const createJob = <Report = unknown>(body: CreateJobRequest) =>
  request<Job<Report>>('/api/v1/jobs', { method: 'POST', body: JSON.stringify(body), timeoutMs: 20_000 });

export const getJob = <Report = unknown>(id: string) =>
  request<Job<Report>>(`/api/v1/jobs/${encodeURIComponent(id)}`, { timeoutMs: 10_000 });

export const listJobs = (opts: { status?: JobStatus; cursor?: string; limit?: number } = {}) => {
  const q = new URLSearchParams();
  if (opts.status) q.set('status', opts.status);
  if (opts.cursor) q.set('cursor', opts.cursor);
  if (opts.limit !== undefined) q.set('limit', String(opts.limit));
  const qs = q.toString();
  return request<JobPage>(`/api/v1/jobs${qs ? `?${qs}` : ''}`, { timeoutMs: 20_000 });
};

/** Queued jobs stop at once; running ones within a few seconds. Finished jobs come back unchanged. */
export const cancelJob = <Report = unknown>(id: string) =>
  request<Job<Report>>(`/api/v1/jobs/${encodeURIComponent(id)}/cancel`, { method: 'POST', timeoutMs: 10_000 });

/**
 * Poll until the job finishes, the signal aborts, or `timeoutMs` passes. Calls `onUpdate` with
 * each poll so the UI can show status and progress. Resolves with the finished job (whatever its
 * status); rejects with ApiError on timeout or abort.
 */
export async function waitForJob<Report = unknown>(
  id: string,
  opts: { intervalMs?: number; timeoutMs?: number; signal?: AbortSignal; onUpdate?: (job: Job<Report>) => void } = {},
): Promise<Job<Report>> {
  const interval = opts.intervalMs ?? 1_500;
  const deadline = Date.now() + (opts.timeoutMs ?? 15 * 60_000);
  for (;;) {
    if (opts.signal?.aborted) throw new ApiError('Stopped waiting for the job.', 0);
    let job: Job<Report>;
    try {
      job = await getJob<Report>(id);
    } catch (error) {
      // A rolling API restart or short proxy outage should not lose a durable
      // job. Keep polling until the overall deadline; callers can still abort.
      const retryable = error instanceof ApiError && (error.status === 0 || [502, 503, 504].includes(error.status));
      if (!retryable || Date.now() + interval > deadline) throw error;
      await new Promise((resolve) => setTimeout(resolve, interval));
      continue;
    }
    opts.onUpdate?.(job);
    if (isFinished(job)) return job;
    if (Date.now() + interval > deadline) throw new ApiError('The job is still running; check back later.', 0);
    await new Promise((resolve) => setTimeout(resolve, interval));
  }
}
