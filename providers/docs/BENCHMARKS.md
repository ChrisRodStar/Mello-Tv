<!--
  BENCHMARKS.md
  Stream TV Providers Service

  Summary:
  Baseline benchmark results recorded prior to repository migration, documenting
  adapter resolution latencies, streaming throughput, and selection outcomes
  across all four upstream providers.
-->

# Baseline Benchmark Results

This document records the baseline performance and stream resolution results of the legacy Provider service before beginning the code migration to the new repository. These measurements provide a direct reference point to verify performance, stream quality, and error behavior after migration.

---

## Test Environment

- **Host Machine:** Apple Mac mini (Apple Silicon M1)
- **Date Tested:** October 20, 2026
- **Network Interface:** Mullvad VPN WireGuard tunnel (`utun12`)
- **VPN Server:** Raleigh, NC (`us-rag-wg-203`)
- **Public Exit IP:** `23.234.77.99`
- **Routing Security:** Fail-closed OS socket binding (`IP_BOUND_IF`)
- **Probing Policy:** 3-point video segment sampling (beginning, middle, and end)

---

## Test Titles

Five distinct media titles were selected across different entertainment categories:

1. **TV Drama:** *Supernatural* — Season 1, Episode 1 (TMDB ID: `1622`)
2. **Feature Film:** *The Conjuring* (2013) (TMDB ID: `138843`)
3. **Anime:** *Dragon Ball Super* — Season 1, Episode 1 (TMDB ID: `62715`)
4. **Cartoon (Adult Animation):** *Rick and Morty* — Season 1, Episode 1 (TMDB ID: `60625`)
5. **Cartoon (Long-Running Animation):** *The Simpsons* — Season 1, Episode 1 (TMDB ID: `456`)

---

## Individual Provider Resolution Times

Each provider was queried independently through the active VPN tunnel. The table below lists the response latency and the outcome for each provider.

| Title | VidCore | VidLink | VidSrc | VidFast |
| :--- | :--- | :--- | :--- | :--- |
| **Supernatural S01E01** | Protocol Change (1,248 ms) | Success (849 ms, 1 source) | Success (1,913 ms, 3 sources) | Empty (1,546 ms, 0 sources) |
| **The Conjuring (2013)** | Protocol Change (860 ms) | Success (485 ms, 1 source) | Success (2,760 ms, 3 sources) | Empty (1,644 ms, 0 sources) |
| **Dragon Ball Super S01E01** | Protocol Change (873 ms) | Not Found (1,134 ms, 0 sources) | Success (2,707 ms, 3 sources) | Empty (1,583 ms, 0 sources) |
| **Rick and Morty S01E01** | Protocol Change (1,125 ms) | Success (407 ms, 1 source) | Success (2,108 ms, 3 sources) | Success (1,664 ms, 3 sources) |
| **The Simpsons S01E01** | Protocol Change (1,190 ms) | Success (777 ms, 1 source) | Success (2,182 ms, 3 sources) | Success (1,729 ms, 3 sources) |

---

## Full Pipeline Resolution and Probing Results

The full pipeline runs all adapters concurrently, dedupes candidates, downloads three media segments per candidate to measure real throughput and latency, and ranks candidates using the 6-tier selection policy.

| Title | Winning Provider | Stream Format | Resolution | Probed Bandwidth | Segment TTFB | Total Pipeline Time | Outcome |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Supernatural S01E01** | VidLink | HLS (`.m3u8`) | 720p | 9.20 Mbps | 512 ms | 8.35 s | Stream Selected |
| **The Conjuring (2013)** | None | HLS (`.m3u8`) | 1080p | Filtered | Filtered | 9.38 s | Headroom Check Failed |
| **Dragon Ball Super S01E01** | None | HLS (`.m3u8`) | 1080p | Filtered | Filtered | 2.71 s | Audio Language Filtered |
| **Rick and Morty S01E01** | VidFast | HLS (`.m3u8`) | 1080p | 17.92 Mbps | 255 ms | 7.64 s | Stream Selected |
| **The Simpsons S01E01** | VidFast | HLS (`.m3u8`) | 1080p | 20.46 Mbps | 636 ms | 7.04 s | Stream Selected |

