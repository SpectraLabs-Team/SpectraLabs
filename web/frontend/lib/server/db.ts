import { Pool, type PoolClient, type QueryResultRow } from 'pg';

/**
 * Postgres connection pool (server-only; never import from middleware or client components).
 *
 * DATABASE_URL is required. Tables are created by `npm run db:migrate` (also run automatically by
 * `npm run dev` and `npm start`) from the SQL files in db/migrations.
 */

// One pool per server process. Kept on globalThis so Next's hot reload doesn't open a new pool
// (and leak connections) every time a module is edited.
const g = globalThis as unknown as { __spectra_pg?: Pool };

function databaseUrl(): string {
  const url = process.env.DATABASE_URL;
  if (!url || !url.trim()) {
    throw new Error('DATABASE_URL is not set. Add it to web/frontend/.env.local (see .env.example) and run `npm run db:migrate`.');
  }
  return url;
}

export function pool(): Pool {
  if (!g.__spectra_pg) {
    const p = new Pool({
      connectionString: databaseUrl(),
      max: Number(process.env.DATABASE_POOL_MAX) || 10,
      idleTimeoutMillis: 30_000,
      connectionTimeoutMillis: 5_000,
    });
    // An idle client can drop (database restart, network blip). Log it; the pool replaces it.
    p.on('error', (err) => console.error('[db] idle connection error:', err.message));
    g.__spectra_pg = p;
  }
  return g.__spectra_pg;
}

type Queryable = Pick<PoolClient, 'query'>;

function explain(err: unknown): unknown {
  const code = (err as { code?: string })?.code;
  if (code === '42P01') {
    // undefined_table: the schema hasn't been created in this database yet.
    console.error('[db] A table is missing. Run `npm run db:migrate` in web/frontend.');
  } else if (code === 'ECONNREFUSED' || code === '3D000') {
    console.error('[db] Cannot reach the database in DATABASE_URL. Is Postgres running? See web/README.md.');
  }
  return err;
}

/** Run one parameterised query and return its rows. Pass `client` to run inside a transaction. */
export async function query<T extends QueryResultRow = QueryResultRow>(
  text: string,
  params: unknown[] = [],
  client: Queryable = pool(),
): Promise<T[]> {
  try {
    return (await client.query<T>(text, params)).rows;
  } catch (err) {
    throw explain(err);
  }
}

/** Run `fn` in a transaction: committed if it resolves, rolled back if it throws. */
export async function transaction<T>(fn: (client: PoolClient) => Promise<T>): Promise<T> {
  const client = await pool().connect();
  try {
    await client.query('BEGIN');
    const result = await fn(client);
    await client.query('COMMIT');
    return result;
  } catch (err) {
    await client.query('ROLLBACK').catch(() => {});
    throw explain(err);
  } finally {
    client.release();
  }
}

/** Close the pool (tests and scripts; the server keeps it for its lifetime). */
export async function closePool(): Promise<void> {
  const p = g.__spectra_pg;
  g.__spectra_pg = undefined;
  if (p) await p.end();
}
