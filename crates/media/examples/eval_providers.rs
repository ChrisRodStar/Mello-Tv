//! Detailed per-provider speed evaluation runner for Mello TV.
//! Tests Movie and TV titles across all 11 active providers.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use stream_core::{
    domain::MediaType,
    providers::{
        DeviceCapabilities, Lookup, ProviderTitle, QualityPreference, StreamFormat, TmdbId,
    },
};
use stream_media::{
    discovery::ProviderCatalog,
    network::HttpClient,
    prober::{ProbeLimits, StreamProber},
    providers::NativeProviders,
};
use tokio_util::sync::CancellationToken;

struct TestTitle {
    name: &'static str,
    media_type: MediaType,
    tmdb_id: i64,
    imdb_id: &'static str,
    season: Option<i64>,
    episode: Option<i64>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("================================================================================");
    println!("               MELLO TV: DETAILED PER-PROVIDER PERFORMANCE BENCHMARK            ");
    println!("================================================================================");

    let titles = [
        TestTitle {
            name: "Whiplash (2014 Movie)",
            media_type: MediaType::Movie,
            tmdb_id: 244786,
            imdb_id: "tt2582802",
            season: None,
            episode: None,
        },
        TestTitle {
            name: "Severance (S01E01 TV)",
            media_type: MediaType::Tv,
            tmdb_id: 97951,
            imdb_id: "tt11280740",
            season: Some(1),
            episode: Some(1),
        },
    ];

    let catalog = NativeProviders::new()?;
    let providers = catalog.snapshot()?;
    let http = Arc::new(HttpClient::new()?);
    let prober = StreamProber::new(
        http,
        ProbeLimits {
            samples_per_variant: 1,
            sample_bytes: 128 * 1024,
            total_sample_bytes: 16 * 1024 * 1024,
            max_variants: 2,
            ..ProbeLimits::default()
        },
    )?;

    for title in &titles {
        println!(
            "\n--------------------------------------------------------------------------------"
        );
        println!(">>> BENCHMARKING: {}", title.name);
        println!(
            "--------------------------------------------------------------------------------"
        );
        println!(
            "{:<12} | {:<12} | {:<12} | {:<10} | {:<12} | {:<12}",
            "Provider", "Resolve (ms)", "Probe (ms)", "Candidates", "Playable?", "Format"
        );
        println!(
            "{:-<12}-+-{:-<12}-+-{:-<12}-+-{:-<10}-+-{:-<12}-+-{:-<12}",
            "", "", "", "", "", ""
        );

        let lookup = Lookup {
            title: ProviderTitle {
                media_type: title.media_type,
                tmdb_id: TmdbId::try_from(title.tmdb_id)?,
                imdb_id: title.imdb_id.to_string(),
                my_anime_list_id: None,
                season: title.season,
                episode: title.episode,
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
            context_key: "eval_user".to_string(),
            startup_height: 720,
            wait_for_complete: false,
            refresh: true,
        };

        for provider in &providers {
            let cancel = CancellationToken::new();
            let t0 = Instant::now();
            let resolved = tokio::time::timeout(
                Duration::from_secs(6),
                provider.resolve(&lookup, cancel.clone()),
            )
            .await;
            let resolve_ms = t0.elapsed().as_millis();

            match resolved {
                Ok(Ok(result)) => {
                    let candidates = result.candidates.unwrap_or_default();
                    let candidate_count = candidates.len();
                    if candidate_count == 0 {
                        println!(
                            "{:<12} | {:>10} ms | {:>10}    | {:>10} | {:<12} | {:<12}",
                            provider.id().as_str(),
                            resolve_ms,
                            "-",
                            0,
                            "No Candidates",
                            "-"
                        );
                        continue;
                    }

                    // Probe top 1 candidate
                    let top_candidate = candidates[0].clone();
                    let format_str = format!("{:?}", top_candidate.format);
                    let t_probe = Instant::now();
                    let probe_res = tokio::time::timeout(
                        Duration::from_secs(5),
                        prober.probe_all(vec![top_candidate], &cancel),
                    )
                    .await;
                    let probe_ms = t_probe.elapsed().as_millis();

                    let is_accessible = match probe_res {
                        Ok(Ok(probed)) => probed.first().is_some_and(|s| s.observation.accessible),
                        _ => false,
                    };

                    println!(
                        "{:<12} | {:>10} ms | {:>10} ms | {:>10} | {:<12} | {:<12}",
                        provider.id().as_str(),
                        resolve_ms,
                        probe_ms,
                        candidate_count,
                        if is_accessible {
                            "Yes (Working)"
                        } else {
                            "Unreachable"
                        },
                        format_str
                    );
                }
                Ok(Err(e)) => {
                    println!(
                        "{:<12} | {:>10} ms | {:>10}    | {:>10} | {:<12} | {:<12}",
                        provider.id().as_str(),
                        resolve_ms,
                        "-",
                        "-",
                        format!("Err: {:?}", e),
                        "-"
                    );
                }
                Err(_) => {
                    println!(
                        "{:<12} | {:>10} ms | {:>10}    | {:>10} | {:<12} | {:<12}",
                        provider.id().as_str(),
                        resolve_ms,
                        "-",
                        "-",
                        "TIMEOUT (6s)",
                        "-"
                    );
                }
            }
        }
    }

    println!("\n================================================================================");
    println!("                     BENCHMARK RUN COMPLETE                                     ");
    println!("================================================================================");
    Ok(())
}
