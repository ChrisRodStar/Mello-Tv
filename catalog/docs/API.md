<div align="center">

# Private HTTP Contract v1

**Internal REST API specification for title metadata, browsing shelves, and recommendations.**

[Overview](#overview) &bull; [Authentication](#authentication) &bull; [Record Structure](#record-structure) &bull; [Endpoints Summary](#endpoints-summary) &bull; [Endpoint Details](#endpoint-details) &bull; [Error Handling](#error-handling)

---

</div>

## Overview

The Catalog Service provides a private HTTP API used by the main Stream TV API (`stv-api`). 

All routes live under the `/internal/v1` prefix. These endpoints are designed for machine-to-machine communication on the private bridge network:
- **Offline First:** All `GET` requests read exclusively from local SQLite tables. They never call external websites or stall incoming user requests.
- **Strict Data Validation:** Unknown JSON fields in requests are rejected immediately.
- **Predictable Formatting:** All JSON properties use `snake_case`.

---

## Authentication

When `CATALOG_PRIVATE_TOKEN` is set, all requests must include a Bearer token in the HTTP Authorization header:

```http
Authorization: Bearer your-catalog-token-here
```

If the token is invalid or missing, the server responds with HTTP `401 Unauthorized`.
All token comparisons use cryptographic constant-time validation (`subtle.ConstantTimeCompare`) to prevent timing side-channel attacks.

---

## Server-Timing Telemetry

Every HTTP response from the catalog includes standard W3C `Server-Timing` headers:

```http
Server-Timing: db;dur=0.007, total;dur=0.021
```

- `db;dur=X.XXX`: Time spent executing the SQLite query in milliseconds (e.g. `0.007 ms` = `7 microseconds`).
- `total;dur=X.XXX`: Total elapsed request duration inside the catalog service in milliseconds.

This allows client inspection tools on your Mac Mini and the public API gateway to monitor internal Raspberry Pi datastore latency on every request without dedicated APM agents.

---

## Record Structure

When you request a movie, show, season, or shelf, the Catalog wraps the item inside a standardized **envelope**:

```json
{
  "kind": "title",
  "key": "movie:550",
  "data": {
    "type": "movie",
    "id": 550,
    "title": "Fight Club",
    "overview": "An ticking-time-bomb insomniac and a slippery soap salesman...",
    "date": "1999-10-15",
    "rating": 8.4,
    "poster": "/internal/v1/artwork/w500/bptfVGEQuv6vDTIMVCHjJ9Dz8PX.jpg",
    "backdrop": "/internal/v1/artwork/w1280/hZkgoQYus5vegHoetLkCJzb17zJ.jpg",
    "imdb_id": "tt0137523",
    "mal_id": null,
    "runtime": 139,
    "details": true
  },
  "updated_at": "2026-10-15T12:00:00Z",
  "expires_at": "2026-10-16T12:00:00Z",
  "revision": 1,
  "state": "fresh"
}
```

### Key Envelope Fields
- `kind`: What type of record this is (`title`, `season`, `shelf`, `recommendation`).
- `key`: The unique primary key (e.g., `movie:550`, `tv:1399`, `tv:1399:1`).
- `state`: Either `fresh` (recently verified) or `stale` (scheduled for background refresh). Stale records are still completely valid and readable.
- `details`: Boolean indicating whether full details (runtime, cast) are hydrated, or if this is just a quick browsing summary.

---

## Endpoints Summary

| Method | Path | Plain English Purpose |
| :--- | :--- | :--- |
| `GET` | `/health/live` | Checks if the Go catalog process is running. |
| `GET` | `/health/ready` | Checks if SQLite is ready and accepting requests. |
| `GET` | `/titles/{type}/{id}` | Returns title metadata (e.g., `/titles/movie/550`). |
| `PUT` | `/titles/{type}/{id}/cross-ids` | Saves external IMDb or MyAnimeList IDs for a title. |
| `GET` | `/tv/{id}/seasons/{season}` | Returns full episode list for a TV season. |
| `GET` | `/tv/{id}/seasons/{season}/episodes/{episode}` | Returns details for an exact TV episode. |
| `GET` | `/shelves` | Returns all available browsing shelves (Trending, Popular, etc.). |
| `GET` | `/shelves/{id}` | Returns titles inside a specific shelf (e.g., `/shelves/trending-movies`). |
| `GET` | `/search?q={query}` | Performs instant substring search over cached titles. |
| `GET` | `/search-results?q={query}` | Returns last known TMDB search results for that query. |
| `GET` | `/artwork/{size}/{filename}` | Redirects (HTTP 302) directly to TMDB's edge image CDN. |
| `POST`| `/refresh` | Queues a background update job for a title, shelf, or profile. |
| `GET` | `/refresh?key={job_key}` | Checks the current status of a queued refresh job. |
| `PUT` | `/recommendations/{handle}/inputs` | Submits viewing history and favorite signals for a user profile. |
| `GET` | `/recommendations/{handle}` | Fetches calculated recommendations for a user profile. |
| `DELETE`| `/recommendations/{handle}` | Cleans up cached recommendations when a profile is deleted. |
| `GET` | `/snapshot` | Exports a consistent, complete read snapshot of the entire catalog. |

---

## Endpoint Details

### 1. Title Lookup (`GET /titles/{type}/{id}`)
Fetches full details for a movie or TV show.

- **Request Example:**
  `GET /internal/v1/titles/movie/550`

- **Response (HTTP 200):**
  Returns the title envelope shown above in [Record Structure](#record-structure).

---

### 2. Season & Episode Details (`GET /tv/{id}/seasons/{season}`)
Fetches all episodes belonging to a specific season of a TV series.

- **Request Example:**
  `GET /internal/v1/tv/1399/seasons/1`

- **Response (HTTP 200):**
  ```json
  {
    "kind": "season",
    "key": "tv:1399:1",
    "data": {
      "show_id": 1399,
      "season": 1,
      "name": "Season 1",
      "episodes": [
        {
          "episode": 1,
          "name": "Winter Is Coming",
          "overview": "Lord Eddard Stark is torn between his family and an old friend...",
          "runtime": 62,
          "still": "/internal/v1/artwork/w300/wrGWeW4WKxnaeA8sxJb2T9OvgZ.jpg",
          "air_date": "2011-04-17"
        }
      ]
    },
    "state": "fresh"
  }
  ```

---

### 3. Local Title Search (`GET /search?q={query}&offset=0`)
Instantly searches titles stored in the local SQLite database.

- **Request Example:**
  `GET /internal/v1/search?q=Dark%20Knight&offset=0`

- **Response (HTTP 200):**
  ```json
  {
    "scope": "cached_titles",
    "offset": 0,
    "limit": 50,
    "records": [
      {
        "type": "movie",
        "id": 155,
        "title": "The Dark Knight",
        "date": "2008-07-16",
        "rating": 8.5,
        "poster": "/internal/v1/artwork/w500/qJ2tW6WMUDux911r6m7haRef0WH.jpg"
      }
    ]
  }
  ```

---

### 4. Background Refresh (`POST /refresh`)
Requests the background worker to fetch new data from TMDB.

- **Request Body Examples:**
  ```json
  // Request a movie refresh:
  { "kind": "title", "type": "movie", "id": 550 }

  // Request a TV season refresh:
  { "kind": "season", "type": "tv", "id": 1399, "season": 1 }

  // Request general shelf updates:
  { "kind": "browse" }
  ```

- **Response (HTTP 202 Accepted):**
  ```json
  {
    "status": "queued",
    "job_key": "title/movie:550"
  }
  ```

---

### 5. Profile Recommendations (`GET /recommendations/{handle}`)
Fetches personalized shelves calculated for an opaque user profile handle.

- **Request Example:**
  `GET /internal/v1/recommendations/profile-user-123?type=all`

- **Response (HTTP 200):**
  ```json
  {
    "kind": "recommendation",
    "key": "recommendations:profile-user-123",
    "data": {
      "fingerprint": "a3f5b7...",
      "sections": [
        {
          "id": "for-you",
          "heading": "Top Picks For You",
          "titles": [ ... ]
        },
        {
          "id": "because-you-watched",
          "heading": "Because You Watched Inception",
          "titles": [ ... ]
        }
      ]
    },
    "state": "fresh"
  }
  ```

---

## Error Handling

All error responses use standard HTTP status codes accompanied by a simple JSON body containing a stable machine-readable error string:

```json
{
  "error": "missing"
}
```

### Error Codes
- `400 Bad Request`: Invalid JSON syntax, unknown fields, or invalid numbers.
- `401 Unauthorized`: Missing or incorrect bearer token.
- `404 Not Found`: Title, season, episode, or recommendation is not stored locally.
- `503 Service Unavailable`: Database is busy, service is shutting down, or queue is full.
- `507 Insufficient Storage`: Logical storage budget exceeded.
