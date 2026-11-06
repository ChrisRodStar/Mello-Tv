//! Tulnex's multi-server resolver and layered encrypted response support.
use crate::{
    AdapterError, ProviderHost,
    common::{self, batch, get},
    destination::public_url,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use stream_core::{
    domain::MediaType,
    providers::{
        AdapterResult, CandidateStream, FetchContext, Lookup, Metadata, PublicResourceContext,
        StreamFormat,
    },
};
use stream_provider_codecs::tulnex::decrypt;

const ORIGIN: &str = "https://api.tulnex.com";
const SERVERS: [&str; 14] = [
    "onion",
    "vidzee",
    "icefy",
    "tik",
    "vaplayer",
    "vidfast-alpha",
    "uniquestream",
    "vidfast-mega",
    "vidfast-vrapid",
    "allmovies",
    "vidlink",
    "vidfast-vedge",
    "vidfast-vfast",
    "moviebox",
];
const LIMIT: u32 = 512 * 1024;
#[derive(Deserialize, Default)]
struct Envelope {
    #[serde(default)]
    payload: String,
}
fn find_urls(value: &Value, out: &mut Vec<(String, Option<String>)>, depth: usize) {
    if depth > 8 || out.len() >= 32 {
        return;
    }
    match value {
        Value::String(url) if url.starts_with("http") => out.push((url.clone(), None)),
        Value::Object(map) => {
            let direct = [
                "url",
                "stream",
                "playlist",
                "streamUrl",
                "stream_url",
                "streaming_url",
                "video_url",
                "m3u8",
            ];
            for key in direct {
                if let Some(url) = map
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|s| s.starts_with("http"))
                {
                    out.push((
                        url.into(),
                        map.get("type").and_then(Value::as_str).map(str::to_owned),
                    ));
                    return;
                }
            }
            for (key, child) in map {
                if matches!(
                    key.as_str(),
                    "sources" | "streams" | "data" | "stream" | "links" | "languages"
                ) {
                    find_urls(child, out, depth + 1);
                }
            }
        }
        Value::Array(items) => {
            for item in items.iter().take(32) {
                find_urls(item, out, depth + 1);
            }
        }
        _ => {}
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
    let requests = SERVERS
        .iter()
        .map(|server| {
            let mut req = get(
                format!("{ORIGIN}/{server}/{path}"),
                &format!("{ORIGIN}/"),
                LIMIT,
            );
            req.timeout_ms = 3_000;
            req
        })
        .collect();
    let responses = batch(host, requests)?;
    let provider = common::provider_id("tulnex")?;
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
    for (server, response) in SERVERS.into_iter().zip(responses) {
        let response = match response {
            Ok(r) => r,
            Err(e) => {
                issues.push(common::issue(&e, "server"));
                continue;
            }
        };
        let envelope: Envelope =
            match common::json(&response.body, LIMIT as usize, "Tulnex response") {
                Ok(v) => v,
                Err(e) => {
                    issues.push(common::issue(&e, "server"));
                    continue;
                }
            };
        if envelope.payload.is_empty() || envelope.payload.len() > LIMIT as usize * 2 {
            issues.push(common::issue(
                &AdapterError::Protocol("Tulnex payload"),
                "decrypt",
            ));
            continue;
        }
        let decoded = match decrypt(&envelope.payload) {
            Ok(v) => v,
            Err(_) => {
                issues.push(common::issue(
                    &AdapterError::Protocol("Tulnex envelope"),
                    "decrypt",
                ));
                continue;
            }
        };
        let value: Value = match common::json(&decoded, LIMIT as usize, "Tulnex plaintext") {
            Ok(v) => v,
            Err(e) => {
                issues.push(common::issue(&e, "decrypt"));
                continue;
            }
        };
        let mut sources = Vec::new();
        find_urls(&value, &mut sources, 0);
        for (index, (url, kind)) in sources.into_iter().enumerate() {
            if public_url(&url).is_err() || !seen.insert(url.clone()) {
                continue;
            }
            let format = match kind.as_deref().unwrap_or("").to_ascii_lowercase().as_str() {
                "mp4" | "mkv" => StreamFormat::Mp4,
                "hls" | "m3u8" => StreamFormat::Hls,
                "" => {
                    if url.contains(".mp4") || url.contains(".mkv") {
                        StreamFormat::Mp4
                    } else {
                        StreamFormat::Hls
                    }
                }
                _ => continue,
            };
            candidates.push(CandidateStream {
                id: format!(
                    "tulnex-{server}-{index}-{}",
                    common::hash_id(&[url.as_bytes()])
                ),
                server: server.into(),
                url,
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
        return Err(AdapterError::Unavailable("Tulnex servers"));
    }
    Ok(common::result(candidates, issues))
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream_provider_sdk::http::{HostError, Request, Response};

    const ENCRYPTED_SAMPLE: &str = "022ba24ce54806fc82061bed5d07a7826a895cd0c16199b9910275720861ef7f14f792960c3c86c0135c346f1385c969b9457d34ac1e3ac4e867fa3c973f6cc2|UVdBVFJMK3lTKzRkWjU2OVVMUXdzUT09LmNtRnVaRzl0YzJGc2RERXlNdz09LldNRjVDMmVFVHkybzFNeFV4eXV1K2I1Nm9qT04vWHBrUHBQbUlqVjk5ZlE4VGtRUUdENUlPOG9yckhsRC9obFBrWUY4cTJRT1RjR3N6NVJCNzlzZEZXUkIwWHdFV0xEWHFVSXV6eGh3UUNjNzlDdnJPWVJ6Qk42ME5YcjZaWDg1S0ZlOWZ4QmN1Y2RHR0NMalV4TzA5aU1vczRMc2dkc2V6WTFNMUtJM3lCbFVVWWpYZnJHWXNmK05BcE5DNEF6VTRLMXI1a3BqL3hiNXgzd0ZkYlhHL3A4U1BtSGRUVXpiMHFIWmdwR08zZWhqbXJXRUhQRkVScnk1RDk5TmN4SDJqYnpQOVlOUnhCNXhBc2RyY0w2ai9ub2ZDSzU5T3BDRnc1QWhXdGJ6S0N5WndSc1pickhsaDJ0dUNwUFEwdU1VN213V1AzWEQ1d2Z5cnMxa000ajEwMUdKS3orNEN0b1FwUzdQZE9RTzdqbmN1QkxKaWhjL0ZFOUx2VEkzd1E0MlpldktSaWY2ZVdiT0RKQktDd3FZU2xqUnpkMHlxQVdzYy9VTkJCWldxWktDVVQ1cGYvMDVxWFdEbVc2QlczekZMSlU3NHdCOXRKMnF1TkEzQXI1WjBxbllUYnAvL3UvVm9GaHYzTERRK2xJeWJBNXh2bXVXUWVBS3RBS2NnQys3NUgvRmpjNzNLTFhSMmdSSjBjendEczFoajZOZW9SOE93SXZEcFpJa0pqY3BYdWIwUzFTQ2pIaXBaaE41YTNZeXdJYjZkaGhkalNZdlJ5VnJYZEIxM0JQQnJTQkhobGtnZi9vWGs4djBad2pjeXhwMmRkUjh2and3Z05yVmR0TzZFS3RiMWNPclkwOUVIejZybG5tZUtWUjJ6SHgyOGZVT01qbGZDTzlMRzB3emtBKzdjQjQ1ME5UWThBRisxR3V0M3h5ZWg1VTRGTU9mN2xVS0lWT2VSNlpHRElVcmFEb2dTdklFU3FBby85OVR4cFQ2dC90QkVFQzE1c2REMmV0VDNtZFc1R0JDUzNyUjZYeTlQeHNJMmZicjI1K0FPc1VlcFB2UFZucHlLeFZSWTdhdisrVTA0K2RaYWpjWSsxdTc0bm9rdVdFM1Q3M0x0SWZjS0Fyai9YdzA2MFBZNll4NGJIUVNaNjZISmpiTUozdmxrenRWWVVSTnJjNGV1OW5UVGdyUkpXOExRclRtUmNEL2NibXcybzA1NFVnc0h3OFdoRUpKU050eFZZRmxmd2dQcU5QN21WMmR1bXBPc08wUXVXeHlKT0RGbHFsei9ZTWhrSlVJdUV5NURBVDB4OU5icHdxWXBqbzE3aW9zSW1vbVlRMk80UTNUd0Z3SU9vbHlxcmJOc2xLYW5lZW9RYTZOTUdvNVhobVJRS0w3RE9zdjVGVy9ITG1Qdm1GWnNINFJNWG4rTEdoSzJLMW5oZllsSStkcXEyVU55TXdEWDNEN1pQZ0ZyV2VvRWs2N25HWGtLWnRPOTd1NGRzbHFnL3l2dmRVSHFGS0RZRWxkLzFDU2RJVU4ySHV3VytuWE5lamdabGx0S3VTT2c3bFVCY0c5cXBvNVpuWWNoV0RySDkvZGIyNWFKQ2krdnl2akJNWHVlUGJGeG54eHBZajRJdUxZOUlDRGc1a2dSbFpCWXNxUjlKQlJ0K09YSXN0aTZvaDB3NHNrQmdUZEhnMnd1bXBweURaRnZNRlJsOGc3KzRhVm1EM09ORVJ4Ti9XbUZZUEpuVG9BL1c2d3A0NFlEYmZMU2hPZUdYb3VjUEZIWnJxbkVWeUhuSkFyeUJkK1d1M1lsdWgwUVBuTlgyMWRKOHBpdlpuOXgra09NYU5vWmFlcmd5RGtkWmJQZVhvb2JvSnJPYjc4MnZnc1YybnQ1ZG8vVHFJdGtJcndXcVV4c0FvK0VGZENTT2N5WlhQUkI0SmRqZGZwWENYV0ZaTzd2U3JnTFRyOERRMnBoYlNnbmZuUHdONXErM1NEcGxrMVNjSm9FQnNIOUVBaGFtOVJQZ1lqcktCdTJDYjhFRWpwZFNMSndmZDFCSmdhaGlRcnlUTWpuK2RSWGEwQU00WWREU3I4WENjQ0tjdzlRN0ZVWkE0bmtxeW05ZnZnUW1HMytaTXUydzltUkdFUjNQZUdaYjUwNzlodjlZM3ZnSUlBbXNGQ3dyNjZLNk54c1JpVHZuMEJUeURsWW5kRnM4M3lLWWZYWUR6cjVLNkVoY3dEcER2aWdzMnBVbDU3dnlBdkdqVGVPYkRWeVhwekxSKzNOZk5GNXZQTXgrL01BMDQvejFlbjE2K3pEZDlpRUpESkNMVVRjQkFNbnc9PQ==";

    struct MockHost {
        respond_with_sample: bool,
    }

    impl ProviderHost for MockHost {
        fn fetch(&self, _: Request) -> Result<Response, HostError> {
            Err(HostError::Denied)
        }

        fn fetch_batch(
            &self,
            requests: Vec<Request>,
        ) -> Result<Vec<Result<Response, HostError>>, HostError> {
            assert_eq!(requests.len(), SERVERS.len());
            let mut out = Vec::new();
            for (idx, req) in requests.into_iter().enumerate() {
                if self.respond_with_sample && idx == 0 {
                    let envelope = serde_json::json!({
                        "payload": ENCRYPTED_SAMPLE
                    });
                    out.push(Ok(Response {
                        url: req.url,
                        status: 200,
                        headers: vec![],
                        body: envelope.to_string().into_bytes(),
                    }));
                } else {
                    out.push(Err(HostError::UpstreamFailure));
                }
            }
            Ok(out)
        }

        fn now_unix_ms(&self) -> Result<i64, HostError> {
            Ok(0)
        }

        fn random_bytes(&self, _: u32) -> Result<Vec<u8>, HostError> {
            Err(HostError::Denied)
        }
    }

    #[test]
    fn resolve_decrypts_stream_from_server_response() -> Result<(), Box<dyn std::error::Error>> {
        let lookup: Lookup = serde_json::from_value(serde_json::json!({
            "title": {
                "type": "movie",
                "tmdb_id": 27205
            }
        }))?;
        let host = MockHost {
            respond_with_sample: true,
        };
        let result = resolve(&lookup, &host)?;
        assert_eq!(result.candidates.as_ref().map(Vec::len), Some(1));
        let candidates = result
            .candidates
            .as_ref()
            .ok_or("missing candidates in result")?;
        let candidate = &candidates[0];
        assert_eq!(candidate.url, "https://example.com/stream.m3u8");
        assert_eq!(candidate.format, StreamFormat::Hls);
        assert_eq!(candidate.server, "onion");
        assert_eq!(result.issues.len(), 13);
        Ok(())
    }

    #[test]
    fn resolve_returns_unavailable_when_all_servers_fail() -> Result<(), Box<dyn std::error::Error>>
    {
        let lookup: Lookup = serde_json::from_value(serde_json::json!({
            "title": {
                "type": "tv",
                "tmdb_id": 1396,
                "season": 1,
                "episode": 1
            }
        }))?;
        let host = MockHost {
            respond_with_sample: false,
        };
        let err = match resolve(&lookup, &host) {
            Err(e) => e,
            Ok(_) => return Err("expected error".into()),
        };
        assert!(matches!(err, AdapterError::Unavailable(_)));
        Ok(())
    }
}
