//! Minimal TMDB v3 client: just what the library sync and play paths need.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

const API_BASE: &str = "https://api.themoviedb.org/3";
const CACHE_TTL: Duration = Duration::from_secs(6 * 3600);
const CACHE_MAX_ENTRIES: usize = 4096;

#[derive(Debug, thiserror::Error)]
pub enum TmdbError {
    #[error("TMDB credentials are not configured")]
    NotConfigured,
    #[error("TMDB title not found")]
    NotFound,
    #[error("TMDB rejected the credentials")]
    Unauthorized,
    #[error("TMDB request failed: {0}")]
    Request(String),
}

type CacheEntry = (Instant, Arc<serde_json::Value>);

#[derive(Clone)]
enum Credential {
    Bearer(String),
    ApiKey(String),
}

#[derive(Clone)]
pub struct Tmdb {
    client: reqwest::Client,
    credential: Option<Credential>,
    base: Arc<str>,
    cache: Arc<Mutex<HashMap<String, CacheEntry>>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Movie {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub imdb_id: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Show {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub first_air_date: Option<String>,
    #[serde(default)]
    pub seasons: Vec<SeasonSummary>,
    #[serde(default)]
    pub external_ids: ExternalIds,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ExternalIds {
    #[serde(default)]
    pub imdb_id: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct SeasonSummary {
    pub season_number: i64,
    #[serde(default)]
    pub episode_count: i64,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Season {
    #[serde(default)]
    pub episodes: Vec<Episode>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Episode {
    pub episode_number: i64,
    #[serde(default)]
    pub air_date: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListItem {
    pub media_type: String,
    pub id: i64,
    pub title: String,
    pub year: Option<String>,
}

#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    results: Vec<RawItem>,
    #[serde(default)]
    items: Vec<RawItem>,
    #[serde(default)]
    total_pages: u32,
}

#[derive(Deserialize)]
struct RawItem {
    id: i64,
    #[serde(default)]
    media_type: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    first_air_date: Option<String>,
}

impl RawItem {
    fn into_item(self, default_type: Option<&str>) -> Option<ListItem> {
        let media_type = self
            .media_type
            .as_deref()
            .or(default_type)
            .filter(|t| matches!(*t, "movie" | "tv"))?
            .to_owned();
        let date = self.release_date.or(self.first_air_date);
        Some(ListItem {
            media_type,
            id: self.id,
            title: self.title.or(self.name).unwrap_or_default(),
            year: year(date.as_deref()).map(str::to_owned),
        })
    }
}

/// First four characters of an ISO date, when present.
pub fn year(date: Option<&str>) -> Option<&str> {
    date.filter(|d| d.len() >= 4 && d.as_bytes()[..4].iter().all(u8::is_ascii_digit))
        .map(|d| &d[..4])
}

impl Tmdb {
    pub fn new(api_key: Option<String>, read_token: Option<String>) -> Result<Self, TmdbError> {
        let credential = read_token
            .map(Credential::Bearer)
            .or(api_key.map(Credential::ApiKey));
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("mello-jellyfin/1.0")
            .build()
            .map_err(|e| TmdbError::Request(format!("HTTP client setup failed: {e}")))?;
        Ok(Self {
            client,
            credential,
            base: Arc::from(API_BASE),
            cache: Arc::default(),
        })
    }

    #[cfg(test)]
    pub fn with_base(mut self, base: &str) -> Self {
        self.base = Arc::from(base);
        self
    }

    pub fn configured(&self) -> bool {
        self.credential.is_some()
    }

    async fn get_value(
        &self,
        path: &str,
        params: &[(&str, &str)],
        cache: bool,
    ) -> Result<Arc<serde_json::Value>, TmdbError> {
        let credential = self.credential.as_ref().ok_or(TmdbError::NotConfigured)?;
        let cache_key = format!("{path}?{params:?}");
        if cache
            && let Ok(entries) = self.cache.lock()
            && let Some((at, value)) = entries.get(&cache_key)
            && at.elapsed() < CACHE_TTL
        {
            return Ok(value.clone());
        }

        for attempt in 0..3 {
            let mut request = self
                .client
                .get(format!("{}{path}", self.base))
                .query(params);
            request = match credential {
                Credential::Bearer(token) => request.bearer_auth(token),
                Credential::ApiKey(key) => request.query(&[("api_key", key)]),
            };
            let response = request
                .send()
                .await
                .map_err(|e| TmdbError::Request(e.without_url().to_string()))?;
            match response.status().as_u16() {
                200 => {
                    let value: serde_json::Value = response
                        .json()
                        .await
                        .map_err(|e| TmdbError::Request(e.without_url().to_string()))?;
                    let value = Arc::new(value);
                    if cache && let Ok(mut entries) = self.cache.lock() {
                        if entries.len() >= CACHE_MAX_ENTRIES {
                            entries.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
                            if entries.len() >= CACHE_MAX_ENTRIES {
                                entries.clear();
                            }
                        }
                        entries.insert(cache_key, (Instant::now(), value.clone()));
                    }
                    return Ok(value);
                }
                401 => return Err(TmdbError::Unauthorized),
                404 => return Err(TmdbError::NotFound),
                429 if attempt < 2 => {
                    let wait = response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(2)
                        .min(10);
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                }
                status => return Err(TmdbError::Request(format!("status {status}"))),
            }
        }
        Err(TmdbError::Request("rate limited".into()))
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<T, TmdbError> {
        let value = self.get_value(path, params, true).await?;
        T::deserialize(value.as_ref()).map_err(|e| TmdbError::Request(e.to_string()))
    }

    /// Bypasses the cache: used by the periodic sync so new episodes appear.
    async fn get_fresh<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<T, TmdbError> {
        let value = self.get_value(path, params, false).await?;
        if let Ok(mut entries) = self.cache.lock() {
            entries.insert(
                format!("{path}?{params:?}"),
                (Instant::now(), value.clone()),
            );
        }
        T::deserialize(value.as_ref()).map_err(|e| TmdbError::Request(e.to_string()))
    }

    pub async fn movie(&self, id: i64) -> Result<Movie, TmdbError> {
        self.get(&format!("/movie/{id}"), &[]).await
    }

    pub async fn show(&self, id: i64, fresh: bool) -> Result<Show, TmdbError> {
        let path = format!("/tv/{id}");
        let params = [("append_to_response", "external_ids")];
        if fresh {
            self.get_fresh(&path, &params).await
        } else {
            self.get(&path, &params).await
        }
    }

    pub async fn season(&self, id: i64, season: i64, fresh: bool) -> Result<Season, TmdbError> {
        let path = format!("/tv/{id}/season/{season}");
        if fresh {
            self.get_fresh(&path, &[]).await
        } else {
            self.get(&path, &[]).await
        }
    }

    pub async fn search(&self, query: &str) -> Result<Vec<ListItem>, TmdbError> {
        let page: Page = self
            .get(
                "/search/multi",
                &[("query", query), ("include_adult", "false")],
            )
            .await?;
        Ok(page
            .results
            .into_iter()
            .filter_map(|item| item.into_item(None))
            .collect())
    }

    /// Pages through a TMDB collection endpoint until `limit` items are found.
    pub async fn collection(
        &self,
        path: &str,
        default_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ListItem>, TmdbError> {
        let mut items = Vec::new();
        let mut page_number = 1u32;
        loop {
            let page_param = page_number.to_string();
            let page: Page = self
                .get_fresh(path, &[("page", page_param.as_str())])
                .await?;
            let total_pages = page.total_pages;
            items.extend(
                page.results
                    .into_iter()
                    .chain(page.items)
                    .filter_map(|item| item.into_item(default_type)),
            );
            if items.len() >= limit || page_number >= total_pages || page_number >= 20 {
                break;
            }
            page_number += 1;
        }
        items.truncate(limit);
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn years_come_from_iso_dates_only() {
        assert_eq!(year(Some("2010-07-15")), Some("2010"));
        assert_eq!(year(Some("")), None);
        assert_eq!(year(Some("TBA")), None);
        assert_eq!(year(None), None);
    }

    #[test]
    fn list_items_normalise_movies_and_shows() {
        let raw: Page = serde_json::from_str(
            r#"{"results":[
                {"id":1,"media_type":"movie","title":"A","release_date":"2001-01-01"},
                {"id":2,"media_type":"tv","name":"B","first_air_date":"2002-02-02"},
                {"id":3,"media_type":"person","name":"C"}
            ],"total_pages":1}"#,
        )
        .unwrap_or_else(|_| unreachable!());
        let items: Vec<_> = raw
            .results
            .into_iter()
            .filter_map(|i| i.into_item(None))
            .collect();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].title, "B");
        assert_eq!(items[1].year.as_deref(), Some("2002"));
    }
}
