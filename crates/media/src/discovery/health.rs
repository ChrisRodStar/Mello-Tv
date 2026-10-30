use super::{DiscoveryCoordinator, DiscoveryError, DiscoveryProvider};
use crate::prober::ProbeOptions;
use chrono::{DateTime, Utc};
use futures_util::{StreamExt, future::BoxFuture, stream::FuturesUnordered};
use semver::Version;
use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, SystemTime},
};
use stream_core::{
    ports::media::ProviderHealth,
    providers::{
        DeviceCapabilities, ErrorCategory, Lookup, ProviderId, ProviderTitle, QualityPreference,
        StreamFormat, TmdbId,
    },
};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

const CACHE_TTL: Duration = Duration::from_secs(60);
const SAMPLE_TIMEOUT: Duration = Duration::from_secs(18);
// Broadly available reference titles; the second avoids treating one missing
// title as a provider outage. Checks do not create playback/history records.
const REFERENCES: [(i64, &str); 2] = [(27205, "tt1375666"), (157336, "tt0816692")];

pub(super) struct CachedHealth {
    generations: Vec<(ProviderId, Version, u64)>,
    expires_at: Instant,
    reports: Vec<ProviderHealth>,
}

impl DiscoveryCoordinator {
    pub(super) async fn run_provider_health_checks(
        &self,
    ) -> Result<Vec<ProviderHealth>, DiscoveryError> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(DiscoveryError::Closed);
        }
        let providers = self.inner.providers.snapshot()?;
        if providers.len() > 32 {
            return Err(DiscoveryError::Capacity);
        }
        let generations = providers
            .iter()
            .map(|provider| {
                (
                    provider.id().clone(),
                    provider.version().clone(),
                    provider.generation(),
                )
            })
            .collect::<Vec<_>>();

        // Hold one batch owner across the await. Simultaneous callers share the
        // completed cached result; cancellation drops the owner and its probes.
        let mut cached = self.inner.health_results.lock().await;
        if let Some(previous) = cached.as_ref()
            && previous.expires_at > Instant::now()
            && previous.generations == generations
        {
            return Ok(previous.reports.clone());
        }
        let cancel = CancellationToken::new();
        let _cancel_on_drop = cancel.clone().drop_guard();
        let mut pending: FuturesUnordered<BoxFuture<'static, ProviderHealth>> =
            FuturesUnordered::new();
        let mut reports = Vec::with_capacity(providers.len());
        for provider in providers {
            let coordinator = self.clone();
            let child = cancel.child_token();
            pending.push(Box::pin(async move {
                coordinator.check_health_sample(provider, child).await
            }));
            if pending.len() == 5
                && let Some(report) = pending.next().await
            {
                reports.push(report);
            }
        }
        while let Some(report) = pending.next().await {
            reports.push(report);
        }
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(DiscoveryError::Closed);
        }
        *cached = Some(CachedHealth {
            generations,
            expires_at: Instant::now() + CACHE_TTL,
            reports: reports.clone(),
        });
        Ok(reports)
    }

    async fn check_health_sample(
        &self,
        provider: Arc<dyn DiscoveryProvider>,
        cancel: CancellationToken,
    ) -> ProviderHealth {
        let _cancel_on_drop = cancel.clone().drop_guard();
        let started = Instant::now();
        let mut detail = "No reference media could be verified.";
        let mut working = false;
        let mut samples_checked = 0;
        for (tmdb_id, imdb_id) in REFERENCES {
            samples_checked += 1;
            let attempt_cancel = cancel.child_token();
            let _cancel_attempt = attempt_cancel.clone().drop_guard();
            let result = tokio::time::timeout(SAMPLE_TIMEOUT, async {
                let _slot = self
                    .inner
                    .slots
                    .acquire()
                    .await
                    .map_err(|_| ErrorCategory::Unavailable)?;
                let lookup = reference_lookup(tmdb_id, imdb_id)?;
                // Call the provider directly: no playback observation or resolver
                // result cache can masquerade as an independent health result.
                let response = provider.resolve(&lookup, attempt_cancel.clone()).await?;
                response
                    .validate_for(provider.id(), &lookup.title, 32)
                    .map_err(|_| ErrorCategory::ProtocolChange)?;
                let candidates = response
                    .candidates
                    .unwrap_or_default()
                    .into_iter()
                    .take(2)
                    .collect::<Vec<_>>();
                if candidates.is_empty() {
                    return Ok(false);
                }
                let probes = self
                    .inner
                    .prober
                    .health_probe()
                    .probe_all_with_options(
                        candidates,
                        ProbeOptions {
                            max_height: None,
                            startup_height: Some(720),
                        },
                        &attempt_cancel,
                    )
                    .await
                    .map_err(|_| ErrorCategory::Unavailable)?;
                // Provider health is not a device codec or bandwidth verdict.
                // Require actual sampled media bytes and known protection instead
                // of applying a specific viewer's playback eligibility rules.
                Ok::<_, ErrorCategory>(probes.iter().any(|probe| {
                    let evidence = &probe.observation;
                    evidence.accessible
                        && evidence.failure.is_none()
                        && evidence.samples_completed > 0
                        && evidence.sample_bytes > 0
                        && evidence.protection_known
                }))
            })
            .await;
            match result {
                Ok(Ok(true)) => {
                    working = true;
                    detail = "Reference media resolved and verified.";
                    break;
                }
                Ok(Ok(false)) => detail = "Reference media access could not be verified.",
                Ok(Err(ErrorCategory::Timeout)) | Err(_) => {
                    detail = "The independent check timed out."
                }
                Ok(Err(ErrorCategory::ProtocolChange)) => {
                    detail = "The provider returned an unexpected response."
                }
                Ok(Err(ErrorCategory::UpstreamFailure)) => {
                    detail = "The provider's upstream request failed."
                }
                Ok(Err(_)) => detail = "The provider could not resolve reference media.",
            }
        }
        let checked_at: DateTime<Utc> = SystemTime::now().into();
        ProviderHealth {
            id: provider.id().to_string(),
            version: provider.version().to_string(),
            working,
            checked_at: checked_at.timestamp(),
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            samples_checked,
            detail: detail.into(),
        }
    }
}

fn reference_lookup(tmdb_id: i64, imdb_id: &str) -> Result<Lookup, ErrorCategory> {
    let id = TmdbId::try_from(tmdb_id).map_err(|_| ErrorCategory::ProtocolChange)?;
    let mut title = ProviderTitle::movie(id);
    title.imdb_id = imdb_id.into();
    Ok(Lookup {
        title,
        preferences: QualityPreference::default(),
        device: DeviceCapabilities {
            formats: vec![StreamFormat::Hls, StreamFormat::Mp4],
            codecs: vec!["h264".into(), "aac".into()],
            max_height: 0,
            allow_unknown_codecs: true,
            allow_unknown_resolution: true,
            supports_encrypted_hls: true,
        },
        context_key: "provider-health".into(),
        startup_height: 720,
        wait_for_complete: true,
        refresh: true,
    })
}
