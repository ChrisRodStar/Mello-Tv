//! Bounded TMDB catalog population and profile-keyed recommendations.

use std::{env, fmt, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use stream_core::{
    accounts::Profile,
    catalog::{
        CatalogRecord, RecommendationInputs, Recommendations, Season, SeasonSummary, Title,
        TitleIdentity,
    },
};
use stream_storage::{CatalogStore, catalog::CatalogWrite};
use tokio::{
    sync::{Mutex, Semaphore},
    time::Instant,
};

const META_TTL: chrono::Duration = chrono::Duration::days(14);
const IMAGE_BASE: &str = "https://image.tmdb.org/t/p";

#[derive(Debug, thiserror::Error)]
pub enum CatalogServiceError {
    #[error("catalog persistence failed")]
    Storage(#[from] stream_storage::StorageError),
    #[error("catalog response was invalid")]
    InvalidResponse,
    #[error("catalog upstream request failed")]
    Upstream,
    #[error("TMDB rejected the configured credential")]
    CredentialRejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    Fresh,
    Stale,
    Unavailable,
}

#[derive(Debug, Clone)]
pub struct CatalogLookup<T> {
    pub value: Option<T>,
    pub availability: Availability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationStatus {
    Ready,
    Unavailable,
}

/// Explicit per-user TMDB credentials and optional mock URL. Debug output is redacted.
pub struct CatalogWorkerConfig {
    bearer_token: Option<String>,
    api_key: Option<String>,
    base_url: String,
}

impl CatalogWorkerConfig {
    pub fn new(bearer_token: Option<String>, api_key: Option<String>) -> Self {
        Self {
            bearer_token,
            api_key,
            base_url: "https://api.themoviedb.org/3".into(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

impl fmt::Debug for CatalogWorkerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CatalogWorkerConfig")
            .field("credentials", &"[REDACTED]")
            .field("base_url", &"[configured]")
            .finish()
    }
}

#[derive(Clone)]
pub struct CatalogWorker {
    store: Arc<CatalogStore>,
    client: Client,
    token: Option<Arc<str>>,
    api_key: Option<Arc<str>>,
    base_url: Arc<str>,
    permits: Arc<Semaphore>,
    warm_lock: Arc<Mutex<()>>,
    rate: Arc<Mutex<RateLimit>>,
}

struct RateLimit {
    next_request: Instant,
    blocked_until: Option<Instant>,
}

impl fmt::Debug for CatalogWorker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CatalogWorker")
            .field("configured", &self.available())
            .finish_non_exhaustive()
    }
}

impl CatalogWorker {
    pub fn from_env(store: Arc<CatalogStore>) -> Result<Self, CatalogServiceError> {
        let token = env::var("TMDB_READ_ACCESS_TOKEN")
            .ok()
            .or_else(|| env::var("TMDB_ACCESS_TOKEN").ok())
            .filter(|v| !v.trim().is_empty());
        let api_key = env::var("TMDB_API_KEY")
            .ok()
            .filter(|v| !v.trim().is_empty());
        Self::with_config(store, CatalogWorkerConfig::new(token, api_key))
    }

    pub fn with_config(
        store: Arc<CatalogStore>,
        config: CatalogWorkerConfig,
    ) -> Result<Self, CatalogServiceError> {
        if config.base_url.len() > 4096
            || config.bearer_token.as_ref().is_some_and(|value| {
                value.len() > 8192 || value.bytes().any(|b| b.is_ascii_control())
            })
            || config.api_key.as_ref().is_some_and(|value| {
                value.len() > 512 || value.bytes().any(|b| b.is_ascii_control())
            })
        {
            return Err(CatalogServiceError::InvalidResponse);
        }
        let parsed = reqwest::Url::parse(&config.base_url)
            .map_err(|_| CatalogServiceError::InvalidResponse)?;
        let host = parsed.host_str().unwrap_or_default();
        let is_loopback = host == "localhost" || host == "127.0.0.1" || host == "::1";
        if parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || !matches!(parsed.scheme(), "http" | "https")
            || (!is_loopback && parsed.scheme() != "https")
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(CatalogServiceError::InvalidResponse);
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(12))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("StreamTV/1.0")
            .build()
            .map_err(|_| CatalogServiceError::Upstream)?;
        Ok(Self {
            store,
            client,
            token: config
                .bearer_token
                .filter(|v| !v.trim().is_empty())
                .map(|value| Arc::<str>::from(value.trim())),
            api_key: config
                .api_key
                .filter(|v| !v.trim().is_empty())
                .map(|value| Arc::<str>::from(value.trim())),
            base_url: Arc::from(config.base_url.trim_end_matches('/')),
            permits: Arc::new(Semaphore::new(4)),
            warm_lock: Arc::new(Mutex::new(())),
            rate: Arc::new(Mutex::new(RateLimit {
                next_request: Instant::now(),
                blocked_until: None,
            })),
        })
    }

    #[cfg(test)]
    fn for_mock(store: Arc<CatalogStore>, base_url: &str) -> Self {
        Self {
            store,
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap_or_default(),
            token: Some(Arc::from("mock-token")),
            api_key: None,
            base_url: Arc::from(base_url),
            permits: Arc::new(Semaphore::new(2)),
            warm_lock: Arc::new(Mutex::new(())),
            rate: Arc::new(Mutex::new(RateLimit {
                next_request: Instant::now(),
                blocked_until: None,
            })),
        }
    }

    /// Inject an alternate upstream for local mock servers. Production defaults to TMDB.
    pub fn with_base_url(mut self, base_url: &str) -> Result<Self, CatalogServiceError> {
        let parsed =
            reqwest::Url::parse(base_url).map_err(|_| CatalogServiceError::InvalidResponse)?;
        let host = parsed.host_str().unwrap_or_default();
        let loopback = host == "localhost" || host == "127.0.0.1" || host == "::1";
        if parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || !matches!(parsed.scheme(), "http" | "https")
            || (!loopback && parsed.scheme() != "https")
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(CatalogServiceError::InvalidResponse);
        }
        self.base_url = Arc::from(base_url.trim_end_matches('/'));
        Ok(self)
    }

    pub fn available(&self) -> bool {
        self.token.is_some() || self.api_key.is_some()
    }

    /// Returns an existing real catalog or the current popular metadata page.
    pub async fn browse(
        &self,
    ) -> Result<CatalogLookup<Vec<CatalogRecord<Title>>>, CatalogServiceError> {
        let now: DateTime<Utc> = std::time::SystemTime::now().into();
        let cached = self.store.list("title", 50, now).await?;
        if !self.available() {
            if !cached.is_empty() {
                let fresh = cached.iter().all(|record| record.expires_at > now);
                let titles = cached
                    .into_iter()
                    .filter_map(|r| {
                        serde_json::from_value(r.data.clone())
                            .ok()
                            .map(|data| typed(r, data))
                    })
                    .collect();
                return Ok(CatalogLookup {
                    value: Some(titles),
                    availability: if fresh {
                        Availability::Fresh
                    } else {
                        Availability::Stale
                    },
                });
            }
            return Ok(CatalogLookup {
                value: None,
                availability: Availability::Unavailable,
            });
        }
        let _guard = self.warm_lock.lock().await;
        if let Err(error) = self.generate_browse(&[]).await {
            if !cached.is_empty() {
                let titles = cached
                    .into_iter()
                    .filter_map(|record| {
                        serde_json::from_value(record.data.clone())
                            .ok()
                            .map(|title| typed(record, title))
                    })
                    .collect();
                return Ok(CatalogLookup {
                    value: Some(titles),
                    availability: Availability::Stale,
                });
            }
            return Err(error);
        }
        let refreshed = self
            .store
            .list("title", 50, std::time::SystemTime::now().into())
            .await?;
        let records = refreshed
            .into_iter()
            .filter_map(|record| {
                serde_json::from_value(record.data.clone())
                    .ok()
                    .map(|title| typed(record, title))
            })
            .collect();
        Ok(CatalogLookup {
            value: Some(records),
            availability: Availability::Fresh,
        })
    }

    /// Profile creation is the first point at which metadata generation is started.
    /// Callers can detach this future and track their own request lifecycle.
    pub async fn warm_profile(
        &self,
        profile: Profile,
    ) -> Result<GenerationStatus, CatalogServiceError> {
        if !self.available() {
            return Ok(GenerationStatus::Unavailable);
        }
        let fingerprint = profile_generation_fingerprint(&profile)?;
        let _guard = self.warm_lock.lock().await;
        let now: DateTime<Utc> = std::time::SystemTime::now().into();
        if let Ok(record) = self.store.get("profile_generation", &profile.id, now).await
            && record.expires_at > now
            && record.data.get("fingerprint").and_then(Value::as_str) == Some(fingerprint.as_str())
        {
            return Ok(GenerationStatus::Ready);
        }
        self.generate_browse(&profile.preferences.genres).await?;
        let marker = json!({"fingerprint": fingerprint, "completed_at": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| CatalogServiceError::InvalidResponse)?.as_secs()});
        self.save(
            "profile_generation",
            &profile.id,
            &marker,
            chrono::Duration::days(30),
        )
        .await?;
        Ok(GenerationStatus::Ready)
    }

    async fn discover_path(&self, genre: &str, media: &str) -> Result<String, CatalogServiceError> {
        let genre_ids = match (genre, media) {
            ("action", "movie") => Some("28|12"),
            ("action", "tv") => Some("10759"),
            ("comedy", "movie" | "tv") => Some("35"),
            ("drama", "movie" | "tv") => Some("18"),
            ("crime", "movie" | "tv") => Some("80|9648"),
            ("scifi", "movie") => Some("878|14"),
            ("scifi", "tv") => Some("10765"),
            ("animation", "movie" | "tv") => Some("16"),
            ("documentary", "movie" | "tv") => Some("99"),
            ("horror", "movie") => Some("27"),
            ("family", "movie" | "tv") => Some("10751"),
            ("horror", "tv") => None,
            _ => return Err(CatalogServiceError::InvalidResponse),
        };
        let mut filters = vec![
            "include_adult=false".to_owned(),
            "sort_by=popularity.desc".to_owned(),
            "vote_count.gte=50".to_owned(),
            "language=en-US".to_owned(),
        ];
        if let Some(genre_ids) = genre_ids {
            filters.push(format!("with_genres={}", urlencoding(genre_ids)));
        } else {
            let keyword = self.horror_keyword_id().await?;
            filters.push(format!("with_keywords={keyword}"));
        }
        if genre == "animation" {
            filters.push("with_original_language=ja".into());
        }
        let date_field = if media == "tv" {
            "first_air_date"
        } else {
            "primary_release_date"
        };
        let now: DateTime<Utc> = std::time::SystemTime::now().into();
        filters.push(format!("{date_field}.lte={}", now.format("%Y-%m-%d")));
        Ok(format!("/discover/{media}?{}", filters.join("&")))
    }

    async fn horror_keyword_id(&self) -> Result<i64, CatalogServiceError> {
        let now: DateTime<Utc> = std::time::SystemTime::now().into();
        if let Ok(record) = self.store.get("keyword", "horror", now).await
            && cache_ttl_acceptable(&record)
            && record.expires_at > now
            && record
                .data
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| name.eq_ignore_ascii_case("horror"))
            && let Some(id) = record
                .data
                .get("id")
                .and_then(Value::as_i64)
                .filter(|id| *id > 0)
        {
            return Ok(id);
        }
        let response = self.get_json("/search/keyword?query=horror&page=1").await?;
        let id = response
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|item| {
                item.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name.eq_ignore_ascii_case("horror"))
            })
            .and_then(|item| item.get("id").and_then(Value::as_i64))
            .filter(|id| *id > 0)
            .ok_or(CatalogServiceError::Upstream)?;
        self.save(
            "keyword",
            "horror",
            &json!({"id": id, "name": "Horror"}),
            chrono::Duration::days(30),
        )
        .await?;
        Ok(id)
    }

    async fn generate_browse(&self, genres: &[String]) -> Result<(), CatalogServiceError> {
        let selected = if genres.is_empty() {
            vec![
                "action",
                "comedy",
                "drama",
                "crime",
                "scifi",
                "animation",
                "documentary",
                "horror",
                "family",
            ]
        } else {
            genres.iter().map(String::as_str).collect()
        };
        // Populate the same real, profile-independent onboarding shelves used by the UI.
        for genre in selected.into_iter().take(9) {
            for (media, label) in [("movie", "Movies"), ("tv", "TV")] {
                let shelf_id = format!("favorites-{media}-{genre}");
                let now: DateTime<Utc> = std::time::SystemTime::now().into();
                let favorite_is_fresh = self
                    .store
                    .get("shelf", &shelf_id, now)
                    .await
                    .is_ok_and(|record| cache_ttl_acceptable(&record) && record.expires_at > now);
                let onboarding_id = format!("onboarding-{genre}");
                let onboarding_is_fresh = media != "movie"
                    || self
                        .store
                        .get("shelf", &onboarding_id, now)
                        .await
                        .is_ok_and(|record| {
                            cache_ttl_acceptable(&record) && record.expires_at > now
                        });
                if favorite_is_fresh && onboarding_is_fresh {
                    continue;
                }
                let path = self.discover_path(genre, media).await?;
                let payload = self.get_json(&path).await?;
                let mut titles = Vec::new();
                if let Some(results) = payload.get("results").and_then(Value::as_array) {
                    for raw in results.iter().take(20) {
                        if let Some(title) = normalize_title(raw) {
                            self.save_title(&title).await?;
                            titles.push(title);
                        }
                    }
                }
                if !titles.is_empty() {
                    let heading = format!("{} {label}", genre_label(genre));
                    let onboarding_titles = titles.clone();
                    self.save(
                        "shelf",
                        &shelf_id,
                        &stream_core::catalog::Shelf {
                            id: shelf_id.clone(),
                            heading,
                            kind: "browse".into(),
                            source: "tmdb-discover".into(),
                            reason: String::new(),
                            titles,
                        },
                        META_TTL,
                    )
                    .await?;
                    if media == "movie" {
                        let onboarding_id = format!("onboarding-{genre}");
                        self.save(
                            "shelf",
                            &onboarding_id,
                            &stream_core::catalog::Shelf {
                                id: onboarding_id.clone(),
                                heading: genre_label(genre).into(),
                                kind: "browse".into(),
                                source: "tmdb-discover".into(),
                                reason: String::new(),
                                titles: onboarding_titles,
                            },
                            META_TTL,
                        )
                        .await?;
                    }
                } else {
                    return Err(CatalogServiceError::Upstream);
                }
            }
        }
        // Refresh each home shelf independently so one fresh shelf cannot mask missing data.
        self.refresh_home_shelf(
            "trending",
            "Trending this week",
            "/trending/all/week?page=1",
            "trending",
            "tmdb-weekly",
        )
        .await?;
        self.refresh_home_shelf(
            "movie-popular",
            "Popular movies",
            "/movie/popular?page=1&language=en-US",
            "browse",
            "tmdb-popular",
        )
        .await?;
        self.refresh_home_shelf(
            "tv-popular",
            "Popular TV shows",
            "/tv/popular?page=1&language=en-US",
            "browse",
            "tmdb-popular",
        )
        .await?;
        Ok(())
    }

    async fn refresh_home_shelf(
        &self,
        id: &str,
        heading: &str,
        path: &str,
        kind: &str,
        source: &str,
    ) -> Result<(), CatalogServiceError> {
        let now: DateTime<Utc> = std::time::SystemTime::now().into();
        if self
            .store
            .get("shelf", id, now)
            .await
            .is_ok_and(|record| cache_ttl_acceptable(&record) && record.expires_at > now)
        {
            return Ok(());
        }
        let payload = self.get_json(path).await?;
        let titles: Vec<Title> = payload
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(20)
            .filter_map(normalize_title)
            .collect();
        if titles.is_empty() {
            return Err(CatalogServiceError::Upstream);
        }
        for title in &titles {
            self.save_title(title).await?;
        }
        let shelf = stream_core::catalog::Shelf {
            id: id.into(),
            heading: heading.into(),
            kind: kind.into(),
            source: source.into(),
            reason: String::new(),
            titles,
        };
        self.save("shelf", id, &shelf, META_TTL).await?;
        Ok(())
    }

    pub async fn search(
        &self,
        query: &str,
        offset: u32,
    ) -> Result<CatalogLookup<Vec<CatalogRecord<Title>>>, CatalogServiceError> {
        let now: DateTime<Utc> = std::time::SystemTime::now().into();
        let cache_key = search_cache_key(query, offset);
        let cached = self.store.get("search", &cache_key, now).await.ok();
        if let Some(record) = &cached
            && record.expires_at > now
            && let Ok(titles) = serde_json::from_value::<Vec<Title>>(record.data.clone())
        {
            return Ok(CatalogLookup {
                value: Some(
                    titles
                        .into_iter()
                        .map(|title| {
                            fresh_record(
                                "title",
                                title.identity.key(),
                                title,
                                record.updated_at,
                                record.revision,
                            )
                        })
                        .collect(),
                ),
                availability: Availability::Fresh,
            });
        }
        let existing = self.store.search(query, offset, now).await?;
        if !self.available() {
            if let Some(lookup) = stale_search_lookup(cached, existing, now) {
                return Ok(lookup);
            }
            return Ok(CatalogLookup {
                value: None,
                availability: Availability::Unavailable,
            });
        }
        let page = offset / 20 + 1;
        let payload = self
            .get_json(&format!(
                "/search/multi?query={}&page={page}",
                urlencoding(query)
            ))
            .await;
        let payload = match payload {
            Ok(payload) => payload,
            Err(error) => {
                if let Some(lookup) = stale_search_lookup(cached, existing, now) {
                    return Ok(lookup);
                } else {
                    return Err(error);
                }
            }
        };
        let results = payload
            .get("results")
            .and_then(Value::as_array)
            .ok_or(CatalogServiceError::InvalidResponse)?;
        let mut titles = Vec::new();
        for raw in results.iter().take(20) {
            if let Some(title) = normalize_title(raw) {
                self.save_title(&title).await?;
                titles.push(title);
            }
        }
        let search_value =
            serde_json::to_value(&titles).map_err(|_| CatalogServiceError::InvalidResponse)?;
        self.store
            .put(
                CatalogWrite {
                    kind: "search".into(),
                    key: cache_key,
                    data: search_value,
                    ttl: chrono::Duration::hours(1),
                },
                now,
            )
            .await?;
        Ok(CatalogLookup {
            value: Some(
                titles
                    .into_iter()
                    .map(|t| fresh_record("title", t.identity.key(), t, now, 0))
                    .collect(),
            ),
            availability: Availability::Fresh,
        })
    }

    pub async fn title(
        &self,
        identity: TitleIdentity,
    ) -> Result<CatalogLookup<CatalogRecord<Title>>, CatalogServiceError> {
        let now: DateTime<Utc> = std::time::SystemTime::now().into();
        let key = identity.key();
        let stale = self.store.title(&key, now).await.ok();
        if let Some(record) = &stale
            && record.expires_at > now
            && let Ok(data) = serde_json::from_value::<Title>(record.data.clone())
            && data.details
            && cache_ttl_acceptable(record)
        {
            return Ok(CatalogLookup {
                value: Some(typed(record.clone(), data)),
                availability: Availability::Fresh,
            });
        }
        if !self.available() {
            if let Some(record) = stale
                && let Ok(data) = serde_json::from_value(record.data.clone())
            {
                return Ok(CatalogLookup {
                    value: Some(typed(record, data)),
                    availability: Availability::Stale,
                });
            }
            return Ok(CatalogLookup {
                value: None,
                availability: Availability::Unavailable,
            });
        }
        let kind = media_path(identity);
        let raw = match self
            .get_json(&format!(
                "/{kind}/{}?append_to_response=external_ids",
                identity.id()
            ))
            .await
        {
            Ok(raw) => raw,
            Err(error) => {
                if let Some(record) = stale {
                    if let Ok(title) = serde_json::from_value(record.data.clone()) {
                        return Ok(CatalogLookup {
                            value: Some(typed(record, title)),
                            availability: Availability::Stale,
                        });
                    } else {
                        return Err(error);
                    }
                } else {
                    return Err(error);
                }
            }
        };
        let mut title = normalize_title(&raw).ok_or(CatalogServiceError::InvalidResponse)?;
        if title.identity != identity {
            return Err(CatalogServiceError::InvalidResponse);
        }
        title.details = true;
        let record = self.save_title(&title).await?;
        Ok(CatalogLookup {
            value: Some(record),
            availability: Availability::Fresh,
        })
    }

    pub async fn season(
        &self,
        show_id: i64,
        season: i32,
    ) -> Result<CatalogLookup<CatalogRecord<Season>>, CatalogServiceError> {
        self.season_inner(show_id, season, false).await
    }

    /// Refresh the season even when its cached snapshot is still within TTL.
    /// Routes use this when a cached season lacks the requested episode.
    pub async fn refresh_season(
        &self,
        show_id: i64,
        season: i32,
    ) -> Result<CatalogLookup<CatalogRecord<Season>>, CatalogServiceError> {
        self.season_inner(show_id, season, true).await
    }

    async fn season_inner(
        &self,
        show_id: i64,
        season: i32,
        force: bool,
    ) -> Result<CatalogLookup<CatalogRecord<Season>>, CatalogServiceError> {
        if show_id <= 0 || season < 0 {
            return Err(CatalogServiceError::InvalidResponse);
        }
        let now: DateTime<Utc> = std::time::SystemTime::now().into();
        let key = format!("tv:{show_id}:{season}");
        let stale = self.store.get("season", &key, now).await.ok();
        if let Some(record) = &stale
            && !force
            && record.expires_at > now
            && cache_ttl_acceptable(record)
            && let Ok(data) = serde_json::from_value(record.data.clone())
        {
            return Ok(CatalogLookup {
                value: Some(typed(record.clone(), data)),
                availability: Availability::Fresh,
            });
        }
        if !self.available() {
            if let Some(record) = stale
                && let Ok(data) = serde_json::from_value(record.data.clone())
            {
                return Ok(CatalogLookup {
                    value: Some(typed(record, data)),
                    availability: Availability::Stale,
                });
            }
            return Ok(CatalogLookup {
                value: None,
                availability: Availability::Unavailable,
            });
        }
        let raw = match self
            .get_json(&format!("/tv/{show_id}/season/{season}"))
            .await
        {
            Ok(raw) => raw,
            Err(error) => {
                if let Some(record) = stale {
                    if let Ok(data) = serde_json::from_value(record.data.clone()) {
                        return Ok(CatalogLookup {
                            value: Some(typed(record, data)),
                            availability: Availability::Stale,
                        });
                    } else {
                        return Err(error);
                    }
                } else {
                    return Err(error);
                }
            }
        };
        let name = raw
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Season")
            .to_owned();
        let raw_episodes = raw
            .get("episodes")
            .and_then(Value::as_array)
            .ok_or(CatalogServiceError::InvalidResponse)?;
        let episodes = raw_episodes
            .iter()
            .map(|episode| normalize_episode(episode).ok_or(CatalogServiceError::InvalidResponse))
            .collect::<Result<Vec<_>, _>>()?;
        let data = Season {
            show_id,
            season,
            name,
            episodes,
        };
        data.validate()
            .map_err(|_| CatalogServiceError::InvalidResponse)?;
        let record = self.save("season", &key, &data, META_TTL).await?;
        Ok(CatalogLookup {
            value: Some(record),
            availability: Availability::Fresh,
        })
    }

    pub async fn refresh_recommendations(
        &self,
        profile_id: &str,
        inputs: RecommendationInputs,
    ) -> Result<CatalogRecord<Recommendations>, CatalogServiceError> {
        let now: DateTime<Utc> = std::time::SystemTime::now().into();
        inputs
            .validate(now)
            .map_err(|_| CatalogServiceError::InvalidResponse)?;
        let mut contributions: Vec<(Vec<Title>, f64)> = Vec::new();
        let mut sections = Vec::new();
        let seeds = recommendation_seeds(&inputs, now);
        let mut excluded: std::collections::HashSet<String> = seeds
            .iter()
            .map(|(favorite, _)| favorite.identity.key())
            .collect();
        excluded.extend(
            inputs
                .favorites
                .iter()
                .map(|favorite| favorite.identity.key()),
        );
        if self.available() {
            for genre in inputs.genres.iter().take(9) {
                let media_types: &[&str] = match inputs.format {
                    stream_core::catalog::ContentPreference::Movie => &["movie"],
                    stream_core::catalog::ContentPreference::Tv => &["tv"],
                    stream_core::catalog::ContentPreference::Both => &["movie", "tv"],
                };
                for media in media_types {
                    if let Ok(path) = self.discover_path(genre, media).await
                        && let Ok(payload) = self.get_json(&path).await
                    {
                        let mut titles = Vec::new();
                        if let Some(results) = payload.get("results").and_then(Value::as_array) {
                            for raw in results.iter().take(12) {
                                if let Some(title) = normalize_title(raw) {
                                    titles.push(title);
                                }
                            }
                        }
                        if !titles.is_empty() {
                            contributions.push((titles.clone(), 2.0));
                            let cleaned = clean_candidates(&titles, &excluded, 20);
                            sections.push(stream_core::catalog::Shelf {
                                id: format!("taste-{media}-{genre}"),
                                heading: format!(
                                    "{} {}",
                                    genre_label(genre),
                                    if *media == "tv" { "series" } else { "movies" }
                                ),
                                kind: "taste".into(),
                                source: "tmdb-discover".into(),
                                reason: String::new(),
                                titles: cleaned,
                            });
                        }
                    }
                }
            }
            for (seed, weight) in seeds.iter().take(20) {
                let path = format!(
                    "/{}/{}/recommendations?page=1",
                    media_path(seed.identity),
                    seed.identity.id()
                );
                if let Ok(payload) = self.get_json(&path).await
                    && let Some(results) = payload.get("results").and_then(Value::as_array)
                {
                    let mut titles = Vec::new();
                    for raw in results.iter().take(10) {
                        if let Some(title) = normalize_title(raw) {
                            titles.push(title);
                        }
                    }
                    if !titles.is_empty() {
                        contributions.push((titles.clone(), *weight));
                        let cleaned = clean_candidates(&titles, &excluded, 20);
                        if !seed.title.trim().is_empty() && !cleaned.is_empty() {
                            let favorite =
                                inputs.favorites.iter().any(|f| f.identity == seed.identity);
                            sections.push(stream_core::catalog::Shelf {
                                id: format!("like-{}", seed.identity.key()),
                                heading: if favorite {
                                    format!("Because you liked {}", seed.title)
                                } else {
                                    format!("Because you watched {}", seed.title)
                                },
                                kind: "because".into(),
                                source: "tmdb-recommendations".into(),
                                reason: if favorite { "liked" } else { "watched" }.into(),
                                titles: cleaned,
                            });
                        }
                    }
                }
            }
        }
        let fingerprint = input_fingerprint(profile_id, &inputs)?;
        let mut pool = rank_contributions(&contributions, &excluded);
        if pool.is_empty() {
            return Err(CatalogServiceError::Upstream);
        }
        pool = diversify(pool, 50);
        for title in &pool {
            self.save_title(title).await?;
        }
        sections.insert(
            0,
            stream_core::catalog::Shelf {
                id: format!("personalized-{profile_id}"),
                heading: "Picked for you".into(),
                kind: "personal".into(),
                source: "profile".into(),
                reason: String::new(),
                titles: pool.clone(),
            },
        );
        let data = Recommendations {
            fingerprint,
            pool: pool.clone(),
            sections,
        };
        self.save(
            "recommendations",
            profile_id,
            &data,
            recommendation_cadence(now, &inputs),
        )
        .await
    }

    async fn save_title(&self, title: &Title) -> Result<CatalogRecord<Title>, CatalogServiceError> {
        // CatalogStore preserves existing hydrated title fields. Cross-ID overrides remain user-owned.
        self.save("title", &title.identity.key(), title, META_TTL)
            .await
    }
    async fn save<T: serde::Serialize + Clone>(
        &self,
        kind: &str,
        key: &str,
        data: &T,
        ttl: chrono::Duration,
    ) -> Result<CatalogRecord<T>, CatalogServiceError> {
        let now: DateTime<Utc> = std::time::SystemTime::now().into();
        let value = serde_json::to_value(data).map_err(|_| CatalogServiceError::InvalidResponse)?;
        let revision = self
            .store
            .put(
                CatalogWrite {
                    kind: kind.into(),
                    key: key.into(),
                    data: value,
                    ttl,
                },
                now,
            )
            .await?;
        Ok(CatalogRecord {
            kind: kind.into(),
            key: key.into(),
            data: data.clone(),
            updated_at: now,
            expires_at: now + ttl,
            revision,
            state: "fresh".into(),
        })
    }
    async fn get_json(&self, path: &str) -> Result<Value, CatalogServiceError> {
        tokio::time::timeout(Duration::from_secs(13), self.get_json_inner(path))
            .await
            .map_err(|_| CatalogServiceError::Upstream)?
    }

    async fn get_json_inner(&self, path: &str) -> Result<Value, CatalogServiceError> {
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| CatalogServiceError::Upstream)?;
        if !self.available() {
            return Err(CatalogServiceError::Upstream);
        }
        let mut url = reqwest::Url::parse(&format!("{}{}", self.base_url, path))
            .map_err(|_| CatalogServiceError::InvalidResponse)?;
        if let Some(key) = &self.api_key {
            url.query_pairs_mut().append_pair("api_key", key);
        }
        for attempt in 0..3 {
            let delay = {
                let mut rate = self.rate.lock().await;
                let now = Instant::now();
                let scheduled = rate.blocked_until.unwrap_or(now).max(rate.next_request);
                rate.next_request = scheduled + Duration::from_millis(100);
                scheduled.saturating_duration_since(now)
            };
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let mut request = self.client.get(url.clone());
            if let Some(token) = &self.token {
                request = request.bearer_auth(token.as_ref());
            }
            let mut response = request
                .send()
                .await
                .map_err(|_| CatalogServiceError::Upstream)?;
            if response.status() == StatusCode::TOO_MANY_REQUESTS {
                let retry_header = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok());
                let retry_after = retry_header.and_then(|value| {
                    value
                        .parse::<u64>()
                        .ok()
                        .map(Duration::from_secs)
                        .or_else(|| {
                            chrono::DateTime::parse_from_rfc2822(value)
                                .ok()
                                .map(|date| {
                                    let now = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs()
                                        as i64;
                                    Duration::from_secs(
                                        date.timestamp().saturating_sub(now).max(0) as u64
                                    )
                                })
                        })
                });
                let Some(retry_after) = retry_after.or_else(|| {
                    retry_header
                        .is_none()
                        .then(|| Duration::from_millis(250 * (attempt + 1)))
                }) else {
                    let mut rate = self.rate.lock().await;
                    rate.blocked_until = Some(Instant::now() + Duration::from_secs(60));
                    return Err(CatalogServiceError::Upstream);
                };
                {
                    let mut rate = self.rate.lock().await;
                    rate.blocked_until = Some(Instant::now() + retry_after);
                }
                if retry_after > Duration::from_secs(3) || attempt == 2 {
                    return Err(CatalogServiceError::Upstream);
                }
                continue;
            }
            if matches!(
                response.status(),
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            ) {
                return Err(CatalogServiceError::CredentialRejected);
            }
            if !response.status().is_success() {
                return Err(CatalogServiceError::Upstream);
            }
            const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
            if response
                .content_length()
                .is_some_and(|length| length > MAX_BODY_BYTES as u64)
            {
                return Err(CatalogServiceError::InvalidResponse);
            }
            let mut bytes = Vec::with_capacity(
                response
                    .content_length()
                    .unwrap_or(0)
                    .min(MAX_BODY_BYTES as u64) as usize,
            );
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| CatalogServiceError::Upstream)?
            {
                if bytes.len().saturating_add(chunk.len()) > MAX_BODY_BYTES {
                    return Err(CatalogServiceError::InvalidResponse);
                }
                bytes.extend_from_slice(&chunk);
            }
            return serde_json::from_slice(&bytes)
                .map_err(|_| CatalogServiceError::InvalidResponse);
        }
        Err(CatalogServiceError::Upstream)
    }
}