---

## Detailed Provider Analysis

### 1. VidCore (Status: Broken / Protocol Change)
- **Latency:** 860 ms to 1,248 ms
- **Root Cause:** The upstream website (`vidcore.io`) updated its player JavaScript code bundle on its servers. The Go adapter has a built-in safety check that computes the SHA-256 hash of the player code before running extraction steps. Because the remote hash did not match the expected hash in the Go code, the adapter stopped immediately and reported `protocol_change`.
- **Safety Impact:** This failure mode worked as intended. The adapter failed closed instead of sending corrupted requests, crashing the service, or returning invalid video URLs.
- **Required Fix:** Update the VidCore adapter chunk hash, inspect the updated player code for changes in encryption keys or endpoints, and update the Go decoding routines.

### 2. VidLink (Status: Working, Fastest Latency)
- **Latency:** 407 ms to 849 ms
- **Performance:** VidLink is the fastest provider in the fleet. For Western movies and popular TV shows, it resolved candidates in under one second.
- **Content Coverage:** Strong for American live-action shows and movies, but failed to locate Japanese anime (*Dragon Ball Super* returned Not Found).
- **Stream Quality:** Delivered high-bitrate progressive and HLS streams hosted on `mwVault` infrastructure. Won the selection round for *Supernatural*.

### 3. VidSrc (Status: Working, Broadest Catalog)
- **Latency:** 1,913 ms to 2,760 ms
- **Performance:** Slower resolution speed due to a multi-stage redirect and decryption challenge, but consistent across all tests.
- **Content Coverage:** VidSrc was the only provider to successfully locate streams for all five titles, including *Dragon Ball Super*.
- **Stream Delivery:** Provided three stream mirrors per title, backed by `willowispwoodshop.space`.

### 4. VidFast (Status: Working for Animation, Highest Bitrate)
- **Latency:** 1,546 ms to 1,729 ms
- **Performance:** Resolved three server mirrors (`vRapid`, `vBlaze`, and `Cobra`) for animated series. Did not return sources for live-action titles (*Supernatural* and *The Conjuring*).
- **Stream Quality:** VidFast delivered the highest visual quality and bandwidth in the benchmark suite:
  - *Rick and Morty:* 1080p at 17.92 Mbps (255 ms first-segment TTFB)
  - *The Simpsons:* 1080p at 20.46 Mbps (636 ms first-segment TTFB)
- In both animation tests, VidFast outranked VidLink and VidSrc due to its verified 1080p stream bitrates and low segment latencies.

---

## Pipeline Filtering Observations

1. **Dragon Ball Super Audio Filter:**
   VidSrc successfully discovered three working 1080p streams. However, the client request specified English audio preference. VidSrc only provided Japanese audio with English subtitles. The selection engine filtered these candidates out because language requirements were not met.

2. **The Conjuring Bandwidth Headroom Filter:**
   Candidates were discovered by both VidLink and VidSrc. During the 3-point segment probing step, the measured download speeds through the test VPN tunnel did not meet the 1.5x bandwidth headroom requirement for the selected bitrate profile. The engine rejected the streams rather than returning a link that would buffer during playback.

---

## Migration Verification Goals

When the Provider service migration is completed in the new repository, testing against this baseline will verify:

1. VidLink, VidSrc, and VidFast maintain or improve their resolution latencies (sub-1 second for VidLink, sub-2.5 seconds for VidFast and VidSrc).
2. The 3-point media prober accurately downloads segments and calculates bitrates matching the 17–20 Mbps throughput observed on VidFast.
3. The selection engine applies identical deterministic ranking and eligibility filters.
4. Fail-closed socket binding continues to block all non-VPN network traffic.
