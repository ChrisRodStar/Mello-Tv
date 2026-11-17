use super::{CatalogServiceError, CatalogWorker, DiscoverOrder, META_TTL, normalize_title};
use serde_json::Value;
use std::collections::HashSet;
use stream_core::catalog::{Shelf, Title};
use stream_storage::catalog::CatalogWrite;
use tokio::task::JoinSet;

impl CatalogWorker {
    pub(crate) async fn shelf_expansion(&self, shelf: &Shelf) -> Vec<Title> {
        let now = std::time::SystemTime::now().into();
        let cached = self.store.get("shelf_expansion", &shelf.id, now).await.ok();
        if let Some(record) = cached.as_ref().filter(|record| record.expires_at > now)
            && let Ok(titles) = serde_json::from_value(record.data.clone())
        {
            return titles;
        }
        if let Ok(Some(path)) = self.expansion_path(&shelf.id).await
            && let Ok(titles) = self.fetch_expansion(&shelf.id, &path).await
        {
            return titles;
        }
        cached
            .and_then(|record| serde_json::from_value(record.data).ok())
            .unwrap_or_default()
    }

    async fn expansion_path(&self, id: &str) -> Result<Option<String>, CatalogServiceError> {
        if let Some((kind, rest)) = id.split_once('-')
            && matches!(kind, "taste" | "discover" | "recent")
            && let Some((media @ ("movie" | "tv"), genre)) = rest.split_once('-')
        {
            let order = match kind {
                "discover" => DiscoverOrder::Rated { page: None },
                "recent" => DiscoverOrder::Recent,
                _ => DiscoverOrder::Popular,
            };
            return self.discover_path(genre, media, order).await.map(Some);
        }
        if let Some(seed) = id.strip_prefix("like-")
            && let Some((media @ ("movie" | "tv"), title_id)) = seed.split_once(':')
            && let Ok(title_id) = title_id.parse::<i64>()
            && title_id > 0
        {
            return Ok(Some(format!(
                "/{media}/{title_id}/recommendations?language=en-US"
            )));
        }
        if let Some((media, genre)) = match id {
            "movie-action" => Some(("movie", "action")),
            "movie-comedy" => Some(("movie", "comedy")),
            "tv-crime" => Some(("tv", "crime")),
            "tv-drama" => Some(("tv", "drama")),
            _ => None,
        } {
            return self
                .discover_path(genre, media, DiscoverOrder::Popular)
                .await
                .map(Some);
        }
        Ok(match id {
            "trending" => Some("/trending/all/week?language=en-US"),
            "movie-popular" => Some("/movie/popular?language=en-US"),
            "movie-top-rated" => Some("/movie/top_rated?language=en-US"),
            "movie-now-playing" => Some("/movie/now_playing?language=en-US"),
            "movie-upcoming" => Some("/movie/upcoming?language=en-US"),
            "tv-popular" => Some("/tv/popular?language=en-US"),
            "tv-top-rated" => Some("/tv/top_rated?language=en-US"),
            "tv-airing-today" => Some("/tv/airing_today?language=en-US"),
            "tv-on-air" => Some("/tv/on_the_air?language=en-US"),
            "topic-classics" => Some("/discover/movie?include_adult=false&primary_release_date.lte=1999-12-31&vote_count.gte=50"),
            "topic-short-movies" => Some("/discover/movie?include_adult=false&with_runtime.lte=99&vote_count.gte=50"),
            "topic-romantic-comedy" => Some("/discover/movie?include_adult=false&with_genres=35%2C10749&vote_count.gte=50"),
            "topic-documentaries" => Some("/discover/movie?include_adult=false&with_genres=99&vote_count.gte=50"),
            "topic-fantasy" => Some("/discover/movie?include_adult=false&with_genres=14&vote_count.gte=50"),
            "topic-mysteries" => Some("/discover/tv?include_adult=false&with_genres=9648&vote_count.gte=50"),
            _ => None,
        }.map(str::to_owned))
    }

    async fn fetch_expansion(
        &self,
        key: &str,
        path: &str,
    ) -> Result<Vec<Title>, CatalogServiceError> {
        let first = self.get_json(&format!("{path}&page=1")).await?;
        let total = first["total_pages"].as_u64().unwrap_or(1).clamp(1, 4) as usize;
        let mut pages = vec![Value::Null; total];
        pages[0] = first;
        let mut requests = JoinSet::new();
        for index in 1..total {
            let worker = self.clone();
            let request = format!("{path}&page={}", index + 1);
            requests.spawn(async move { (index, worker.get_json(&request).await) });
        }
        while let Some(result) = requests.join_next().await {
            let (index, page) = result.map_err(|_| CatalogServiceError::Upstream)?;
            pages[index] = page?;
        }
        let mut seen = HashSet::new();
        let mut titles = Vec::new();
        for page in pages {
            let rows = page["results"]
                .as_array()
                .ok_or(CatalogServiceError::InvalidResponse)?;
            titles.extend(
                rows.iter()
                    .take(20)
                    .filter_map(normalize_title)
                    .filter(|title| !title.poster.is_empty() && seen.insert(title.identity.key())),
            );
        }
        let mut writes = Vec::with_capacity(titles.len() + 1);
        for title in &titles {
            writes.push(CatalogWrite {
                kind: "title".into(),
                key: title.identity.key(),
                data: serde_json::to_value(title)
                    .map_err(|_| CatalogServiceError::InvalidResponse)?,
                ttl: META_TTL,
            });
        }
        writes.push(CatalogWrite {
            kind: "shelf_expansion".into(),
            key: key.into(),
            data: serde_json::to_value(&titles)
                .map_err(|_| CatalogServiceError::InvalidResponse)?,
            ttl: chrono::Duration::hours(6),
        });
        self.store
            .put_batch(writes, std::time::SystemTime::now().into())
            .await?;
        Ok(titles)
    }
}