fn media_path(id: TitleIdentity) -> &'static str {
    if id.media_type() == stream_core::MediaType::Movie {
        "movie"
    } else {
        "tv"
    }
}

fn cache_ttl_acceptable<T>(record: &CatalogRecord<T>) -> bool {
    record.expires_at > record.updated_at
        && record.expires_at - record.updated_at <= chrono::Duration::days(30)
}

/// Cache key shared with routes. Offset zero keeps legacy `/search` key compatibility.
pub fn search_cache_key(query: &str, offset: u32) -> String {
    let page_offset = (offset / 20) * 20;
    if page_offset == 0 && query.len() <= 200 {
        return query.to_owned();
    }
    let digest = Sha256::digest(query.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("search:{page_offset}:{digest}")
}

fn stale_search_lookup(
    cached: Option<CatalogRecord>,
    existing: Vec<CatalogRecord>,
    now: DateTime<Utc>,
) -> Option<CatalogLookup<Vec<CatalogRecord<Title>>>> {
    if let Some(record) = cached
        && let Ok(titles) = serde_json::from_value::<Vec<Title>>(record.data.clone())
    {
        let records = titles
            .into_iter()
            .map(|title| {
                let mut result = fresh_record(
                    "title",
                    title.identity.key(),
                    title,
                    record.updated_at,
                    record.revision,
                );
                result.expires_at = record.expires_at;
                result.state = "stale".into();
                result
            })
            .collect();
        return Some(CatalogLookup {
            value: Some(records),
            availability: Availability::Stale,
        });
    }
    if existing.is_empty() {
        return None;
    }
    let records = existing
        .into_iter()
        .filter_map(|record| {
            serde_json::from_value::<Title>(record.data.clone())
                .ok()
                .map(|title| {
                    let is_fresh = record.expires_at > now;
                    let mut typed_record = typed(record, title);
                    if !is_fresh {
                        typed_record.state = "stale".into();
                    }
                    typed_record
                })
        })
        .collect();
    Some(CatalogLookup {
        value: Some(records),
        availability: Availability::Stale,
    })
}
fn recommendation_seeds(
    inputs: &RecommendationInputs,
    now: DateTime<Utc>,
) -> Vec<(stream_core::catalog::Favorite, f64)> {
    let allowed = |identity: TitleIdentity| match inputs.format {
        stream_core::catalog::ContentPreference::Both => true,
        stream_core::catalog::ContentPreference::Movie => {
            identity.media_type() == stream_core::MediaType::Movie
        }
        stream_core::catalog::ContentPreference::Tv => {
            identity.media_type() == stream_core::MediaType::Tv
        }
    };
    let mut seeds: Vec<_> = inputs
        .favorites
        .iter()
        .filter(|favorite| allowed(favorite.identity))
        .cloned()
        .map(|favorite| (favorite, 5.0))
        .collect();
    let mut seen: std::collections::HashSet<String> = seeds
        .iter()
        .map(|(favorite, _)| favorite.identity.key())
        .collect();
    let mut history = Vec::new();
    for signal in &inputs.signals {
        if !allowed(signal.identity) {
            continue;
        }
        let key = signal.identity.key();
        if seen.contains(&key) {
            continue;
        }
        let watched = signal.watched_seconds;
        let qualifies = if signal.identity.media_type() == stream_core::MediaType::Tv {
            watched >= 1200.0 && (signal.meaningful_episodes >= 2 || watched >= 2700.0)
        } else {
            watched >= 600.0
                && (signal.completed
                    || (signal.duration_seconds > 0.0 && watched >= signal.duration_seconds * 0.2))
        };
        let age_days = (now - signal.updated_at).num_seconds().max(0) as f64 / 86_400.0;
        if qualifies && age_days <= 60.0 {
            let base = if signal.identity.media_type() == stream_core::MediaType::Tv {
                (signal.meaningful_episodes + 1) as f64
            } else {
                watched / 1800.0 + 1.0
            };
            let weight = base.min(5.0) * 0.5_f64.powf(age_days / 14.0);
            history.push((
                stream_core::catalog::Favorite {
                    identity: signal.identity,
                    title: signal.title.clone(),
                },
                weight,
            ));
            seen.insert(key);
        }
    }
    history.sort_by(|(a, aw), (b, bw)| {
        bw.total_cmp(aw)
            .then_with(|| a.identity.key().cmp(&b.identity.key()))
    });
    history.truncate(20);
    seeds.extend(history);
    seeds
}

pub fn recommendation_fingerprint(
    profile_id: &str,
    inputs: &RecommendationInputs,
) -> Result<String, CatalogServiceError> {
    let mut canonical = inputs.clone();
    canonical.genres.sort();
    canonical
        .favorites
        .sort_by_key(|favorite| favorite.identity.key());
    canonical
        .signals
        .sort_by_key(|signal| signal.identity.key());
    let qualified: Vec<bool> = canonical.signals.iter().map(signal_qualifies).collect();
    for signal in &mut canonical.signals {
        signal.watched_seconds = (signal.watched_seconds / 600.0).floor() * 600.0;
        signal.updated_at = DateTime::<Utc>::UNIX_EPOCH;
    }
    canonical.activity_days.clear();
    let bytes = serde_json::to_vec(&(2, profile_id, canonical, qualified))
        .map_err(|_| CatalogServiceError::InvalidResponse)?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn signal_qualifies(signal: &stream_core::catalog::ViewingSignal) -> bool {
    if signal.identity.media_type() == stream_core::MediaType::Tv {
        signal.watched_seconds >= 1200.0
            && (signal.meaningful_episodes >= 2 || signal.watched_seconds >= 2700.0)
    } else {
        signal.watched_seconds >= 600.0
            && (signal.completed
                || (signal.duration_seconds > 0.0
                    && signal.watched_seconds >= signal.duration_seconds * 0.2))
    }
}

fn recommendation_cadence(now: DateTime<Utc>, inputs: &RecommendationInputs) -> chrono::Duration {
    let start = now.date_naive() - chrono::Duration::days(2);
    let active_days: std::collections::HashSet<_> = inputs
        .activity_days
        .iter()
        .filter_map(|day| chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
        .filter(|day| *day >= start && *day <= now.date_naive())
        .collect();
    if active_days.len() >= 2 {
        chrono::Duration::days(1)
    } else {
        chrono::Duration::days(7)
    }
}

pub fn input_fingerprint(
    profile_id: &str,
    inputs: &RecommendationInputs,
) -> Result<String, CatalogServiceError> {
    recommendation_fingerprint(profile_id, inputs)
}

pub fn profile_generation_fingerprint(profile: &Profile) -> Result<String, CatalogServiceError> {
    let mut genres = profile.preferences.genres.clone();
    genres.sort();
    let bytes = serde_json::to_vec(&(
        profile.preferences.completed,
        profile.preferences.format,
        genres,
    ))
    .map_err(|_| CatalogServiceError::InvalidResponse)?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn clean_candidates(
    titles: &[Title],
    excluded: &std::collections::HashSet<String>,
    limit: usize,
) -> Vec<Title> {
    titles
        .iter()
        .filter(|title| !excluded.contains(&title.identity.key()))
        .take(limit)
        .cloned()
        .collect()
}

fn rank_contributions(
    contributions: &[(Vec<Title>, f64)],
    excluded: &std::collections::HashSet<String>,
) -> Vec<Title> {
    let mut scores: std::collections::HashMap<String, (Title, f64, usize)> =
        std::collections::HashMap::new();
    for (titles, weight) in contributions {
        let mut seen = std::collections::HashSet::new();
        for (index, title) in titles.iter().enumerate() {
            let key = title.identity.key();
            if excluded.contains(&key) || !seen.insert(key.clone()) {
                continue;
            }
            let score = *weight / ((index + 1) as f64).sqrt();
            let entry = scores.entry(key).or_insert_with(|| (title.clone(), 0.0, 0));
            entry.1 += score;
            entry.2 += 1;
        }
    }
    let mut ranked: Vec<_> = scores
        .into_iter()
        .map(|(key, (title, score, matches))| {
            let score =
                score + ((matches + 1) as f64).log2() * 1.5 + (title.rating - 5.0).max(0.0) * 0.15;
            (key, title, score)
        })
        .collect();
    ranked.sort_by(|(ak, a, ascore), (bk, b, bscore)| {
        bscore
            .total_cmp(ascore)
            .then_with(|| bk.cmp(ak))
            .then_with(|| a.title.cmp(&b.title))
    });
    ranked.into_iter().map(|(_, title, _)| title).collect()
}

fn title_family(title: &str) -> String {
    title
        .split([':', '–', '—', '-'])
        .next()
        .unwrap_or(title)
        .to_lowercase()
        .split_whitespace()
        .filter(|part| part.chars().any(|c| !c.is_ascii_digit()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn diversify(titles: Vec<Title>, limit: usize) -> Vec<Title> {
    let target = titles.len().min(limit);
    let mut remaining = titles.into_iter().map(Some).collect::<Vec<_>>();
    let mut output = Vec::with_capacity(target);
    let mut recent = Vec::new();
    while output.len() < target {
        let start = remaining.iter().position(Option::is_some);
        let Some(first) = start else {
            break;
        };
        let chosen = remaining
            .iter()
            .position(|candidate| {
                candidate
                    .as_ref()
                    .is_some_and(|title| !recent.contains(&title_family(&title.title)))
            })
            .unwrap_or(first);
        if let Some(title) = remaining[chosen].take() {
            recent.push(title_family(&title.title));
            if recent.len() > 3 {
                recent.remove(0);
            }
            output.push(title);
        }
    }
    output
}
fn genre_label(genre: &str) -> &'static str {
    match genre {
        "action" => "Action & adventure",
        "comedy" => "Comedy",
        "drama" => "Drama",
        "crime" => "Crime & mystery",
        "scifi" => "Sci-fi & fantasy",
        "animation" => "Anime",
        "documentary" => "Documentaries",
        "horror" => "Horror",
        "family" => "Family",
        _ => "Browse",
    }
}
fn urlencoding(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn normalize_title(raw: &Value) -> Option<Title> {
    let kind = match raw.get("media_type").and_then(Value::as_str) {
        Some("tv") => stream_core::MediaType::Tv,
        Some("movie") => stream_core::MediaType::Movie,
        Some(_) => return None,
        _ => {
            if raw.get("name").is_some() {
                stream_core::MediaType::Tv
            } else {
                stream_core::MediaType::Movie
            }
        }
    };
    let id = raw.get("id")?.as_i64()?;
    let identity = TitleIdentity::new(kind, id).ok()?;
    let name = raw
        .get("title")
        .or_else(|| raw.get("name"))?
        .as_str()?
        .to_owned();
    let date = raw
        .get("release_date")
        .or_else(|| raw.get("first_air_date"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let genres = raw
        .get("genres")
        .and_then(Value::as_array)
        .map(|g| {
            g.iter()
                .filter_map(|x| x.get("name").and_then(Value::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let seasons = raw
        .get("seasons")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|s| {
                    Some(SeasonSummary {
                        season: s.get("season_number")?.as_i64()? as i32,
                        name: s.get("name")?.as_str()?.to_owned(),
                        episode_count: s.get("episode_count").and_then(Value::as_i64).unwrap_or(0)
                            as i32,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let poster = raw
        .get("poster_path")
        .and_then(Value::as_str)
        .map(|s| format!("{IMAGE_BASE}/w500{s}"))
        .unwrap_or_default();
    let backdrop = raw
        .get("backdrop_path")
        .and_then(Value::as_str)
        .map(|s| format!("{IMAGE_BASE}/w1280{s}"))
        .unwrap_or_default();
    let title = Title {
        identity,
        title: name,
        overview: raw
            .get("overview")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        date,
        rating: raw
            .get("vote_average")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .max(0.0),
        poster,
        backdrop,
        imdb_id: raw
            .get("external_ids")
            .and_then(|v| v.get("imdb_id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        mal_id: 0,
        runtime: raw
            .get("runtime")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0) as i32,
        genres,
        seasons,
        details: false,
    };
    title.validate().ok()?;
    Some(title)
}
fn normalize_episode(raw: &Value) -> Option<stream_core::catalog::Episode> {
    Some(stream_core::catalog::Episode {
        tmdb_episode_id: raw.get("id")?.as_i64()?,
        season: raw.get("season_number")?.as_i64()? as i32,
        episode: raw.get("episode_number")?.as_i64()? as i32,
        name: raw.get("name")?.as_str()?.to_owned(),
        overview: raw
            .get("overview")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        runtime: raw
            .get("runtime")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0) as i32,
        still: raw
            .get("still_path")
            .and_then(Value::as_str)
            .map(|s| format!("{IMAGE_BASE}/w500{s}"))
            .unwrap_or_default(),
        air_date: raw
            .get("air_date")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
    })
}
fn fresh_record<T>(
    kind: &str,
    key: String,
    data: T,
    now: DateTime<Utc>,
    revision: i64,
) -> CatalogRecord<T> {
    CatalogRecord {
        kind: kind.into(),
        key,
        data,
        updated_at: now,
        expires_at: now + META_TTL,
        revision,
        state: "fresh".into(),
    }
}
fn typed<T>(record: CatalogRecord<Value>, data: T) -> CatalogRecord<T> {
    CatalogRecord {
        kind: record.kind,
        key: record.key,
        data,
        updated_at: record.updated_at,
        expires_at: record.expires_at,
        revision: record.revision,
        state: record.state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    #[test]
    fn normalizes_tmdb_media_identity_artwork_and_details() {
        let raw = json!({"media_type":"tv","id":60625,"name":"Rick and Morty","first_air_date":"2013-12-02","overview":"A real overview","vote_average":8.7,"poster_path":"/poster.jpg","backdrop_path":"/backdrop.jpg","genres":[{"name":"Sci-Fi & Fantasy"}],"seasons":[{"season_number":1,"name":"Season 1","episode_count":11}],"external_ids":{"imdb_id":"tt2861424"}});
        let title = normalize_title(&raw);
        assert!(title.is_some());
        let Some(title) = title else {
            return;
        };
        assert_eq!(title.identity.key(), "tv:60625");
        assert_eq!(title.title, "Rick and Morty");
        assert!(!title.details);
        assert!(title.poster.ends_with("/w500/poster.jpg"));
        assert_eq!(title.seasons[0].episode_count, 11);
        assert_eq!(title.imdb_id, "tt2861424");
    }

    #[test]
    fn search_query_escaping_does_not_allow_query_injection() {
        assert_eq!(urlencoding("A & B?"), "A%20%26%20B%3F");
    }

    #[test]
    fn rejects_person_results_and_uses_page_specific_search_keys() {
        assert!(
            normalize_title(&json!({"media_type":"person","id":1,"name":"Actor Example"}))
                .is_none()
        );
        assert_eq!(search_cache_key("Foundation", 0), "Foundation");
        assert_eq!(
            search_cache_key("Foundation", 20),
            search_cache_key("Foundation", 39)
        );
        assert_ne!(
            search_cache_key("Foundation", 0),
            search_cache_key("Foundation", 20)
        );
    }

    #[tokio::test]
    async fn discover_paths_use_valid_tmdb_genre_and_keyword_filters_for_all_default_genres() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").await;
        assert!(listener.is_ok());
        let Ok(listener) = listener else {
            return;
        };
        let address = listener.local_addr();
        assert!(address.is_ok());
        let Ok(address) = address else {
            return;
        };
        let keyword_server = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut request = [0_u8; 1024];
                let read = socket.read(&mut request).await;
                assert!(read.is_ok());
                let body =
                    r#"{"results":[{"id":99123,"name":"Horror"},{"id":222,"name":"Horrors"}]}"#;
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(header.as_bytes()).await;
                let _ = socket.write_all(body.as_bytes()).await;
            }
        });
        let dir = tempfile::tempdir();
        assert!(dir.is_ok());
        let Ok(dir) = dir else {
            return;
        };
        let store = CatalogStore::open(dir.path().join("catalog.sqlite")).await;
        assert!(store.is_ok());
        let Ok(store) = store else {
            return;
        };
        let worker = CatalogWorker::for_mock(Arc::new(store.clone()), &format!("http://{address}"));
        let cases = [
            ("action", "movie", Some("28|12")),
            ("action", "tv", Some("10759")),
            ("comedy", "movie", Some("35")),
            ("comedy", "tv", Some("35")),
            ("drama", "movie", Some("18")),
            ("drama", "tv", Some("18")),
            ("crime", "movie", Some("80|9648")),
            ("crime", "tv", Some("80|9648")),
            ("scifi", "movie", Some("878|14")),
            ("scifi", "tv", Some("10765")),
            ("animation", "movie", Some("16")),
            ("animation", "tv", Some("16")),
            ("documentary", "movie", Some("99")),
            ("documentary", "tv", Some("99")),
            ("horror", "movie", Some("27")),
            ("horror", "tv", None),
            ("family", "movie", Some("10751")),
            ("family", "tv", Some("10751")),
        ];
        for (genre, media, expected_id) in cases {
            let path = worker.discover_path(genre, media).await;
            assert!(path.is_ok());
            let Ok(path) = path else {
                return;
            };
            let url = reqwest::Url::parse(&format!("http://example.test{path}"));
            assert!(url.is_ok());
            let Ok(url) = url else {
                return;
            };
            let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
            assert_eq!(
                query.get("include_adult").map(String::as_str),
                Some("false")
            );
            assert_eq!(query.get("vote_count.gte").map(String::as_str), Some("50"));
            assert_eq!(
                query.get("sort_by").map(String::as_str),
                Some("popularity.desc")
            );
            assert!(query.contains_key(if media == "tv" {
                "first_air_date.lte"
            } else {
                "primary_release_date.lte"
            }));
            if let Some(expected) = expected_id {
                assert_eq!(query.get("with_genres").map(String::as_str), Some(expected));
            } else {
                assert_eq!(
                    query.get("with_keywords").map(String::as_str),
                    Some("99123")
                );
                assert!(!query.contains_key("with_genres"));
            }
            if genre == "animation" {
                assert_eq!(
                    query.get("with_original_language").map(String::as_str),
                    Some("ja")
                );
            }
        }
        let _ = keyword_server.await;
        let _ = store.close().await;
    }

    #[test]
    fn stale_search_snapshot_is_available_as_last_known_good() -> TestResult {
        let now: DateTime<Utc> = std::time::SystemTime::now().into();
        let title =
            normalize_title(&json!({"media_type":"movie","id":123,"title":"Cached result"}))
                .ok_or("stale search fixture did not normalize")?;
        let record = CatalogRecord {
            kind: "search".into(),
            key: "cached".into(),
            data: json!([title]),
            updated_at: now - chrono::Duration::days(2),
            expires_at: now - chrono::Duration::days(1),
            revision: 4,
            state: "stale".into(),
        };
        let result = stale_search_lookup(Some(record), Vec::new(), now)
            .ok_or("stale search snapshot was unavailable")?;
        assert_eq!(result.availability, Availability::Stale);
        assert_eq!(result.value.as_ref().map(Vec::len), Some(1));
        Ok(())
    }

    #[test]
    fn recommendation_fingerprint_tracks_format_and_favorites() {
        let mut inputs = RecommendationInputs {
            completed: true,
            format: stream_core::catalog::ContentPreference::Both,
            genres: vec!["action".into()],
            favorites: Vec::new(),
            signals: Vec::new(),
            activity_days: Vec::new(),
        };
        let original = input_fingerprint("profile-a", &inputs);
        inputs.format = stream_core::catalog::ContentPreference::Movie;
        let movie = input_fingerprint("profile-a", &inputs);
        assert!(original.is_ok() && movie.is_ok() && original.as_ref().ok() != movie.as_ref().ok());
        let identity = TitleIdentity::new(stream_core::MediaType::Movie, 27205);
        assert!(identity.is_ok());
        let Ok(identity) = identity else {
            return;
        };
        inputs.favorites.push(stream_core::catalog::Favorite {
            identity,
            title: "Inception".into(),
        });
        let favorited = input_fingerprint("profile-a", &inputs);
        assert!(
            movie.is_ok() && favorited.is_ok() && movie.as_ref().ok() != favorited.as_ref().ok()
        );
    }

    #[test]
    fn recommendation_fingerprint_ignores_small_progress_ticks_but_tracks_buckets_and_qualification()
     {
        let identity = TitleIdentity::new(stream_core::MediaType::Tv, 42);
        assert!(identity.is_ok());
        let Ok(identity) = identity else {
            return;
        };
        let updated_at: DateTime<Utc> = std::time::SystemTime::now().into();
        let signal = stream_core::catalog::ViewingSignal {
            identity,
            title: "Series".into(),
            watched_seconds: 2600.0,
            duration_seconds: 0.0,
            completed: false,
            meaningful_episodes: 1,
            updated_at,
        };
        let mut inputs = RecommendationInputs {
            completed: true,
            format: stream_core::catalog::ContentPreference::Both,
            genres: Vec::new(),
            favorites: Vec::new(),
            signals: vec![signal.clone()],
            activity_days: vec![updated_at.format("%Y-%m-%d").to_string()],
        };
        let baseline = recommendation_fingerprint("p", &inputs);
        inputs.signals[0].watched_seconds = 2609.0;
        inputs.signals[0].updated_at = updated_at + chrono::Duration::seconds(2);
        inputs.activity_days.clear();
        let progress_tick = recommendation_fingerprint("p", &inputs);
        assert!(
            baseline.is_ok()
                && progress_tick.is_ok()
                && baseline.as_ref().ok() == progress_tick.as_ref().ok()
        );
        inputs.signals[0].watched_seconds = 2700.0;
        let qualified = recommendation_fingerprint("p", &inputs);
        assert!(
            progress_tick.is_ok()
                && qualified.is_ok()
                && progress_tick.as_ref().ok() != qualified.as_ref().ok()
        );
        inputs.signals[0].watched_seconds = 3001.0;
        let next_bucket = recommendation_fingerprint("p", &inputs);
        assert!(
            qualified.is_ok()
                && next_bucket.is_ok()
                && qualified.as_ref().ok() != next_bucket.as_ref().ok()
        );
    }

    #[test]
    fn weighted_recommendations_put_similar_to_favorite_ahead_of_genre_only_pick() -> TestResult {
        let movie = |id, title: &str| {
            TitleIdentity::new(stream_core::MediaType::Movie, id)
                .ok()
                .map(|identity| Title {
                    identity,
                    title: title.into(),
                    overview: String::new(),
                    date: String::new(),
                    rating: 7.0,
                    poster: "/poster.jpg".into(),
                    backdrop: String::new(),
                    imdb_id: String::new(),
                    mal_id: 0,
                    runtime: 90,
                    genres: Vec::new(),
                    seasons: Vec::new(),
                    details: false,
                })
        };
        let favorite_similar = movie(10, "Favorite adjacent");
        let genre_only = movie(11, "Genre adjacent");
        let favorite_similar = favorite_similar.ok_or("favorite fixture has invalid identity")?;
        let genre_only = genre_only.ok_or("genre fixture has invalid identity")?;
        let contributions = vec![
            (vec![favorite_similar.clone()], 5.0),
            (vec![genre_only.clone()], 2.0),
        ];
        let ranked = rank_contributions(&contributions, &std::collections::HashSet::new());
        assert!(
            ranked
                .first()
                .is_some_and(|title| title.identity == favorite_similar.identity)
        );
        assert_ne!(favorite_similar.identity, genre_only.identity);
        Ok(())
    }

    #[tokio::test]
    async fn mock_tmdb_fetch_persists_real_title_metadata() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").await;
        assert!(listener.is_ok());
        let Ok(listener) = listener else {
            return;
        };
        let address = listener.local_addr();
        assert!(address.is_ok());
        let Ok(address) = address else {
            return;
        };
        let response = json!({"id":27205,"title":"Inception","overview":"A thief enters dreams.","release_date":"2010-07-16","vote_average":8.4,"runtime":148,"poster_path":"/poster.jpg","backdrop_path":"/backdrop.jpg","genres":[{"name":"Science Fiction"}],"external_ids":{"imdb_id":"tt1375666"}}).to_string();
        let response_bytes = response.into_bytes();
        let server = tokio::spawn(async move {
            let accepted = listener.accept().await;
            if let Ok((mut socket, _)) = accepted {
                let mut request = [0_u8; 2048];
                let read = socket.read(&mut request).await;
                if let Ok(count) = read {
                    let head = String::from_utf8_lossy(&request[..count]).to_ascii_lowercase();
                    assert!(head.contains("authorization: bearer mock-token"));
                }
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response_bytes.len()
                );
                let _ = socket.write_all(header.as_bytes()).await;
                let _ = socket.write_all(&response_bytes).await;
            }
        });
        let dir = tempfile::tempdir();
        assert!(dir.is_ok());
        let Ok(dir) = dir else {
            return;
        };
        let store = CatalogStore::open(dir.path().join("catalog.sqlite")).await;
        assert!(store.is_ok());
        let Ok(store) = store else {
            return;
        };
        let now: DateTime<Utc> = std::time::SystemTime::now().into();
        let cross_id = store
            .put(
                CatalogWrite {
                    kind: "cross_ids".into(),
                    key: "movie:27205".into(),
                    data: json!({"imdb_id":"tt9999999","mal_id":123}),
                    ttl: chrono::Duration::days(30),
                },
                now,
            )
            .await;
        assert!(cross_id.is_ok());
        let worker = CatalogWorker::for_mock(Arc::new(store.clone()), &format!("http://{address}"));
        let identity = TitleIdentity::new(stream_core::MediaType::Movie, 27205);
        assert!(identity.is_ok());
        let Ok(identity) = identity else {
            return;
        };
        let result = worker.title(identity).await;
        assert!(result.is_ok());
        let Ok(result) = result else {
            return;
        };
        let record = result.value;
        assert!(record.is_some());
        if let Some(record) = record {
            assert_eq!(record.data.title, "Inception");
            assert_eq!(record.data.imdb_id, "tt1375666");
            assert!(record.data.details);
        }
        let merged = store.title("movie:27205", now).await;
        assert!(merged.is_ok());
        if let Ok(merged) = merged {
            assert_eq!(
                merged.data.get("imdb_id").and_then(Value::as_str),
                Some("tt9999999")
            );
            assert_eq!(merged.data.get("mal_id").and_then(Value::as_i64), Some(123));
        }
        let _ = server.await;
        let _ = store.close().await;
    }

    #[tokio::test]
    async fn search_fetches_and_caches_each_tmdb_page_separately() -> TestResult {
        use tokio::{
            io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            for expected_page in [1_i64, 2_i64] {
                let (mut socket, _) = listener.accept().await?;
                let mut request_line = String::new();
                let mut reader = BufReader::new(&mut socket);
                reader.read_line(&mut request_line).await?;
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).await? == 0 {
                        return Err("request ended before headers completed".into());
                    }
                    if header == "\r\n" || header == "\n" {
                        break;
                    }
                }
                drop(reader);
                let target = request_line
                    .split_whitespace()
                    .nth(1)
                    .ok_or("missing request target")?;
                let url = reqwest::Url::parse(&format!("http://{address}{target}"))?;
                assert_eq!(url.path(), "/search/multi");
                let query: std::collections::HashMap<_, _> =
                    url.query_pairs().into_owned().collect();
                assert_eq!(query.get("query").map(String::as_str), Some("page"));
                assert_eq!(query.get("page"), Some(&expected_page.to_string()));
                let payload = json!({"results":[{"media_type":"movie","id":expected_page,"title":format!("Page {expected_page}"),"overview":"real result","vote_average":7.0,"poster_path":"/poster.jpg"}]}).to_string();
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                );
                socket.write_all(head.as_bytes()).await?;
                socket.write_all(payload.as_bytes()).await?;
            }
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        });
        let dir = tempfile::tempdir()?;
        let store = CatalogStore::open(dir.path().join("catalog.sqlite")).await?;
        let worker = CatalogWorker::for_mock(Arc::new(store.clone()), &format!("http://{address}"));
        for (offset, expected_title) in [(0, "Page 1"), (20, "Page 2")] {
            let result = worker.search("page", offset).await?;
            assert_eq!(result.availability, Availability::Fresh);
            let items = result.value.ok_or("page missing from search result")?;
            assert_eq!(
                items.first().map(|item| item.data.title.as_str()),
                Some(expected_title)
            );
        }
        tokio::time::timeout(Duration::from_secs(2), server).await???;
        // The mock listener is now closed: this lookup must be served from the cache.
        let cached = worker.search("page", 20).await?;
        assert_eq!(cached.availability, Availability::Fresh);
        let items = cached.value.ok_or("cached page missing")?;
        assert_eq!(
            items.first().map(|item| item.data.title.as_str()),
            Some("Page 2")
        );
        store.close().await?;
        Ok(())
    }

    #[tokio::test]
    async fn oversized_upstream_body_is_rejected_from_content_length() {
        use tokio::{io::AsyncReadExt, io::AsyncWriteExt, net::TcpListener};
        let listener = TcpListener::bind("127.0.0.1:0").await;
        assert!(listener.is_ok());
        let Ok(listener) = listener else {
            return;
        };
        let address = listener.local_addr();
        assert!(address.is_ok());
        let Ok(address) = address else {
            return;
        };
        let server = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut request = [0_u8; 1024];
                let _ = socket.read(&mut request).await;
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    2 * 1024 * 1024 + 1
                );
                let _ = socket.write_all(header.as_bytes()).await;
            }
        });
        let dir = tempfile::tempdir();
        assert!(dir.is_ok());
        let Ok(dir) = dir else {
            return;
        };
        let store = CatalogStore::open(dir.path().join("catalog.sqlite")).await;
        assert!(store.is_ok());
        let Ok(store) = store else {
            return;
        };
        let worker = CatalogWorker::for_mock(Arc::new(store.clone()), &format!("http://{address}"));
        assert!(matches!(
            worker.get_json("/oversized").await,
            Err(CatalogServiceError::InvalidResponse)
        ));
        let _ = server.await;
        let _ = store.close().await;
    }

    #[tokio::test]
    async fn long_retry_after_sets_shared_cooldown_without_early_retry() {
        use tokio::{io::AsyncReadExt, io::AsyncWriteExt, net::TcpListener};
        let listener = TcpListener::bind("127.0.0.1:0").await;
        assert!(listener.is_ok());
        let Ok(listener) = listener else {
            return;
        };
        let address = listener.local_addr();
        assert!(address.is_ok());
        let Ok(address) = address else {
            return;
        };
        let server = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut request = [0_u8; 1024];
                let _ = socket.read(&mut request).await;
                let _ = socket.write_all(b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 30\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
            }
        });
        let dir = tempfile::tempdir();
        assert!(dir.is_ok());
        let Ok(dir) = dir else {
            return;
        };
        let store = CatalogStore::open(dir.path().join("catalog.sqlite")).await;
        assert!(store.is_ok());
        let Ok(store) = store else {
            return;
        };
        let worker = CatalogWorker::for_mock(Arc::new(store.clone()), &format!("http://{address}"));
        assert!(matches!(
            worker.get_json("/rate-limited").await,
            Err(CatalogServiceError::Upstream)
        ));
        let rate = worker.rate.lock().await;
        assert!(
            rate.blocked_until
                .is_some_and(|until| until > Instant::now() + Duration::from_secs(20))
        );
        drop(rate);
        let _ = server.await;
        let _ = store.close().await;
    }

    #[tokio::test]
    async fn upstream_failures_serve_stale_title_season_and_search_records() -> TestResult {
        use tokio::{io::AsyncReadExt, io::AsyncWriteExt, net::TcpListener};
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            for _ in 0..3 {
                if let Ok((mut socket, _)) = listener.accept().await {
                    let mut request = [0_u8; 1024];
                    let _ = socket.read(&mut request).await;
                    let _ = socket.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                }
            }
        });
        let dir = tempfile::tempdir()?;
        let store = CatalogStore::open(dir.path().join("catalog.sqlite")).await?;
        let now: DateTime<Utc> = std::time::SystemTime::now().into();
        let title = normalize_title(
            &json!({"media_type":"movie","id":303,"title":"Last Known Good","overview":"cached","vote_average":7.0}),
        ).ok_or("cached title fixture did not normalize")?;
        let season = Season {
            show_id: 42,
            season: 1,
            name: "Season One Cached".into(),
            episodes: Vec::new(),
        };
        store
            .put(
                CatalogWrite {
                    kind: "search".into(),
                    key: "known".into(),
                    data: json!([title]),
                    ttl: META_TTL,
                },
                now - chrono::Duration::days(15),
            )
            .await?;
        store
            .put(
                CatalogWrite {
                    kind: "title".into(),
                    key: "movie:303".into(),
                    data: serde_json::to_value(&title)?,
                    ttl: META_TTL,
                },
                now - chrono::Duration::days(15),
            )
            .await?;
        store
            .put(
                CatalogWrite {
                    kind: "season".into(),
                    key: "tv:42:1".into(),
                    data: serde_json::to_value(&season)?,
                    ttl: META_TTL,
                },
                now - chrono::Duration::days(15),
            )
            .await?;
        let worker = CatalogWorker::for_mock(Arc::new(store.clone()), &format!("http://{address}"));
        let identity = TitleIdentity::new(stream_core::MediaType::Movie, 303)?;
        let title_result = worker.title(identity).await;
        assert!(title_result.as_ref().is_ok_and(|result| {
            result.availability == Availability::Stale
                && result
                    .value
                    .as_ref()
                    .is_some_and(|record| record.data.title == "Last Known Good")
        }));
        let season_result = worker.season(42, 1).await;
        assert!(season_result.as_ref().is_ok_and(|result| {
            result.availability == Availability::Stale
                && result
                    .value
                    .as_ref()
                    .is_some_and(|record| record.data.name == "Season One Cached")
        }));
        let search_result = worker.search("known", 0).await;
        assert!(
            search_result
                .as_ref()
                .is_ok_and(|result| result.availability == Availability::Stale
                    && result.value.as_ref().is_some_and(|records| records
                        .first()
                        .is_some_and(|record| record.data.title == "Last Known Good")))
        );
        tokio::time::timeout(Duration::from_secs(2), server).await??;
        store.close().await?;
        Ok(())
    }
}
