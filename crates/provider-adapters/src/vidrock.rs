//! VidRock's current movie/episode API. Quality is established by native probing.
use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, FetchContext, Lookup, Metadata, PublicResourceContext,
        StreamFormat, Track,
    },
};
use stream_provider_codecs::vidrock::decrypt_url;

use crate::{
    AdapterError, ProviderHost,
    common::{self, batch, get, header},
    destination::public_url,
};

use url::Url;

const ORIGIN: &str = "https://vidrock.net";
const LIMIT: u32 = 512 * 1024;

#[derive(Deserialize)]
struct Source {
    #[serde(default)]
    url: String,
    #[serde(default)]
    language: String,
    #[serde(default, rename = "type")]
    format: String,
}
#[derive(Deserialize)]
struct Subtitle {
    label: String,
    file: String,
}

fn is_reachable_media_url(url: &Url, format: StreamFormat) -> bool {
    if url
        .host_str()
        .is_some_and(|host| host.ends_with(".workers.dev") || host == "workers.dev")
    {
        return false;
    }
    let path = url.path().to_ascii_lowercase();
    match format {
        StreamFormat::Hls => path.ends_with(".m3u8"),
        StreamFormat::Mp4 => path.ends_with(".mp4"),
        _ => false,
    }
}

fn candidate_rank(candidate: &CandidateStream) -> u8 {
    match candidate.server.as_str() {
        "Orion" => 0,
        "Nova" => 1,
        _ => 2,
    }
}

pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    let path = if lookup.title.media_type == MediaType::Tv {
        format!(
            "tv/{}/{}/{}",
            lookup.title.tmdb_id.get(),
            lookup.title.season.ok_or(AdapterError::Input)?,
            lookup.title.episode.ok_or(AdapterError::Input)?
        )
    } else {
        format!("movie/{}", lookup.title.tmdb_id.get())
    };
    // Optional subtitles share the lookup's batch and deadline; no serial lookup per mirror.
    let requests = [
        format!("{ORIGIN}/api/{path}"),
        format!("https://sub.vdrk.site/v1/{path}"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, url)| {
        let mut request = get(url, &format!("{ORIGIN}/"), LIMIT);
        request.headers.push(header("Origin", ORIGIN));
        if index == 1 {
            // Optional captions must not hold up startup for the full source deadline.
            request.timeout_ms = 1_000;
        }
        request
    })
    .collect();
    let mut responses = batch(host, requests)?.into_iter();
    let response = responses
        .next()
        .ok_or(AdapterError::Protocol("VidRock response missing"))??;
    let sources: BTreeMap<String, Option<Source>> =
        common::json(&response.body, LIMIT as usize, "VidRock sources")?;
    if sources.len() > 32 {
        return Err(AdapterError::Protocol("VidRock source count"));
    }
    let mut issues = Vec::new();
    let mut tracks = Vec::new();
    if let Some(response) = responses.next() {
        match response.and_then(|response| {
            common::json::<Vec<Subtitle>>(&response.body, LIMIT as usize, "VidRock subtitles")
        }) {
            Ok(subtitles) if subtitles.len() <= 64 => {
                for subtitle in subtitles {
                    if public_url(&subtitle.file).is_ok() && subtitle.label.len() <= 256 {
                        tracks.push(Track {
                            kind: "subtitles".into(),
                            name: subtitle.label,
                            url: subtitle.file,
                            ..Default::default()
                        });
                    }
                }
            }
            Ok(_) => issues.push(common::issue(
                &AdapterError::Protocol("VidRock subtitle count"),
                "subtitles",
            )),
            Err(error) => issues.push(common::issue(&error, "subtitles")),
        }
    }
    let provider = common::provider_id("vidrock")?;
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    for (server, source) in sources {
        let Some(source) = source.filter(|s| !s.url.is_empty()) else {
            continue;
        };
        let parsed = (|| {
            let decoded = decrypt_url(&source.url)
                .map_err(|_| AdapterError::Protocol("VidRock URL envelope"))?;
            let url = std::str::from_utf8(&decoded)
                .map_err(|_| AdapterError::Protocol("VidRock URL encoding"))?;
            let destination = public_url(url)?;
            let format = match source.format.as_str() {
                "hls" => StreamFormat::Hls,
                "mp4" => StreamFormat::Mp4,
                _ => return Err(AdapterError::Protocol("VidRock stream format")),
            };
            if !is_reachable_media_url(&destination, format) {
                return Err(AdapterError::Protocol("VidRock stream endpoint"));
            }
            Ok((url.to_owned(), format))
        })();
        let (url, format) = match parsed {
            Ok(value) => value,
            Err(error) => {
                issues.push(common::issue(&error, "sources"));
                continue;
            }
        };
        if server.len() > 256 || source.language.len() > 256 || !seen.insert(url.clone()) {
            continue;
        }
        let mut metadata = Metadata {
            tracks: tracks.clone(),
            ..Default::default()
        };
        // A provider's language label is a claim, not a complete track inventory.
        metadata
            .provider_data
            .insert("audio_language_claim".into(), source.language.into());
        candidates.push(CandidateStream {
            id: format!(
                "vidrock-{}",
                common::hash_id(&[server.as_bytes(), url.as_bytes()])
            ),
            server,
            url,
            format,
            provider: provider.clone(),
            title: lookup.title.clone(),
            fetch_context: FetchContext {
                scopes: Vec::new(),
                public_resources: Some(PublicResourceContext {
                    origin: ORIGIN.into(),
                    referer: format!("{ORIGIN}/"),
                }),
            },
            resolver_metadata: metadata,
            content_verified: false,
            expires_at: None,
            selected_variant_url: String::new(),
        });
    }
    if candidates.is_empty() {
        return Err(AdapterError::Unavailable("VidRock servers"));
    }
    candidates.sort_by_key(candidate_rank);
    Ok(common::result(candidates, issues))
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_core::providers::{ProviderTitle, TmdbId};
    use stream_provider_sdk::http::{HostError, Request, Response};
    struct Host {
        expected: &'static str,
    }
    impl ProviderHost for Host {
        fn fetch(&self, _: Request) -> Result<Response, HostError> {
            Err(HostError::Denied)
        }
        fn fetch_batch(
            &self,
            requests: Vec<Request>,
        ) -> Result<Vec<Result<Response, HostError>>, HostError> {
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[1].timeout_ms, 1_000);
            assert_eq!(
                requests[0].url,
                format!("https://vidrock.net/api/{}", self.expected)
            );
            assert_eq!(
                requests[1].url,
                format!("https://sub.vdrk.site/v1/{}", self.expected)
            );
            let body = serde_json::json!({
                "Good": {"url": "BwcHBwcHBwcHBwcHUIDmI2upLjuC1aGKIukhZeM6LFAjnj7NR4QXN3j7KptnsoFvi_ClGoJ1JLGTJininJ-BBXw", "type": "hls", "language": "English"},
                "Duplicate": {"url": "BwcHBwcHBwcHBwcHUIDmI2upLjuC1aGKIukhZeM6LFAjnj7NR4QXN3j7KptnsoFvi_ClGoJ1JLGTJininJ-BBXw", "type": "hls"},
                "Private": {"url": "BwcHBwcHBwcHBwcHUIDmIyK8LiXdh-vTbfdqLK0nLlUw0SnHNNISlbDwvUIv0Eb2dcId2g", "type": "hls"},
                "Broken": {"url": "bad", "type": "hls"},
                "Empty": {"url": ""}
            });
            Ok(vec![
                Ok(Response {
                    url: requests[0].url.clone(),
                    status: 200,
                    headers: vec![],
                    body: body.to_string().into_bytes(),
                }),
                Err(HostError::Timeout),
            ])
        }
        fn now_unix_ms(&self) -> Result<i64, HostError> {
            Ok(0)
        }
        fn random_bytes(&self, _: u32) -> Result<Vec<u8>, HostError> {
            Err(HostError::Denied)
        }
    }
    #[test]
    fn movie_and_episode_lookup_preserve_good_sources_without_trusting_claims()
    -> Result<(), Box<dyn std::error::Error>> {
        for (title, expected) in [
            (
                serde_json::json!({"type":"movie", "tmdb_id":550}),
                "movie/550",
            ),
            (
                serde_json::json!({"type":"tv", "tmdb_id":1622,"season":1,"episode":1}),
                "tv/1622/1/1",
            ),
        ] {
            let lookup: Lookup = serde_json::from_value(serde_json::json!({"title":title}))?;
            let result = resolve(&lookup, &Host { expected })?;
            result.validate_for(&common::provider_id("vidrock")?, &lookup.title, 32)?;
            let candidates = result.candidates.ok_or("missing candidates")?;
            assert_eq!(candidates.len(), 1);
            let source = &candidates[0];
            assert_eq!(source.url, "https://media.example.com/master.m3u8");
            assert_eq!(source.title, lookup.title);
            assert!(!source.content_verified);
            assert!(!source.resolver_metadata.tracks_known);
            assert!(source.resolver_metadata.variants.is_empty());
            assert_eq!(result.issues.len(), 3);
        }
        Ok(())
    }

    #[test]
    fn reachability_filter_and_server_priority() -> Result<(), Box<dyn std::error::Error>> {
        let valid_hls = Url::parse("https://stellarpathways.xyz/master.m3u8")?;
        let valid_mp4 = Url::parse("https://stellarpathways.xyz/video.mp4")?;
        let worker_hls = Url::parse("https://belfast.armadilloscp.workers.dev/test.m3u8")?;
        let raw_worker = Url::parse("https://belfast.armadilloscp.workers.dev/cCiYXLhHVr")?;
        let no_ext = Url::parse("https://stellarpathways.xyz/stream/12345")?;

        assert!(is_reachable_media_url(&valid_hls, StreamFormat::Hls));
        assert!(is_reachable_media_url(&valid_mp4, StreamFormat::Mp4));
        assert!(!is_reachable_media_url(&worker_hls, StreamFormat::Hls));
        assert!(!is_reachable_media_url(&raw_worker, StreamFormat::Hls));
        assert!(!is_reachable_media_url(&no_ext, StreamFormat::Hls));
        assert!(!is_reachable_media_url(&valid_hls, StreamFormat::Mp4));

        let title = ProviderTitle {
            media_type: MediaType::Movie,
            tmdb_id: TmdbId::try_from(550)?,
            imdb_id: String::new(),
            my_anime_list_id: None,
            season: None,
            episode: None,
        };
        let pid = common::provider_id("vidrock")?;
        let mk_candidate = |server: &str| CandidateStream {
            id: format!("test-{server}"),
            server: server.into(),
            url: "https://media.example.com/master.m3u8".into(),
            format: StreamFormat::Hls,
            provider: pid.clone(),
            title: title.clone(),
            fetch_context: FetchContext::default(),
            resolver_metadata: Metadata::default(),
            content_verified: false,
            expires_at: None,
            selected_variant_url: String::new(),
        };
        let mut list = [
            mk_candidate("Luna"),
            mk_candidate("Nova"),
            mk_candidate("Orion"),
        ];
        list.sort_by_key(candidate_rank);
        assert_eq!(list[0].server, "Orion");
        assert_eq!(list[1].server, "Nova");
        assert_eq!(list[2].server, "Luna");
        Ok(())
    }
}
