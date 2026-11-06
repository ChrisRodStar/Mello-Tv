//! Icefy's TMDB keyed stream endpoint.
use serde::Deserialize;
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, FetchContext, Lookup, Metadata, PublicResourceContext,
        StreamFormat,
    },
};

use crate::{
    AdapterError, ProviderHost,
    common::{self, fetch, get, header},
    destination::public_url,
};

const ORIGIN: &str = "https://streams.icefy.top";
const LIMIT: u32 = 64 * 1024;

#[derive(Deserialize)]
struct IcefyResponse {
    success: Option<bool>,
    #[serde(default)]
    stream: String,
}

pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    let path = match lookup.title.media_type {
        MediaType::Movie => format!("movie/{}", lookup.title.tmdb_id.get()),
        MediaType::Tv => format!(
            "tv/{}/{}/{}",
            lookup.title.tmdb_id.get(),
            lookup.title.season.ok_or(AdapterError::Input)?,
            lookup.title.episode.ok_or(AdapterError::Input)?
        ),
    };
    let mut request = get(format!("{ORIGIN}/{path}"), &format!("{ORIGIN}/"), LIMIT);
    request.headers.push(header("Origin", ORIGIN));
    request.headers.push(header(
        "Accept",
        "application/json, text/javascript, */*; q=0.01",
    ));
    request
        .headers
        .push(header("Accept-Language", "en-US,en;q=0.9"));
    request.headers.push(header(
        "User-Agent",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150 Safari/537.36",
    ));
    let response = fetch(host, request)?;
    let data: IcefyResponse = common::json(&response.body, LIMIT as usize, "Icefy response")?;
    if data.success == Some(false) || data.stream.is_empty() {
        return Err(AdapterError::Unavailable("Icefy stream"));
    }
    public_url(&data.stream)?;
    let provider = common::provider_id("icefy")?;
    let candidate = CandidateStream {
        id: format!("icefy-{}", common::hash_id(&[data.stream.as_bytes()])),
        server: "default".into(),
        url: data.stream,
        format: StreamFormat::Hls,
        provider,
        title: lookup.title.clone(),
        fetch_context: FetchContext {
            scopes: Vec::new(),
            public_resources: Some(PublicResourceContext {
                origin: ORIGIN.into(),
                referer: format!("{ORIGIN}/"),
            }),
        },
        resolver_metadata: Metadata::default(),
        content_verified: false,
        expires_at: None,
        selected_variant_url: String::new(),
    };
    Ok(common::result(vec![candidate], Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_provider_sdk::http::{HostError, Request, Response};

    struct Host {
        expected: String,
    }
    impl ProviderHost for Host {
        fn fetch(&self, request: Request) -> Result<Response, HostError> {
            assert_eq!(request.url, self.expected);
            assert!(
                request
                    .headers
                    .iter()
                    .any(|h| h.name == "Origin" && h.value == ORIGIN)
            );
            Ok(Response {
                url: request.url,
                status: 200,
                headers: Vec::new(),
                body: br#"{"stream":"https://media.example.org/movie.m3u8"}"#.to_vec(),
            })
        }
        fn fetch_batch(
            &self,
            _: Vec<Request>,
        ) -> Result<Vec<Result<Response, HostError>>, HostError> {
            Err(HostError::Denied)
        }
        fn now_unix_ms(&self) -> Result<i64, HostError> {
            Ok(0)
        }
        fn random_bytes(&self, _: u32) -> Result<Vec<u8>, HostError> {
            Err(HostError::Denied)
        }
    }
    #[test]
    fn resolves_movie_and_episode_with_unverified_claims() -> Result<(), Box<dyn std::error::Error>>
    {
        for (title, path) in [
            (
                serde_json::json!({"type":"movie","tmdb_id":550}),
                "movie/550",
            ),
            (
                serde_json::json!({"type":"tv","tmdb_id":1622,"season":1,"episode":2}),
                "tv/1622/1/2",
            ),
        ] {
            let lookup: Lookup = serde_json::from_value(serde_json::json!({"title":title}))?;
            let result = resolve(
                &lookup,
                &Host {
                    expected: format!("{ORIGIN}/{path}"),
                },
            )?;
            result.validate_for(&common::provider_id("icefy")?, &lookup.title, 4)?;
            let candidates = result.candidates.ok_or("no candidate")?;
            assert_eq!(candidates.len(), 1);
            assert!(!candidates[0].content_verified);
            assert!(candidates[0].resolver_metadata.provider_data.is_empty());
        }
        Ok(())
    }
}
