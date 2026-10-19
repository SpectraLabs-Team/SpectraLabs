-- params: text, text, text, text[], integer
-- A worker is alive. $1 id, $2 hostname, $3 version, $4 kinds, $5 concurrency.
INSERT INTO workers (id, hostname, version, kinds, concurrency)
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (id) DO UPDATE SET heartbeat_at = now(), kinds = EXCLUDED.kinds, concurrency = EXCLUDED.concurrency
