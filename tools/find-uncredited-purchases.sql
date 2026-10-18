-- Charges with no delivery: a transaction that took currency and logged no grant at all.
--
-- inventorylog is append-only (migration 0010 adds triggers refusing UPDATE and DELETE), so this is
-- the authoritative record of what each account was actually charged. A purchase writes its debit and
-- its grant under one transactionId, so a row here means the debit committed on its own.
--
-- Read-only. Run against a COPY of undaunted.db, not the live file:
--   sqlite3 backups/<date>/undaunted.db ".read tools/find-uncredited-purchases.sql"

.mode column
.headers on

SELECT
    debit.time,
    debit.userId,
    debit.characterId,
    debit.caller,
    debit.source,
    debit.catalogId                AS currency,
    -debit.quantityChange          AS charged,
    debit.quantityAfter            AS balance_after,
    debit.transactionId
FROM inventorylog AS debit
WHERE debit.operation = 'remove'
  AND debit.catalogId LIKE 'CURRENCY_%'
  AND debit.quantityChange < 0
  AND debit.transactionId IS NOT NULL
  AND NOT EXISTS (
      SELECT 1
      FROM inventorylog AS grant_row
      WHERE grant_row.transactionId = debit.transactionId
        AND grant_row.characterId   = debit.characterId
        AND grant_row.operation IN ('add', 'save')
  )
ORDER BY debit.time DESC;

-- Totals per account, for working out what to return.
SELECT
    debit.userId,
    debit.catalogId                AS currency,
    COUNT(*)                       AS charges,
    SUM(-debit.quantityChange)     AS total_charged,
    MIN(debit.time)                AS first_seen,
    MAX(debit.time)                AS last_seen
FROM inventorylog AS debit
WHERE debit.operation = 'remove'
  AND debit.catalogId LIKE 'CURRENCY_%'
  AND debit.quantityChange < 0
  AND debit.transactionId IS NOT NULL
  AND NOT EXISTS (
      SELECT 1
      FROM inventorylog AS grant_row
      WHERE grant_row.transactionId = debit.transactionId
        AND grant_row.characterId   = debit.characterId
        AND grant_row.operation IN ('add', 'save')
  )
GROUP BY debit.userId, debit.catalogId
ORDER BY total_charged DESC;
