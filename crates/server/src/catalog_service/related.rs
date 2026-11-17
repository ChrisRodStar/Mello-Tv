use super::{CatalogServiceError, CatalogWorker, META_TTL, media_path, normalize_title};
use serde_json::Value;
use std::collections::HashSet;
use stream_core::catalog::{MediaType, Title, TitleIdentity};
use stream_storage::catalog::CatalogWrite;

impl CatalogWorker {
    pub(crate) async fn refresh_related(
        &self,
        identity: TitleIdentity,
    ) -> Result<Vec<Title>, CatalogServiceError> {
        let media = media_path(identity);
        let raw = self
            .get_json(&format!(
                "/{media}/{}?append_to_response=recommendations",
                identity.id()
            ))
            .await?;
        let seed = normalize_title(&raw).ok_or(CatalogServiceError::InvalidResponse)?;
        if seed.identity != identity {
            return Err(CatalogServiceError::InvalidResponse);
        }

        let mut linked = Vec::new();
        let mut incomplete = false;
        if let Some(collection) = raw["belongs_to_collection"]["id"]
            .as_i64()
            .filter(|id| *id > 0)
        {
            match self.get_json(&format!("/collection/{collection}")).await {
                Ok(record) if record["parts"].is_array() => {
                    linked.extend(result_titles(&record["parts"]));
                }
                _ => incomplete = true,
            }
        }
        if identity.media_type() == MediaType::Tv {
            for &id in story_links(identity.id()) {
                let peer = TitleIdentity::new(MediaType::Tv, id)
                    .map_err(|_| CatalogServiceError::InvalidResponse)?;
                match self.title(peer).await {
                    Ok(result) if result.value.is_some() => {
                        linked.extend(result.value.map(|record| record.data));
                    }
                    _ => incomplete = true,
                }
            }
        }

        let upstream = &raw["recommendations"]["results"];
        let mut recommendations = result_titles(upstream);
        // Some franchises are split into multiple collections. Prioritize matching
        // series names within the upstream recommendations, keeping their order.
        recommendations.sort_by_key(|candidate| !same_series_name(&seed.title, &candidate.title));
        if linked.is_empty() && !upstream.is_array() {
            return Err(CatalogServiceError::InvalidResponse);
        }
        let mut seen = HashSet::new();
        let titles: Vec<_> = linked
            .into_iter()
            .chain(recommendations)
            .filter(|title| {
                title.identity != identity
                    && title.identity.media_type() == identity.media_type()
                    && seen.insert(title.identity.key())
            })
            .take(12)
            .collect();
        if incomplete && titles.is_empty() {
            return Err(CatalogServiceError::Upstream);
        }

        let mut writes = titles
            .iter()
            .map(|title| {
                Ok(CatalogWrite {
                    kind: "title".into(),
                    key: title.identity.key(),
                    data: serde_json::to_value(title)?,
                    ttl: META_TTL,
                })
            })
            .collect::<Result<Vec<_>, serde_json::Error>>()
            .map_err(|_| CatalogServiceError::InvalidResponse)?;
        writes.push(CatalogWrite {
            kind: "related".into(),
            key: identity.key(),
            data: serde_json::to_value(&titles)
                .map_err(|_| CatalogServiceError::InvalidResponse)?,
            ttl: if incomplete || titles.is_empty() {
                chrono::Duration::hours(1)
            } else {
                chrono::Duration::days(3)
            },
        });
        self.store
            .put_batch(writes, std::time::SystemTime::now().into())
            .await?;
        Ok(titles)
    }
}

fn result_titles(value: &Value) -> Vec<Title> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .take(30)
        .filter_map(normalize_title)
        .collect()
}

fn same_series_name(left: &str, right: &str) -> bool {
    let stem = |title: &str| {
        title
            .split_once(": ")
            .map(|(series, _)| series.to_lowercase())
    };
    match (stem(left), stem(right)) {
        (Some(left), Some(right)) => left == right,
        (Some(series), None) => series == right.to_lowercase(),
        (None, Some(series)) => series == left.to_lowercase(),
        _ => false,
    }
}

// TV series have no equivalent of TMDB's film collections. These explicit
// story links come from Disney's VisionQuest press kit:
// https://press.disney.co.uk/press-kit/visionquest-press-kit
fn story_links(id: i64) -> &'static [i64] {
    match id {
        213375 => &[85271, 138501], // VisionQuest
        85271 => &[138501, 213375], // WandaVision
        138501 => &[85271, 213375], // Agatha All Along
        _ => &[],
    }
}
