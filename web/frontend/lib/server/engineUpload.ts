import crypto from 'crypto';
import fs from 'fs';
import os from 'os';
import path from 'path';

/**
 * Helpers for app/api/engine-upload (M1 task 4): a data file streamed from the browser to the
 * engine's POST /api/v1/uploads.
 *
 * The engine's signature covers the body's SHA-256, so the file is first written to a temporary
 * file while it is hashed (never held in memory), then sent with that hash. Anything over
 * MAX_UPLOAD_BYTES is refused as soon as it is seen.
 */
export const MAX_UPLOAD_BYTES = 100 * 1024 * 1024;

/** The name to show: no directories, no control characters, at most 255 characters. */
export function cleanFilename(name: string | null | undefined): string {
  const base = (name ?? '').split(/[\\/]/).pop() ?? '';
  return base.replace(/[\u0000-\u001f\u007f]/g, '').slice(0, 255).trim();
}

export type Spooled = { file: string; sha256: string; size: number };

export class UploadTooLarge extends Error {}

/** Writes `body` to a new temporary file, hashing it; throws UploadTooLarge past `limit` bytes. */
export async function spoolToTempFile(body: ReadableStream<Uint8Array>, limit = MAX_UPLOAD_BYTES): Promise<Spooled> {
  const file = path.join(os.tmpdir(), `spectra-upload-${crypto.randomUUID()}`);
  const out = fs.createWriteStream(file, { flags: 'wx', mode: 0o600 });
  const hash = crypto.createHash('sha256');
  let size = 0;
  const reader = body.getReader();
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > limit) throw new UploadTooLarge(`Uploads are limited to ${Math.round(limit / 1024 / 1024)} MiB.`);
      hash.update(value);
      if (!out.write(value)) await new Promise<void>((resolve) => out.once('drain', () => resolve()));
    }
    await new Promise<void>((resolve, reject) => out.end((err?: Error | null) => (err ? reject(err) : resolve())));
    return { file, sha256: hash.digest('hex'), size };
  } catch (e) {
    out.destroy();
    await reader.cancel().catch(() => {});
    await fs.promises.rm(file, { force: true });
    throw e;
  }
}
