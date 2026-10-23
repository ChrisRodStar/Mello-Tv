# Stream TV Catalog - Master Roadmap and Implementation Plan

This roadmap tracks the step-by-step audit, verification, and engineering improvements for the Stream TV Catalog service. Each component lives in its own folder inside `internal/` so you can work on one part at a time with total clarity.

---

### The Recommended Order of Execution

In software engineering, the best way to build and optimize is from the **bottom up** (dependency order):
1. **The Blueprints (`model/`)**: The shared data shapes that everyone relies on.
2. **The Filing Cabinet (`db/`)**: The database engine. Making reads fast here speeds up the entire application.
3. **The Web Messenger (`upstream/`)**: Talks to TMDB. Optimizing HTTP connections keeps memory low and network requests fast.
4. **The Recommendation Engine (`recommend/`)**: The brain of the service. Pre-allocating memory and preparing clean hooks for personalized discovery.
5. **The Background Robot (`service/`)**: The worker that coordinates database storage, TMDB syncing, and periodic sweeps.
6. **The Front Desk (`api/`)**: The private HTTP server. Adding `Server-Timing` headers so your Mac Mini can monitor Raspberry Pi latency.
7. **The Speed Inspector**: End-to-end benchmark comparison proving measurable speedups against our baseline.

---

### Step-by-Step Progress Tracker

- [x] **Step 1: The Starter Switch** ([cmd/catalog/main.go](catalog/cmd/catalog/main.go))
  - **Folder**: `cmd/catalog/`
  - **What it does**: Turns the service on, validates security tokens, handles the `--backup` command, and performs a clean shutdown when stopped.
  - **Status**: Audited line-by-line. Matches the legacy version with zero regressions.

- [x] **Step 2: Split and Organize Folders**
  - **What it does**: Moved files into dedicated subfolders (`model/`, `recommend/`, `db/`, `upstream/`, `service/`, and `api/`).
  - **Status**: Completed. Monolithic `internal/catalog` deleted. Clean isolated folders in place.

- [x] **Step 3: Baseline Performance Measurement** ([results/](catalog/results))
  - **What it does**: Ran comprehensive benchmarks on the Apple M4 to record exact latencies, memory allocations, and operations per second.
  - **Status**: Recorded in [results/2026-10-19-catalog-baseline/report.md](catalog/results/2026-10-19-catalog-baseline/report.md).

- [x] **Step 4: The Media Blueprints** ([internal/model/model.go](catalog/internal/model/model.go))
  - **Folder**: `internal/model/`
  - **Analogy**: The index card templates in a school library (defines what a Movie, TV Show, Season, and Episode look like).
  - **Optimization**:
    1. Replaced `fmt.Sprintf` with `strconv.Itoa` in `Identity.Key()` and `SeasonKey()`.
    2. Zero outside dependencies (only standard library).
  - **Status**: Completed and benchmarked. `BenchmarkIdentity_Key`: 17.07 ns/op (5 B/op, 1 alloc). `BenchmarkSeasonKey`: 25.80 ns/op (16 B/op, 2 allocs). Zero reflection overhead.

- [x] **Step 5: The Filing Cabinet** ([internal/db/store.go](catalog/internal/db/store.go))
  - **Folder**: `internal/db/`
  - **Analogy**: The steel filing cabinet where every title and shelf is saved to disk so nothing is lost when the Raspberry Pi restarts.
  - **Optimization**:
    1. **Search Indexing**: Added expression index `records_title_search` on `records(kind, lower(json_extract(data, '$.title')), key)` and used `INDEXED BY`. Cut search latency by 78% (from 722 µs to 159 µs).
    2. **Raspberry Pi Memory Mapping**: Configured `PRAGMA mmap_size = 268435456` and `PRAGMA cache_size = -64000` for zero-copy SQLite kernel memory mapping.
    3. **Fail-Forward Resilience**: Unconditional last-known-good cached record delivery under upstream degradation.
  - **Status**: Completed and benchmarked. Point lookups: 6.98 µs/op; Enriched titles: 21.23 µs/op; 1,000-title search: 159.51 µs/op (4.5x faster). All unit tests pass.

