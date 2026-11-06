//! Opt-in live acceptance through the production native provider, prober,
//! selection and relay. Reports omit signed URLs, headers and credentials.
//!
//! cargo run -p stream-media --example check_providers -- vidrock tv 1399 2 5 --relay
//! `all` checks every registered provider. `--relay` saves three segments under
//! ignored Local/reports/provider-acceptance for independent audio/video decoding.
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{
    error::Error,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};
use stream_core::{
    domain::MediaType,
    providers::{
        DeviceCapabilities, Lookup, ProviderTitle, QualityPreference, Selection, StreamFormat,
        TmdbId,
    },
};
use stream_media::{
    discovery::ProviderCatalog,
    network::HttpClient,
    prober::{ProbeLimits, StreamProber},
    providers::NativeProviders,
    relay::{MediaRelay, RelayResponse},
    selection,
};
use tokio_util::sync::CancellationToken;

type CheckResult<T> = Result<T, Box<dyn Error>>;

#[tokio::main]
async fn main() -> CheckResult<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        return Err("usage: check_providers <all|provider,provider> <movie|tv> <tmdb_id> [season episode] [--relay]".into());
    }
    let media_type = match args[1].as_str() {
        "movie" => MediaType::Movie,
        "tv" => MediaType::Tv,
        _ => return Err("content type must be movie or tv".into()),
    };
    let (season, episode) = if media_type == MediaType::Tv {
        (
            Some(args.get(3).ok_or("missing season")?.parse()?),
            Some(args.get(4).ok_or("missing episode")?.parse()?),
        )
    } else {
        (None, None)
    };
    let imdb_id = if let Some(index) = args.iter().position(|arg| arg == "--imdb") {
        args.get(index + 1).ok_or("missing IMDb ID")?.clone()
    } else {
        String::new()
    };
    let lookup = Lookup {
        title: ProviderTitle {
            media_type,
            tmdb_id: TmdbId::try_from(args[2].parse::<i64>()?)?,
            imdb_id,
            my_anime_list_id: None,
            season,
            episode,
        },
        preferences: QualityPreference::default(),
        device: DeviceCapabilities {
            formats: vec![StreamFormat::Hls, StreamFormat::Mp4],
            codecs: vec!["avc1".into(), "mp4a".into(), "h264".into(), "aac".into()],
            max_height: 1080,
            allow_unknown_codecs: true,
            allow_unknown_resolution: true,
            supports_encrypted_hls: true,
        },
        context_key: String::new(),
        startup_height: 1080,
        wait_for_complete: false,
        refresh: true,
    };
    lookup.validate()?;
    let requested: Vec<&str> = args[0].split(',').collect();
    let catalog = NativeProviders::new()?;
    let providers = catalog.snapshot()?;
    for id in &requested {
        if *id != "all" && !providers.iter().any(|p| p.id().as_str() == *id) {
            return Err(format!("unknown provider {id}").into());
        }
    }
    let prober = StreamProber::new(
        Arc::new(HttpClient::new()?),
        ProbeLimits {
            total_sample_bytes: 16 * 1024 * 1024,
            max_variants: 3,
            ..ProbeLimits::default()
        },
    )?;
    for provider in providers
        .into_iter()
        .filter(|p| requested.contains(&"all") || requested.contains(&p.id().as_str()))
    {
        let started = Instant::now();
        let cancel = CancellationToken::new();
        let mut report = json!({"provider":provider.id().as_str(),"title":{"tmdb_id":lookup.title.tmdb_id.get(),"season":season,"episode":episode}});
        let resolved = tokio::time::timeout(
            Duration::from_secs(18),
            provider.resolve(&lookup, cancel.clone()),
        )
        .await;
        report["lookup_ms"] = json!(started.elapsed().as_millis());
        let result = match resolved {
            Ok(Ok(result)) => result,
            Ok(Err(category)) => {
                report["lookup_failure"] = json!(category);
                println!("{report}");
                continue;
            }
            Err(_) => {
                cancel.cancel();
                report["lookup_failure"] = json!("timeout");
                println!("{report}");
                continue;
            }
        };
        if let Err(error) = result.validate_for(provider.id(), &lookup.title, 32) {
            report["lookup_failure"] = json!("invalid_provider_contract");
            report["validation"] = json!({"field":error.field,"reason":error.reason});
            println!("{report}");
            continue;
        }
        let candidates = result.candidates.unwrap_or_default();
        report["candidates"] = json!(candidates.len());
        report["issues"] = json!(
            result
                .issues
                .iter()
                .map(|i| json!({"stage":i.stage,"category":i.category}))
                .collect::<Vec<_>>()
        );
        let streams = prober.probe_all(candidates, &cancel).await?;
        report["streams"] = json!(streams.iter().map(|s| json!({"server":s.source.server,"accessible":s.observation.accessible,"format":s.observation.format,"width":s.observation.variant.as_ref().and_then(|v|v.width),"height":s.observation.variant.as_ref().and_then(|v|v.height),"sample_bytes":s.observation.sample_bytes,"samples_completed":s.observation.samples_completed,"failure":s.observation.failure})).collect::<Vec<_>>());
        let selection = selection::select(&lookup, streams, SystemTime::now().into())?;
        report["selected"] = json!(selection.selected.is_some());
        report["rejected"] = json!(
            selection
                .rejected
                .as_ref()
                .map(|items| items.values().collect::<Vec<_>>())
        );
        if args.iter().any(|a| a == "--relay") && selection.selected.is_some() {
            let directory = PathBuf::from("Local/reports/provider-acceptance")
                .join(provider.id().as_str())
                .join(format!(
                    "{}_{}_{}",
                    lookup.title.tmdb_id.get(),
                    season.unwrap_or(0),
                    episode.unwrap_or(0)
                ));
            report["relay"] = match tokio::time::timeout(
                Duration::from_secs(90),
                verify_relay(selection, directory),
            )
            .await
            {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => json!({"failure":"relay_delivery_failed"}),
                Err(_) => json!({"failure":"relay_delivery_timeout"}),
            };
        }
        report["total_ms"] = json!(started.elapsed().as_millis());
        println!("{report}");
    }
    Ok(())
}

