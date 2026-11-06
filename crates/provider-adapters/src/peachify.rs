//! Peachify's six-origin parallel source catalog.
use crate::{
    AdapterError, ProviderHost,
    common::{self, batch, get, header},
    destination::public_url,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, FetchContext, Lookup, Metadata, PublicResourceContext,
        StreamFormat, Track,
    },
};
use stream_provider_codecs::peachify::decrypt;

const ORIGIN: &str = "https://peachify.top";
const SERVERS: [&str; 6] = [
    "https://uwu.eat-peach.sbs/moviebox",
    "https://usa.eat-peach.sbs/holly",
    "https://usa.eat-peach.sbs/air",
    "https://usa.eat-peach.sbs/multi",
    "https://uwu.eat-peach.sbs/net",
    "https://uwu.eat-peach.sbs/bmb",
];
const LIMIT: u32 = 512 * 1024;
#[derive(Deserialize, Default)]
struct Envelope {
    #[serde(default, rename = "isEncrypted")]
    encrypted: bool,
    #[serde(default)]
    data: String,
    #[serde(default)]
    sources: Vec<Value>,
    #[serde(default)]
    subtitles: Vec<Value>,
}
fn field<'a>(value: &'a Value, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|n| {
        value
            .get(*n)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    })
}
fn bounded_text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.bytes().any(|b| b.is_ascii_control())
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
    let requests = SERVERS
        .iter()
        .map(|server| {
            let mut req = get(format!("{server}/{path}"), &format!("{ORIGIN}/"), LIMIT);
            req.timeout_ms = 3_000;
            req.headers.push(header("Origin", ORIGIN));
            req
        })
        .collect();
    let responses = batch(host, requests)?;
    let provider = common::provider_id("peachify")?;
    let context = FetchContext {
        scopes: Vec::new(),
        public_resources: Some(PublicResourceContext {
            origin: ORIGIN.into(),
            referer: format!("{ORIGIN}/"),
        }),
    };
    let mut candidates = Vec::new();
    let mut issues = Vec::new();
    let mut seen = BTreeSet::new();
    let mut all_tracks = Vec::new();
    for (server_index, response) in responses.into_iter().enumerate() {
        let response = match response {
            Ok(r) => r,
            Err(e) => {
                issues.push(common::issue(&e, "server"));
                continue;
            }
        };
        let mut body: Envelope =
            match common::json(&response.body, LIMIT as usize, "Peachify response") {
                Ok(v) => v,
                Err(e) => {
                    issues.push(common::issue(&e, "server"));
                    continue;
                }
            };
        if body.encrypted {
            let decoded = match decrypt(&body.data) {
                Ok(v) => v,
                Err(_) => {
                    issues.push(common::issue(
                        &AdapterError::Protocol("Peachify envelope"),
                        "decrypt",
                    ));
                    continue;
                }
            };
            body = match common::json(&decoded, LIMIT as usize, "Peachify plaintext") {
                Ok(v) => v,
                Err(e) => {
                    issues.push(common::issue(&e, "decrypt"));
                    continue;
                }
            };
        }
        if body.sources.len() > 64 || body.subtitles.len() > 128 {
            issues.push(common::issue(
                &AdapterError::Protocol("Peachify response count"),
                "server",
            ));
            continue;
        }
        for subtitle in body.subtitles {
            if let Some(url) = field(&subtitle, &["url", "file", "src"])
                && bounded_text(url, 8192)
                && public_url(url).is_ok()
            {
                let name =
                    field(&subtitle, &["label", "name", "language", "lang"]).unwrap_or("Subtitle");
                if name.len() <= 256 {
                    all_tracks.push(Track {
                        kind: "subtitles".into(),
                        name: name.into(),
                        url: url.into(),
                        ..Default::default()
                    });
                }
            }
        }
        for (index, source) in body.sources.into_iter().enumerate() {
            let Some(url) = field(
                &source,
                &["url", "src", "file", "stream", "streamUrl", "playbackUrl"],
            ) else {
                continue;
            };
            if !bounded_text(url, 8192) || public_url(url).is_err() || !seen.insert(url.to_owned())
            {
                continue;
            }
            let format = match field(&source, &["type", "format", "container"])
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str()
            {
                "mp4" => StreamFormat::Mp4,
                "" | "hls" | "m3u8" => StreamFormat::Hls,
                _ => {
                    issues.push(common::issue(
                        &AdapterError::Protocol("Peachify format"),
                        "source",
                    ));
                    continue;
                }
            };
            candidates.push(CandidateStream {
                id: format!(
                    "peachify-{server_index}-{index}-{}",
                    common::hash_id(&[url.as_bytes()])
                ),
                server: format!("server-{server_index}"),
                url: url.into(),
                format,
                provider: provider.clone(),
                title: lookup.title.clone(),
                fetch_context: context.clone(),
                resolver_metadata: Metadata::default(),
                content_verified: false,
                expires_at: None,
                selected_variant_url: String::new(),
            });
            if candidates.len() >= 32 {
                break;
            }
        }
    }
    if candidates.is_empty() {
        return Err(AdapterError::Unavailable("Peachify servers"));
    }
    if all_tracks.len() > 128 {
        all_tracks.truncate(128);
    }
    for candidate in &mut candidates {
        candidate.resolver_metadata.tracks = all_tracks.clone();
        candidate.resolver_metadata.tracks_known = !all_tracks.is_empty();
    }
    Ok(common::result(candidates, issues))
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_provider_sdk::http::{HostError, Request, Response};
    struct Host {
        expected_path: &'static str,
    }
    impl ProviderHost for Host {
        fn fetch(&self, _: Request) -> Result<Response, HostError> {
            Err(HostError::Denied)
        }
        fn fetch_batch(
            &self,
            requests: Vec<Request>,
        ) -> Result<Vec<Result<Response, HostError>>, HostError> {
            assert_eq!(requests.len(), SERVERS.len());
            assert!(
                requests
                    .iter()
                    .all(|r| r.timeout_ms == 3_000 && r.url.contains(self.expected_path))
            );
            let good = serde_json::json!({"sources":[{"url":"https://media.example.com/master.m3u8","type":"hls"},{"url":"http://127.0.0.1/private.m3u8","type":"hls"}],"subtitles":[{"file":"https://media.example.com/en.vtt","label":"English"}]});
            Ok(requests
                .into_iter()
                .enumerate()
                .map(|(i, request)| {
                    if i == 0 {
                        Ok(Response {
                            url: request.url,
                            status: 200,
                            headers: vec![],
                            body: good.to_string().into_bytes(),
                        })
                    } else {
                        Err(HostError::UpstreamFailure)
                    }
                })
                .collect())
        }
        fn now_unix_ms(&self) -> Result<i64, HostError> {
            Ok(1)
        }
        fn random_bytes(&self, _: u32) -> Result<Vec<u8>, HostError> {
            Err(HostError::Denied)
        }
    }
    #[test]
    fn resolve_movie_and_episode_yield_owned_public_candidates()
    -> Result<(), Box<dyn std::error::Error>> {
        for (title, path) in [
            (
                serde_json::json!({"type":"movie","tmdb_id":550}),
                "movie/550",
            ),
            (
                serde_json::json!({"type":"tv","tmdb_id":1399,"season":2,"episode":5}),
                "tv/1399/2/5",
            ),
        ] {
            let lookup: Lookup = serde_json::from_value(serde_json::json!({"title":title}))?;
            let result = resolve(
                &lookup,
                &Host {
                    expected_path: path,
                },
            )?;
            result.validate_for(&common::provider_id("peachify")?, &lookup.title, 32)?;
            let candidates = result.candidates.ok_or("missing candidates")?;
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates[0].url, "https://media.example.com/master.m3u8");
            assert_eq!(candidates[0].title, lookup.title);
            assert!(!candidates[0].content_verified);
            assert_eq!(candidates[0].resolver_metadata.tracks.len(), 1);
        }
        Ok(())
    }
}
