<div align="center">

# SQLite Database Architecture

**Storage engine, table schemas, concurrency models, and backup procedures.**

[Overview](#overview) &bull; [File Layout & WAL Mode](#file-layout-and-wal-mode) &bull; [Table Schemas](#table-schemas) &bull; [Safety & Concurrency](#safety-and-concurrency) &bull; [Backup & Recovery](#backup-and-recovery)

---

</div>

## Overview

The Stream TV Catalog uses **SQLite** as its dedicated persistent datastore.

Think of SQLite as a high-speed digital filing cabinet contained within a single file on disk (`catalog.sqlite`). 

Because SQLite is embedded directly into the Go catalog process:
- **Zero Network Lag:** Queries execute in microseconds without TCP network overhead.
- **Zero Maintenance Daemons:** There are no background database servers (like PostgreSQL or MySQL) to install, configure, or keep running.
- **Atomic Reliability:** If the Raspberry Pi suddenly loses power, SQLite's transactional journal guarantees that the database will not corrupt.

---

## File Layout and WAL Mode

When the Catalog is running, you will see up to three files in the database directory:

```text
data/
├── catalog.sqlite       # Main database file storing all tables and indexes
├── catalog.sqlite-wal   # Write-Ahead Log capturing new writes before they commit
└── catalog.sqlite-shm   # Shared memory index used for coordinating readers
```

### Why Write-Ahead Logging (WAL) Matters
Standard databases often lock the entire file during a write, which causes readers to pause and lag. 

The Catalog operates in **WAL mode** (`PRAGMA journal_mode = WAL`):
1. **Readers Never Block Writers:** When the API requests a movie details page, it reads instantly.
2. **Writers Never Block Readers:** When the background worker updates 50 movies from TMDB, browsing continues at full speed.
3. **Durable Synchronous Commits:** Commits use `PRAGMA synchronous = FULL` to ensure writes are flushed to physical disk before confirmation.
4. **Zero-Copy Kernel Memory Mapping:** Uses `PRAGMA mmap_size = 268435456` (256 MB) so the Raspberry Pi reads database pages directly from Linux kernel memory with zero-copy overhead.
5. **Generous Page Cache:** Allocates a 64 MB database page cache via `PRAGMA cache_size = -64000`.

---

## Table Schemas

The database schema is strictly versioned and automatically migrated when the service starts (current version: `2`).

### 1. The `records` Table
Stores all media records including title summaries, hydrated details, TV seasons, browsing shelves, external cross-IDs, and computed recommendation envelopes.

```sql
CREATE TABLE IF NOT EXISTS records (
    kind        TEXT NOT NULL,              -- 'title', 'season', 'shelf', 'cross_ids', 'inputs', 'recommendations'
    key         TEXT NOT NULL,              -- 'movie:550', 'tv:1399', 'tv:1399:1', 'for-you'
    data        BLOB NOT NULL,              -- Validated JSON payload
    updated     INTEGER NOT NULL,           -- Unix epoch millisecond timestamp of last refresh
    expires     INTEGER NOT NULL,           -- Unix epoch millisecond timestamp when refresh is due
    revision    INTEGER NOT NULL,           -- Global atomic revision counter
    PRIMARY KEY (kind, key)
);

-- Fast background expiration sweeps
CREATE INDEX IF NOT EXISTS records_expiry 
ON records (expires, kind);

-- Expression index for 4.4x faster case-insensitive substring title searches
CREATE INDEX IF NOT EXISTS records_title_search 
ON records (kind, lower(json_extract(data, '$.title')), key);
```

### 2. The `revision` Table
Tracks a monotonic, atomic integer revision sequence across the entire catalog datastore. Every batch update increments this counter, allowing clients and the API layer to detect changes instantly.

```sql
CREATE TABLE IF NOT EXISTS revision (
    value INTEGER NOT NULL
);
```

### 3. The `jobs` Table
Maintains durable, persistent background refresh jobs so queued updates survive server restarts and power outages.

```sql
CREATE TABLE IF NOT EXISTS jobs (
    key             TEXT PRIMARY KEY,       -- e.g., 'title/movie:550', 'browse', 'recommendations/alice'
    spec            BLOB NOT NULL,          -- Serialized JSON job specification
    due             INTEGER NOT NULL DEFAULT 0, -- Unix epoch millisecond timestamp when eligible
    last_success    INTEGER NOT NULL DEFAULT 0, -- Unix epoch millisecond timestamp of last success
    failed          INTEGER NOT NULL DEFAULT 0  -- 1 if failed (triggers 30-min exponential cooldown)
);

CREATE INDEX IF NOT EXISTS jobs_due 
ON jobs (due);
```

### 4. Cross-IDs Mapping
External IDs (IMDb IDs like `tt0137523` and MyAnimeList IDs like `55`) are stored directly in `records` under `kind = 'cross_ids'`. During title hydration, the catalog automatically enriches the title payload with these verified IDs while preventing TMDB refreshes from overwriting custom mappings.

---

## Safety & Concurrency

1. **Five-Second Busy Timeout:**
   If multiple internal routines attempt to write simultaneously, SQLite waits up to **5,000 milliseconds** (`PRAGMA busy_timeout = 5000`) before returning an error.
2. **Quota Enforcement:**
   A logical data payload limit (default: **128 Megabytes**) prevents runaway growth. If total stored JSON bytes exceed the limit, new write operations fail safely with HTTP `507` while existing data remains intact.
3. **Summary vs. Details Protection:**
   Lightweight discovery summaries (e.g. from general browsing shelves) are never allowed to overwrite or downgrade full hydrated detail records in the database.

---

## Backup & Recovery

### How to Take an Online Backup
Because the database uses WAL mode, you must never simply copy `catalog.sqlite` while the service is running (you would miss in-flight writes inside `-wal`).

The Catalog includes a built-in backup flag that uses SQLite's official online backup API (`VACUUM INTO`):

```bash
# Safely snapshot the live database into an isolated backup file
CATALOG_DB_PATH=data/catalog.sqlite ./.local/catalog -backup backups/catalog-snapshot.sqlite
```

This creates a consistent, standalone SQLite file with `0600` permissions that includes all committed data from both the database and the WAL log.

### Disaster Recovery Runbook
If the server's disk becomes corrupted or you need to restore an earlier snapshot:

1. **Stop the Catalog process:**
   ```bash
   kill -TERM $(pgrep catalog)
   ```
2. **Move corrupted files out of the way:**
   ```bash
   mv data/catalog.sqlite data/catalog.sqlite.bak
   rm -f data/catalog.sqlite-wal data/catalog.sqlite-shm
   ```
3. **Restore the snapshot:**
   ```bash
   cp backups/catalog-snapshot.sqlite data/catalog.sqlite
   chmod 600 data/catalog.sqlite
   ```
4. **Restart the Catalog process:**
   On startup, the Catalog will verify table integrity and automatically resume background refresh schedules.
