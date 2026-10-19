/**
 * Base URL for links in emails (invites, security notices). APP_URL in production: building it from
 * the request's Host header would let anyone who can send a request with a forged Host get the
 * server to email links to their own site. In development the request origin is fine.
 */
export class AppUrlError extends Error {
  constructor() {
    super('APP_URL is not set: links in emails need the public address of the app.');
    this.name = 'AppUrlError';
  }
}

export function appUrl(req: Request): string {
  const configured = process.env.APP_URL?.trim();
  if (configured) return configured.replace(/\/+$/, '');
  if (process.env.NODE_ENV === 'production') throw new AppUrlError();
  return new URL(req.url).origin;
}
