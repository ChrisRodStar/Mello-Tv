//! CineSU exposes coordinate-addressed HLS manifests.
use crate::{
    AdapterError, ProviderHost,
    common::{self, fetch, get},
    destination::public_url,
};
use regex::Regex;
use std::sync::OnceLock;
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, FetchContext, Lookup, Metadata, PublicResourceContext,
        StreamFormat, Variant,
    },
};

const ORIGIN: &str = "https://cine.su";
const LIMIT: u32 = 512 * 1024;

pub fn resolve(lookup: &Lookup, host: &dyn ProviderHost) -> Result<AdapterResult, AdapterError> {
    lookup.title.validate().map_err(|_| AdapterError::Input)?;
    let path = match lookup.title.media_type {
        MediaType::Movie => format!("movie/{}.m3u8", lookup.title.tmdb_id.get()),
        MediaType::Tv => format!(
            "tv/{}/{}/{}.m3u8",
            lookup.title.tmdb_id.get(),
            lookup.title.season.ok_or(AdapterError::Input)?,
            lookup.title.episode.ok_or(AdapterError::Input)?
        ),
    };
    let url = format!("{ORIGIN}/v1/stream/master/{path}");
    let mut request = get(&url, &format!("{ORIGIN}/en/watch"), LIMIT);
    request.headers.push(common::header("Origin", ORIGIN));
    let response = fetch(host, request)?;
    let body = std::str::from_utf8(&response.body)
        .map_err(|_| AdapterError::Protocol("CineSU manifest encoding"))?;
    if !body.starts_with("#EXTM3U") {
        return Err(AdapterError::Protocol("CineSU manifest signature"));
    }
    let variants = parse_variants(body, &url)?;
    let provider = common::provider_id("cinesu")?;
    let candidate = CandidateStream {
        expires_at: None,
        id: format!("cinesu-{}", common::hash_id(&[url.as_bytes()])),
        server: "CineSU".into(),
        selected_variant_url: String::new(),
        url: url.clone(),
        provider: provider.clone(),
        format: StreamFormat::Hls,
        title: lookup.title.clone(),
        fetch_context: FetchContext {
            scopes: Vec::new(),
            public_resources: Some(PublicResourceContext {
                origin: ORIGIN.into(),
                referer: format!("{ORIGIN}/"),
            }),
        },
        resolver_metadata: Metadata {
            variants,
            ..Default::default()
        },
        content_verified: false,
    };
    Ok(common::result(vec![candidate], Vec::new()))
}

fn parse_variants(body: &str, manifest: &str) -> Result<Vec<Variant>, AdapterError> {
    static RESOLUTION: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    let resolution = RESOLUTION.get_or_init(|| Regex::new(r"(?:^|,)RESOLUTION=(\d+)x(\d+)(?:,|$)"));
    let resolution = resolution
        .as_ref()
        .map_err(|_| AdapterError::Protocol("CineSU parser"))?;
    let base =
        url::Url::parse(manifest).map_err(|_| AdapterError::Protocol("CineSU manifest URL"))?;
    let mut variants = Vec::new();
    let lines: Vec<&str> = body.lines().map(str::trim).collect();
    for pair in lines.windows(2) {
        if !pair[0].starts_with("#EXT-X-STREAM-INF:") {
            continue;
        }
        let Some(captures) = resolution.captures(pair[0].trim_start_matches("#EXT-X-STREAM-INF:"))
        else {
            continue;
        };
        if variants.len() >= 64 {
            return Err(AdapterError::Protocol("CineSU variant count"));
        }
        let width = captures
            .get(1)
            .and_then(|m| m.as_str().parse::<i64>().ok())
            .ok_or(AdapterError::Protocol("CineSU width"))?;
        let height = captures
            .get(2)
            .and_then(|m| m.as_str().parse::<i64>().ok())
            .ok_or(AdapterError::Protocol("CineSU height"))?;
        let resolved = base
            .join(pair[1])
            .map_err(|_| AdapterError::Protocol("CineSU variant URL"))?;
        public_url(resolved.as_str())?;
        variants.push(Variant {
            url: resolved.to_string(),
            width: Some(width),
            height: Some(height),
            ..Default::default()
        });
    }
    Ok(variants)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_provider_sdk::http::{HostError, Request, Response};

    struct FixtureHost;
    impl ProviderHost for FixtureHost {
        fn fetch(&self, request: Request) -> Result<Response, HostError> {
            if request.url != format!("{ORIGIN}/v1/stream/master/tv/1622/1/2.m3u8") {
                return Err(HostError::InvalidRequest);
            }
            Ok(Response {
                url: request.url,
                status: 200,
                headers: Vec::new(),
                body: b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1400000,RESOLUTION=1280x720\n720/index.m3u8\n".to_vec(),
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
    fn resolves_relative_variants_and_keeps_measured_dimensions()
    -> Result<(), Box<dyn std::error::Error>> {
        let variants = parse_variants(
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=400000,RESOLUTION=640x360\nlow/index.m3u8\n#EXT-X-STREAM-INF:RESOLUTION=1920x1080,BANDWIDTH=2000000\n../high.m3u8\n",
            "https://media.example/path/master.m3u8",
        )?;
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].url, "https://media.example/path/low/index.m3u8");
        assert_eq!(variants[0].height, Some(360));
        assert_eq!(variants[1].height, Some(1080));
        Ok(())
    }
    #[test]
    fn rejects_private_variant_destinations() {
        assert!(
            parse_variants(
                "#EXTM3U\n#EXT-X-STREAM-INF:RESOLUTION=1280x720\nhttp://127.0.0.1/a.m3u8\n",
                "https://media.example/master.m3u8"
            )
            .is_err()
        );
    }

    #[test]
    fn resolve_checks_manifest_bytes_and_candidate_contract()
    -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup = serde_json::from_value(
            serde_json::json!({"title":{"type":"tv","tmdb_id":1622,"season":1,"episode":2}}),
        )?;
        let result = resolve(&lookup, &FixtureHost)?;
        let provider = common::provider_id("cinesu")?;
        let candidates = result.candidates.ok_or("candidate list missing")?;
        assert_eq!(candidates.len(), 1);
        candidates[0].validate_for(&provider, &lookup.title)?;
        assert_eq!(
            candidates[0].resolver_metadata.variants[0].height,
            Some(720)
        );
        assert!(!candidates[0].content_verified);
        Ok(())
    }
}
