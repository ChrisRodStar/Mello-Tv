# Catalog Benchmark Results Directory

This directory stores historical and baseline performance benchmarks for the Stream TV Catalog Service (`stv-catalog`).

## Benchmark Methodology

Benchmarks measure execution speed, memory consumption, and allocation rates using Go's standard benchmarking tooling (`go test -bench=. -benchmem`). All tests run deterministically with zero network I/O, ensuring consistent numbers across iterations.

### Core Metrics Measured

1. **Recommendation Engine (`internal/recommend`)**:
   - `BenchmarkInputs_Fingerprint`: SHA-256 canonical hashing across 50 viewing signals and 20 favorites.
   - `BenchmarkInputs_Seeds`: Half-life viewing signal decay and seed weighting calculation.
   - `BenchmarkRank_100Candidates`: Multi-signal scoring and ranking for 100 candidate items.
   - `BenchmarkRank_1000Candidates`: Stress-test scaling behavior across 1,000 candidates.
   - `BenchmarkDiversify_100Titles`: Franchise sequel spacing and anti-clustering algorithm.
   - `BenchmarkRecommendations_View`: Client category filtering and top-24 shelf preparation.

2. **Storage Tier (`internal/db`)**:
   - `BenchmarkStore_Get`: Single-key point-lookup in SQLite.
   - `BenchmarkStore_TitleEnriched`: Title query with external catalog cross-ID (`IMDb`, `MAL`) overlay.
   - `BenchmarkStore_Search`: Substring search over 1,000 cached records.
   - `BenchmarkStore_PutSingle`: Atomic single-record disk write with WAL mode.
   - `BenchmarkStore_PutBatch50`: Atomic multi-record transaction writing 50 items.
   - `BenchmarkStore_ExportSnapshot`: Point-in-time database snapshot export.

3. **HTTP API Presentation Layer (`internal/api`)**:
   - `BenchmarkHandler_HealthLive`: Raw HTTP pipeline and middleware throughput.
   - `BenchmarkHandler_HealthLive_WithAuth`: Constant-time bearer token authorization overhead.
   - `BenchmarkHandler_GetTitle`: End-to-end HTTP request including routing, token check, SQLite lookup, and JSON encoding.
   - `BenchmarkHandler_ParallelGetTitle`: Multi-core concurrent HTTP request throughput.

## Benchmark Runs

| Date | Run Directory | Architecture | Summary |
| :--- | :--- | :--- | :--- |
| 2026-10-19 | [2026-10-19-catalog-baseline](catalog/results/2026-10-19-catalog-baseline/report.md) | Apple M4 / Darwin arm64 | Modular catalog baseline: sub-microsecond HTTP routes, 16µs ranking, 6.9µs SQLite lookups |
