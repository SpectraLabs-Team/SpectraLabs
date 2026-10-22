-- params: uuid, uuid, timestamptz
-- Resolves universe members as of a specific point-in-time date.
-- $1 workspace, $2 universe id, $3 as_of timestamp.
-- Returns constituent symbol, resolved ticker as of date, active status (false if delisted before/at date), and delisted timestamp.
WITH members AS (
    SELECT DISTINCT m.symbol
      FROM universe_members m
     WHERE m.workspace_id = $1
       AND m.universe_id = $2
       AND m.added_at <= $3
       AND (m.removed_at IS NULL OR m.removed_at > $3)
),
mapped AS (
    SELECT m.symbol,
           COALESCE(
               (SELECT sc.new_symbol
                  FROM symbol_changes sc
                 WHERE sc.workspace_id = $1
                   AND sc.old_symbol = m.symbol
                   AND sc.effective_date <= $3
                 ORDER BY sc.effective_date DESC
                 LIMIT 1),
               (SELECT sc.old_symbol
                  FROM symbol_changes sc
                 WHERE sc.workspace_id = $1
                   AND sc.new_symbol = m.symbol
                   AND sc.effective_date > $3
                 ORDER BY sc.effective_date ASC
                 LIMIT 1),
               m.symbol
           ) AS resolved_symbol
      FROM members m
),
delisted AS (
    SELECT mp.symbol,
           mp.resolved_symbol,
           (SELECT d.delisted_at
              FROM instrument_delistings d
             WHERE d.workspace_id = $1
               AND (d.symbol = mp.symbol OR d.symbol = mp.resolved_symbol)
               AND d.delisted_at <= $3
             ORDER BY d.delisted_at DESC
             LIMIT 1) AS delisted_prior_to_as_of,
           (SELECT d.delisted_at
              FROM instrument_delistings d
             WHERE d.workspace_id = $1
               AND (d.symbol = mp.symbol OR d.symbol = mp.resolved_symbol)
             ORDER BY d.delisted_at DESC
             LIMIT 1) AS actual_delisted_at
      FROM mapped mp
)
SELECT symbol,
       resolved_symbol,
       (delisted_prior_to_as_of IS NULL) AS is_active,
       actual_delisted_at AS delisted_at
  FROM delisted
 ORDER BY resolved_symbol ASC;
