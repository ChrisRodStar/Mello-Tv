//! Baseline speed evaluation tool for Mello TV Rust backend.
//! Measures playback discovery, provider adapters, prober sampling, relay proxying, and storage.

use chrono::{DateTime, Utc};
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};
use stream_core::{
    domain::MediaType as DomainMediaType,
    providers::{
        DeviceCapabilities, Lookup, ProviderTitle, QualityPreference, StreamFormat, TmdbId,
    },
};
use stream_media::{
    DiscoveryCoordinator, HttpClient, MediaRelay, NativeProviders, RelayResponse,
    discovery::ProviderCatalog,
    prober::{ProbeLimits, StreamProber},
};
use stream_storage::{CatalogStore, catalog::CatalogWrite, db::Database};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("============================================================");
    println!("     MELLO TV RUST BACKEND — BASELINE SPEED EVALUATION      ");
    println!("============================================================");

    // ------------------------------------------------------------------------
    // PART 1: STORAGE & DATABASE BASELINES
    // ------------------------------------------------------------------------
    println!("\n>>> [1/4] BENCHMARKING STORAGE & SQLITE ACTOR...");
    let db = Database::in_memory(64).await?;
    let store = CatalogStore::from_database(db).await?;
    let now: DateTime<Utc> = SystemTime::now().into();

    // Seed 200 dummy catalog shelf records
    let mut batch = Vec::new();
    for i in 0..200 {
        let write = CatalogWrite {
            kind: "shelf".to_string(),
            key: format!("shelf_{:04}", i),
            data: json!({
                "id": format!("shelf_{i}"),
                "title": format!("Popular Trending Movies Category {i}"),
                "items": (0..20).map(|item_id| json!({
                    "id": item_id,
                    "title": format!("Movie Title #{item_id} in Shelf {i}"),
                    "poster": format!("https://image.tmdb.org/t/p/w500/sample_{i}_{item_id}.jpg")
                })).collect::<Vec<_>>()
            }),
            ttl: chrono::Duration::seconds(86400),
        };
        batch.push(write);
    }
    store.put_batch(batch, now).await?;

    // Benchmark 1.1: Shelf list latency (Home feed loading)
    let iters = 20;
    let mut shelf_times = Vec::new();
    for _ in 0..iters {
        let t0 = Instant::now();
        let shelves = store.list("shelf", 50, now).await?;
        assert_eq!(shelves.len(), 50);
        shelf_times.push(t0.elapsed().as_micros());
    }
    let avg_shelf_us = shelf_times.iter().sum::<u128>() / iters as u128;
    println!(
        "  * Shelf Listing (50 items, home screen): avg {} µs ({:.2} ms)",
        avg_shelf_us,
        avg_shelf_us as f64 / 1000.0
    );

    // Benchmark 1.2: Catalog search query (substring scan with json_extract)
    let mut search_times = Vec::new();
    for _ in 0..iters {
        let t0 = Instant::now();
        let _ = store.search("Category 15", 0, now).await?;
        search_times.push(t0.elapsed().as_micros());
    }
    let avg_search_us = search_times.iter().sum::<u128>() / iters as u128;
    println!(
        "  * Catalog Search (substring query): avg {} µs ({:.2} ms)",
        avg_search_us,
        avg_search_us as f64 / 1000.0
    );

    // Benchmark 1.3: Single record write transaction latency (under synchronous=FULL)
    let mut write_times = Vec::new();
    for i in 0..iters {
        let rec = CatalogWrite {
            kind: "test_write".to_string(),
            key: format!("key_{i}"),
            data: json!({"count": i}),
            ttl: chrono::Duration::seconds(3600),
        };
        let t0 = Instant::now();
        store.put(rec, now).await?;
        write_times.push(t0.elapsed().as_micros());
    }
    let avg_write_us = write_times.iter().sum::<u128>() / iters as u128;
    println!(
        "  * Single Write Transaction (synchronous=FULL): avg {} µs ({:.2} ms)",
        avg_write_us,
        avg_write_us as f64 / 1000.0
    );

    store.shutdown().await?;

    // ------------------------------------------------------------------------
    // PART 2: INDIVIDUAL PROVIDER ADAPTER LATENCY
    // ------------------------------------------------------------------------
    println!("\n>>> [2/4] BENCHMARKING INDIVIDUAL PROVIDER ADAPTERS...");
    // Test title: Whiplash (TMDB 244786, IMDb tt2582802)
    let test_lookup = Lookup {
        title: ProviderTitle {
            media_type: DomainMediaType::Movie,
            tmdb_id: TmdbId::try_from(244786)?,
            imdb_id: "tt2582802".to_string(),
            my_anime_list_id: None,
            season: None,
            episode: None,
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
        context_key: "bench_user".to_string(),
        startup_height: 720,
        wait_for_complete: false,
        refresh: true,
    };

    let catalog = NativeProviders::new()?;
    let providers = catalog.snapshot()?;
    println!("  Active providers registered: {}", providers.len());

    let mut working_candidates = Vec::new();

    for provider in &providers {
        let cancel = CancellationToken::new();
        let t0 = Instant::now();
        let res = tokio::time::timeout(
            Duration::from_secs(6),
            provider.resolve(&test_lookup, cancel.clone()),
        )
        .await;
        let elapsed_ms = t0.elapsed().as_millis();
        match res {
            Ok(Ok(result)) => {
                let count = result.candidates.as_ref().map_or(0, |c| c.len());
                println!(
                    "  * [{:<10}] RESOLVED in {:>4} ms ({} candidates)",
                    provider.id().as_str(),
                    elapsed_ms,
                    count
                );
                if let Some(mut candidates) = result.candidates {
                    for candidate in candidates.drain(..) {
                        working_candidates.push((provider.id().to_string(), candidate));
                    }
                }
            }
            Ok(Err(err)) => {
                println!(
                    "  * [{:<10}] FAILED in   {:>4} ms ({:?})",
                    provider.id().as_str(),
                    elapsed_ms,
                    err
                );
            }
            Err(_) => {
                println!(
                    "  * [{:<10}] TIMED OUT in {:>4} ms",
                    provider.id().as_str(),
                    elapsed_ms
                );
            }
        }
    }

    // ------------------------------------------------------------------------
    // PART 3: PROBER SAMPLING & STARTUP SPEED BREAKDOWN
    // ------------------------------------------------------------------------
    println!("\n>>> [3/4] BENCHMARKING STREAM PROBER (1-SAMPLE vs 3-SAMPLE)...");
    let http = Arc::new(HttpClient::new()?);

    // Filter to a reliable working candidate
    if let Some((prov, candidate)) = working_candidates.first().cloned() {
        println!(
            "  Selected test candidate from provider: '{}' ({})",
            prov, candidate.url
        );

        // 3.1: Probe with 3 samples (Current default)
        let prober_3 = StreamProber::new(
            http.clone(),
            ProbeLimits {
                samples_per_variant: 3,
                sample_bytes: 128 * 1024,
                total_sample_bytes: 16 * 1024 * 1024,
                max_variants: 1,
                ..ProbeLimits::default()
            },
        )?;
        let cancel = CancellationToken::new();
        let t0 = Instant::now();
        let res_3 = prober_3.probe_all(vec![candidate.clone()], &cancel).await?;
        let time_3_ms = t0.elapsed().as_millis();
        let sample_bytes_3 = res_3.first().map_or(0, |s| s.observation.sample_bytes);
        let samples_completed_3 = res_3.first().map_or(0, |s| s.observation.samples_completed);
        println!(
            "  * 3-SAMPLE Probe (Current Default): {} ms (samples: {}, bytes: {} B)",
            time_3_ms, samples_completed_3, sample_bytes_3
        );

        // 3.2: Probe with 1 sample (Proposed Netflix-style Fast Start)
        let prober_1 = StreamProber::new(
            http.clone(),
            ProbeLimits {
                samples_per_variant: 1,
                sample_bytes: 128 * 1024,
                total_sample_bytes: 16 * 1024 * 1024,
                max_variants: 1,
                ..ProbeLimits::default()
            },
        )?;
        let cancel = CancellationToken::new();
        let t0 = Instant::now();
        let res_1 = prober_1.probe_all(vec![candidate.clone()], &cancel).await?;
        let time_1_ms = t0.elapsed().as_millis();
        let sample_bytes_1 = res_1.first().map_or(0, |s| s.observation.sample_bytes);
        let samples_completed_1 = res_1.first().map_or(0, |s| s.observation.samples_completed);
        println!(
            "  * 1-SAMPLE Probe (Fast-Start Mode): {} ms (samples: {}, bytes: {} B)",
            time_1_ms, samples_completed_1, sample_bytes_1
        );
        println!(
            "  --> Potential Prober Time Saved:    {} ms ({:.1}% faster)",
            time_3_ms.saturating_sub(time_1_ms),
            (time_3_ms.saturating_sub(time_1_ms) as f64 / time_3_ms.max(1) as f64) * 100.0
        );
    }

    // ------------------------------------------------------------------------
    // PART 4: DISCOVERY COORDINATOR (COLD vs WARM) & RELAY LATENCY
    // ------------------------------------------------------------------------
    println!("\n>>> [4/4] BENCHMARKING END-TO-END DISCOVERY COORDINATOR & RELAY...");
    let shutdown = CancellationToken::new();
    let coordinator = DiscoveryCoordinator::default_live(shutdown.clone())?;

    // 4.1: Cold Resolution (Full pipeline: 11 providers + prober)
    let cancel = CancellationToken::new();
    let t0 = Instant::now();
    let cold_selection = coordinator.resolve(test_lookup.clone(), &cancel).await?;
    let cold_total_ms = t0.elapsed().as_millis();
    let selected_provider = cold_selection
        .selected
        .as_ref()
        .map(|s| s.candidate.source.provider.as_str())
        .unwrap_or("none");
    println!(
        "  * Cold DiscoveryCoordinator resolve: {} ms (Selected provider: {})",
        cold_total_ms, selected_provider
    );

    // 4.2: Warm Resolution (Immediate re-lookup: tests current cache behavior)
    let mut warm_lookup = test_lookup.clone();
    warm_lookup.refresh = false; // allow cache
    let cancel = CancellationToken::new();
    let t0 = Instant::now();
    let _warm_selection = coordinator.resolve(warm_lookup, &cancel).await?;
    let warm_total_ms = t0.elapsed().as_millis();
    let warm_total_us = t0.elapsed().as_micros();
    println!(
        "  * Warm DiscoveryCoordinator resolve: {} ms ({} µs) [Instant In-Memory Selection Cache]",
        warm_total_ms, warm_total_us
    );

    // 4.3: MediaRelay Segment Delivery & Header Inspection
    let relay = Arc::new(MediaRelay::new());
    let token = MediaRelay::generate_token();
    let session_id = "bench_session_1".to_string();
    relay.register_session(session_id.clone(), token.clone(), (*cold_selection).clone());

    // Fetch master manifest
    let t0 = Instant::now();
    let entry_res = relay.fetch_entry(&session_id, &token, None).await?;
    let entry_ms = t0.elapsed().as_millis();
    match &entry_res {
        RelayResponse::Manifest(manifest) => {
            println!(
                "  * MediaRelay Master Manifest Rewrite: {} ms (Manifest size: {} bytes)",
                entry_ms,
                manifest.len()
            );
        }
        _ => println!("  * MediaRelay Master Manifest: {} ms", entry_ms),
    }

    // Format info
    if let Some((fmt, _)) = relay.active_info(&session_id) {
        println!("  * MediaRelay Active Session Format: {:?}", fmt);
    }

    println!("\n============================================================");
    println!("                   BASELINE BENCHMARK COMPLETED             ");
    println!("============================================================");

    Ok(())
}
