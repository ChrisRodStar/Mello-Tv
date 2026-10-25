# Providers Expansion Roadmap

Reference repositories:
- [CinePro Core Providers](https://github.com/cinepro-org/core/tree/main/src/providers)
- [CinePro Backend Providers](https://github.com/cinepro-org/backend/tree/main/src/controllers/providers)

---

## Active Providers (Native Go)
- [x] **VidSrc / VidCore** (`vidcore.net`)
  - Resolution: 1080p / 720p HLS
  - Reverse-engineered bytecode VM, linear congruential cipher, and dynamic script payload extraction.
- [x] **VidLink** (`vidlink.pro`)
  - Resolution: 1080p / 720p HLS
  - Subtitle track extraction, multi-tenant session hashing.
- [x] **VidFast** (`vidfast.vc`)
  - Resolution: 1080p / 720p HLS
  - Rapid parallel bundle resolver with zero external dependencies.
- [x] **Videasy** (`vidy.st` / `api.wecollege.net`)
  - Resolution: **2160p (4K UHD)** & 1080p / 720p HLS
  - Custom 61-element S-box keystream cipher with FNV-1a / Murmur3-style 32-bit avalanche hashing in pure Go.
  - Active server cluster: `boise` (4K UHD), `miami` (4K UHD), `phoenix` (1080p), `reno` (1080p worker proxy), `atlanta` (720p), `seattle`, `portland`, `vegas`, `oakland`.
  - Upstream headers: `Origin: https://vidy.st`, `Referer: https://vidy.st/`.
  - Verified live: The Matrix (TMDB 603 -> 2160p 4K UHD), Supernatural S01E10 (TMDB 1622 -> 1080p FHD).
  - Validation tool: `cmd/videasy-check`.

---

## Provider Import Queue

### Tier 1: High Quality & Primary Streaming
- [ ] **VixSrc** — 1080p FHD HLS, dual audio, subtitles
  - Reference: [cinepro-org/core/src/providers/vixsrc](https://github.com/cinepro-org/core/tree/main/src/providers/vixsrc)
  - Target: Next in priority
- [x] **Videasy** — 4K UHD & 1080p HLS multi-server (Complete)
- [ ] **2Embed** — Multi-server HLS, deep TV back-catalog
  - Reference: [cinepro-org/backend/src/controllers/providers/2Embed](https://github.com/cinepro-org/backend/tree/main/src/controllers/providers/2Embed)
- [ ] **MultiEmbed** — Multi-mirror aggregator mapped to TMDB/IMDb
  - Reference: [cinepro-org/backend/src/controllers/providers/MultiEmbed](https://github.com/cinepro-org/backend/tree/main/src/controllers/providers/MultiEmbed)
- [ ] **AnyEmbed (SmashyStream)** — SmashyStream backend stream API
  - Reference: [cinepro-org/core/src/providers/anyembed](https://github.com/cinepro-org/core/tree/main/src/providers/anyembed)

### Tier 2: Redundancy & Server Fallbacks
- [ ] **VidRock** — Multi-language HLS embed host
  - Reference: [cinepro-org/core/src/providers/vidrock](https://github.com/cinepro-org/core/tree/main/src/providers/vidrock)
- [ ] **VidNest** — Direct TV/movie stream hoster
  - Reference: [cinepro-org/core/src/providers/vidnest](https://github.com/cinepro-org/core/tree/main/src/providers/vidnest)
- [ ] **VidZee** — Embed scraper and player
  - Reference: [cinepro-org/core/src/providers/vidzee](https://github.com/cinepro-org/core/tree/main/src/providers/vidzee)
- [ ] **CinemaOS** — Multi-source player aggregator
  - Reference: [cinepro-org/backend/src/controllers/providers/CinemaOS](https://github.com/cinepro-org/backend/tree/main/src/controllers/providers/CinemaOS)
- [ ] **111Movies** — Lightweight streaming mirror
  - Reference: [cinepro-org/backend/src/controllers/providers/111movies](https://github.com/cinepro-org/backend/tree/main/src/controllers/providers/111movies)
- [ ] **xprime** — Direct movie and series stream scraper
  - Reference: [cinepro-org/backend/src/controllers/providers/xprime](https://github.com/cinepro-org/backend/tree/main/src/controllers/providers/xprime)

### Tier 3: Additional & Regional Mirrors
- [ ] **CineSu** — [cinepro-org/core/src/providers/cinesu](https://github.com/cinepro-org/core/tree/main/src/providers/cinesu)
- [ ] **StreamMafia** — [cinepro-org/core/src/providers/streammafia](https://github.com/cinepro-org/core/tree/main/src/providers/streammafia)
- [ ] **Icefy** — [cinepro-org/core/src/providers/icefy](https://github.com/cinepro-org/core/tree/main/src/providers/icefy)
- [ ] **Peachify** — [cinepro-org/core/src/providers/peachify](https://github.com/cinepro-org/core/tree/main/src/providers/peachify)
- [ ] **Movieapiclub** — [cinepro-org/backend/src/controllers/providers/Movieapiclub](https://github.com/cinepro-org/backend/tree/main/src/controllers/providers/Movieapiclub)
- [ ] **PrimeWire** — [cinepro-org/backend/src/controllers/providers/PrimeWire](https://github.com/cinepro-org/backend/tree/main/src/controllers/providers/PrimeWire)
- [ ] **Uembed** — [cinepro-org/backend/src/controllers/providers/Uembed](https://github.com/cinepro-org/backend/tree/main/src/controllers/providers/Uembed)