async fn verify_relay(selection: Selection, directory: PathBuf) -> CheckResult<Value> {
    let selected = selection
        .selected
        .as_ref()
        .ok_or("missing selected stream")?;
    let binary = selected.candidate.source.format == StreamFormat::Mp4;
    let variant = selected.candidate.source.selected_variant_url.clone();
    let relay = MediaRelay::new();
    if !relay.register_session("acceptance".into(), "local-only".into(), selection) {
        return Err("session registration failed".into());
    }
    let response = relay
        .fetch_entry(
            "acceptance",
            "local-only",
            binary.then_some("bytes=0-2097151"),
        )
        .await
        .map_err(|_| "entry delivery failed")?;
    std::fs::create_dir_all(&directory)?;
    let mut manifest = match response {
        RelayResponse::Manifest(manifest) => manifest,
        response @ RelayResponse::Binary { .. } => {
            let bytes = save_binary(response, directory.join("source.mp4")).await?;
            return Ok(json!({"binary_bytes":bytes,"path":directory.join("source.mp4")}));
        }
    };
    if manifest.contains("#EXT-X-STREAM-INF") {
        let resource = if !variant.is_empty() {
            MediaRelay::resource_id(&variant)
        } else {
            manifest_resource(&manifest)?
        };
        manifest = match relay
            .fetch_resource("acceptance", &resource, "local-only", None)
            .await
            .map_err(|_| "rendition delivery failed")?
        {
            RelayResponse::Manifest(manifest) => manifest,
            _ => return Err("rendition was not a playlist".into()),
        };
    }
    let mut samples = Vec::new();
    let mut local_manifest = String::new();
    let mut auxiliaries = std::collections::BTreeSet::new();
    for line in manifest.lines() {
        if line.starts_with("/stream/") {
            let resource = resource_id(line)?;
            let response = relay
                .fetch_resource("acceptance", resource, "local-only", None)
                .await
                .map_err(|_| "segment delivery failed")?;
            let name = format!("segment-{}.ts", samples.len());
            let path = directory.join(&name);
            let bytes = save_binary(response, path.clone()).await?;
            samples.push(json!({"bytes":bytes,"path":path}));
            local_manifest.push_str(&name);
            local_manifest.push('\n');
            if samples.len() == 3 {
                break;
            }
        } else if (line.starts_with("#EXT-X-KEY:") || line.starts_with("#EXT-X-MAP:"))
            && line.contains("URI=\"")
        {
            let (_, rest) = line.split_once("URI=\"").ok_or("missing auxiliary URI")?;
            let (uri, _) = rest.split_once('"').ok_or("invalid auxiliary URI")?;
            let resource = resource_id(uri)?;
            let is_key = line.starts_with("#EXT-X-KEY:");
            let name = format!("{}-{resource}.bin", if is_key { "key" } else { "init" });
            if auxiliaries.insert(name.clone()) {
                let response = relay
                    .fetch_resource("acceptance", resource, "local-only", None)
                    .await
                    .map_err(|_| "auxiliary delivery failed")?;
                let bytes = save_binary(response, directory.join(&name)).await?;
                if is_key && bytes != 16 {
                    return Err("invalid AES-128 key length".into());
                }
            }
            local_manifest.push_str(&line.replace(uri, &name));
            local_manifest.push('\n');
        } else {
            local_manifest.push_str(line);
            local_manifest.push('\n');
        }
    }
    if samples.is_empty() {
        return Err("no media segments delivered".into());
    }
    if !local_manifest.lines().any(|line| line == "#EXT-X-ENDLIST") {
        local_manifest.push_str("#EXT-X-ENDLIST\n");
    }
    let playlist = directory.join("playlist.m3u8");
    std::fs::write(&playlist, local_manifest)?;
    Ok(json!({"segments":samples,"playlist":playlist}))
}
fn resource_id(uri: &str) -> CheckResult<&str> {
    uri.split_once("/r/")
        .and_then(|(_, rest)| rest.split('?').next())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "invalid relay resource".into())
}
fn manifest_resource(manifest: &str) -> CheckResult<String> {
    Ok(resource_id(
        manifest
            .lines()
            .find(|l| l.starts_with("/stream/"))
            .ok_or("no rendition resource")?,
    )?
    .to_owned())
}
async fn save_binary(response: RelayResponse, path: PathBuf) -> CheckResult<usize> {
    let RelayResponse::Binary {
        status, mut stream, ..
    } = response
    else {
        return Err("expected binary media".into());
    };
    if !(200..300).contains(&status) {
        return Err("media delivery status failed".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "media body failed")?;
        if bytes.len() + chunk.len() > 32 * 1024 * 1024 {
            return Err("media exceeds acceptance limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return Err("empty media".into());
    }
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(&bytes)?;
    Ok(bytes.len())
}