- [x] **Step 6: The Web Messenger** ([internal/upstream/upstream.go](catalog/internal/upstream/upstream.go))
  - **Folder**: `internal/upstream/`
  - **Analogy**: The messenger who bikes to the central library (TMDB) to fetch movie posters, descriptions, and air dates without getting ticketed for speeding.
  - **Optimization**:
    1. **Connection Keep-Alive Pooling**: Configured `http.Transport` with keep-alives (30s), idle connection pooling (20 total, 10 per host), and 90s idle timeout to eliminate repeated TLS handshakes.
    2. **Streaming JSON Reader**: Implemented `json.NewDecoder` streaming over `io.LimitReader` to eliminate 4 MB buffer allocations.
    3. **Jittered Backoff**: Added randomized exponential jitter (up to 5s) to rate limit retry backoffs to prevent thundering herds across concurrent workers.
  - **Status**: Completed and benchmarked. Streaming JSON round-trip: 32.38 µs/op; Artwork key validator: 156.1 ns/op (1 alloc). All unit tests pass.

- [x] **Step 7: The Recommendation Engine** ([internal/recommend/recommend.go](catalog/internal/recommend/recommend.go))
  - **Folder**: `internal/recommend/`
  - **Analogy**: The school librarian who knows your taste and suggests the perfect next book so you never stare at an empty shelf.
  - **Optimization**:
    1. **Memory Pre-allocation**: Pre-allocated slice capacities in `Fingerprint`, `Seeds`, `Rank`, and `DiversifyLimit`. Cut 100-candidate ranking latency from 16.42 µs to 9.84 µs (41% faster), and 1,000-candidate ranking from 193.03 µs to 111.28 µs (43% faster). Memory usage reduced by ~40% across ranking and diversification.
    2. **The 70/30 Serendipity Rule**: Implemented `SerendipityMix` combining 70% taste alignment with 30% discovery titles to prevent echo chambers (runs in 7.96 µs).
    3. **Sequel & Franchise Spacing**: Verified `Family()` prefix and subtitle extraction prevents sequel clustering on general shelves while preserving next episode ordering.
    4. **Contextual Shelves**: Implemented `ContextualShelf()` for dynamic "Because you watched..." shelves.
  - **Status**: Completed and benchmarked. Ranking: 9.84 µs/op (100 items), 111.28 µs/op (1000 items); Seeds: 5.45 µs/op; SerendipityMix: 7.96 µs/op. All 10 tests pass.

- [x] **Step 8: The Background Robot** ([internal/service/service.go](catalog/internal/service/service.go))
  - **Folder**: `internal/service/`
  - **Analogy**: The night custodian who silently sweeps the floors, reorganizes books, and updates the display shelves while everyone is asleep.
  - **Optimization**:
    1. Worker queue admission tuning (< 32 refill threshold) ensuring workers never idle.
    2. Non-blocking periodic sweeps (15-minute intervals) with durable SQLite job recovery and TryLock sweep mutex.
    3. Graceful worker draining with bound context shutdown.
  - **Status**: Completed and benchmarked. Job validation: 2.23 ns/op (zero allocs); Job keying: 30.86 ns/op (2 allocs); Deduplicated request check: 9.36 µs/op. All unit tests pass.

- [x] **Step 9: The Front Desk** ([internal/api/handler.go](catalog/internal/api/handler.go))
  - **Folder**: `internal/api/`
  - **Analogy**: The front reception desk that checks student IDs, answers questions, and hands out requested books.
  - **Optimization**:
    1. **Server-Timing Header**: Added `timingWriter` middleware injecting `Server-Timing: total;dur=...` and endpoint-specific `db;dur=...` into HTTP response headers so the Mac Mini client can inspect Raspberry Pi latency per request.
    2. **Constant-Time Security**: Bearer token authentication validated via `subtle.ConstantTimeCompare` with zero timing-leak vulnerabilities.
  - **Status**: Completed and benchmarked. Live health: 993.1 ns/op (1.00M req/sec); Authenticated health: 1.11 µs/op; Full End-to-End title lookup: 24.05 µs/op (41,580 req/sec) with active Server-Timing headers. All unit tests pass.

- [x] **Step 10: Comparison Benchmark Run & Evidence**
  - **What it does**: Re-ran the full benchmark suite across all subsystems, verified measurable speedups against the baseline, and recorded results in [results/2026-10-19-catalog-optimized/report.md](catalog/results/2026-10-19-catalog-optimized/report.md).
  - **Status**: Completed. Title search: 4.4x faster (722 µs -> 162 µs); 100-candidate recommendation: 40% faster (16.4 µs -> 9.8 µs); Memory allocations cut in half across recommendations; 100% tests pass.
