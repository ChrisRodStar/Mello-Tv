//! Explicit preparation outside the store owner, followed by its existing CAS transaction.
use super::*;
use crate::contracts::{MutationBase, SessionReady, UpdateSource, providers::Selection};
use crate::session::Registration;
use tokio_util::sync::CancellationToken;
const BUDGET: Error = Error(StatusCode::TOO_MANY_REQUESTS, "preparation_limit");
const CANCELLED: Error = Error(StatusCode::REQUEST_TIMEOUT, "preparation_cancelled");

impl Delivery {
    pub async fn replace(&self, sid: String, request: UpdateSource) -> Result<SessionReady, Error> {
        self.recover(
            sid,
            MutationBase {
                owner_id: request.owner_id,
                expected_revision: request.expected_revision,
            },
            Some(request.selection),
        )
        .await
    }
    pub async fn failover(
        &self,
        sid: String,
        request: MutationBase,
    ) -> Result<SessionReady, Error> {
        self.recover(sid, request, None).await
    }
    async fn recover(
        &self,
        sid: String,
        request: MutationBase,
        replacement: Option<Selection>,
    ) -> Result<SessionReady, Error> {
        let _time = crate::health::Timer::start(crate::health::Stage::Preparation);
        let refresh = replacement.is_some();
        let snapshot = self
            .store
            .recovery(
                sid.clone(),
                request.owner_id.clone(),
                request.expected_revision,
                replacement,
            )
            .await?;
        let deadline =
            lease_deadline(snapshot.expires_at)?.min(Instant::now() + Duration::from_secs(8));
        let cancel = snapshot.cancelled.child_token();
        // Dropping the HTTP handler or its timeout cancels queued publication too.
        let _on_drop = cancel.clone().drop_guard();
        let mut selection = snapshot.selection;
        let candidates = if refresh {
            vec![selection.selected.clone().ok_or(STORE)?]
        } else {
            selection.backups.take().unwrap_or_default()
        };
        let mut bytes_left = self.config.limits.max_playlist_bytes as usize;
        for (index, candidate) in candidates.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(CANCELLED);
            }
            if Instant::now() >= deadline {
                return Err(Error(StatusCode::GATEWAY_TIMEOUT, "upstream_timeout"));
            }
            if candidate
                .candidate
                .source
                .expires_at
                .is_some_and(|v| v.timestamp() <= chrono::Utc::now().timestamp() + 5)
            {
                crate::health::failure("source_expired");
                if refresh {
                    return Err(Error(StatusCode::GONE, "source_expired"));
                }
                continue;
            }
            selection.selected = Some(candidate.clone());
            if !refresh {
                selection.backups = Some(candidates[index + 1..].to_vec());
            }
            crate::validation::selection(&selection, &self.config, chrono::Utc::now())?;
            let epoch = self.transport.guard().lease().map_err(network)?;
            let root = Store::root(&selection)?;
            let expires = root
                .expires_at
                .unwrap_or(snapshot.expires_at)
                .min(snapshot.expires_at);
            let candidate_deadline = deadline.min(lease_deadline(expires)?);
            let prepared = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(CANCELLED),
                r = tokio::time::timeout_at(candidate_deadline, self.prepare(&sid, root, expires, cancel.clone(), candidate_deadline, &mut bytes_left)) =>
                    r.map_err(|_| Error(StatusCode::GATEWAY_TIMEOUT, "upstream_timeout"))?,
            };
            let batch = match prepared {
                Ok(batch) => batch,
                Err(error)
                    if !refresh
                        && matches!(
                            error.1,
                            "upstream_rejected"
                                | "invalid_manifest"
                                | "unsupported_manifest"
                                | "range_mismatch"
                        ) =>
                {
                    crate::health::failure(error.1);
                    continue;
                }
                Err(error) => return Err(error),
            };
            // Recheck the VPN epoch before publication, including a loss after validation.
            if !self.transport.guard().healthy() {
                return Err(Error(StatusCode::SERVICE_UNAVAILABLE, "vpn_unavailable"));
            }
            return self
                .store
                .activate(
                    sid,
                    UpdateSource {
                        owner_id: request.owner_id,
                        expected_revision: request.expected_revision,
                        selection,
                    },
                    batch,
                    cancel,
                    epoch.cancel,
                    candidate_deadline,
                )
                .await;
        }
        self.store
            .check_mutation(sid, request.owner_id, request.expected_revision)
            .await?;
        Err(Error(StatusCode::CONFLICT, "source_refresh_required"))
    }
    async fn prepare(
        &self,
        sid: &str,
        root: Registration,
        expires: i64,
        cancel: CancellationToken,
        deadline: Instant,
        bytes_left: &mut usize,
    ) -> Result<Vec<Registration>, Error> {
        if root.kind == ResourceKind::Mp4 {
            if *bytes_left < 16 {
                return Err(BUDGET);
            }
            *bytes_left -= 16;
            // HEAD delivery validates an ordinary GET's framing and bounded prefix,
            // then closes the unused body. This is the actual stage-5 validator.
            self.serve_inner(
                sid,
                "",
                Authorized {
                    registration: Some(root),
                    generation: 0,
                    expires_at: expires,
                    cancelled: cancel,
                    reader: None,
                },
                Method::HEAD,
                &HeaderMap::new(),
                deadline,
            )
            .await?;
            return Ok(vec![]);
        }
        let mut pending = vec![root];
        let mut batch = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        while let Some(registration) = pending.pop() {
            // Repeated graph nodes with distinct ancestry must still be parsed so
            // cycles and selected-variant/group constraints cannot hide in aliases.
            let fingerprint = serde_json::to_vec(&registration).map_err(|_| STORE)?;
            if !seen.insert(fingerprint) {
                continue;
            }
            if seen.len() > self.config.limits.max_manifest_registrations_count as usize {
                return Err(BUDGET);
            }
            let mut upstream = self
                .transport
                .fetch(Fetch {
                    url: &registration.uri,
                    method: Method::GET,
                    context: &registration.context,
                    session_id: sid,
                    deadline,
                    cancel: cancel.clone(),
                    range: None,
                    if_range: None,
                })
                .await
                .map_err(network)?;
            identity(&upstream.headers)?;
            if upstream.status != StatusCode::OK {
                return Err(REJECTED);
            }
            let final_url = upstream.url.clone();
            let mut bytes = Vec::new();
            while let Some(chunk) = upstream.chunk().await.map_err(network)? {
                if chunk.len() > *bytes_left {
                    return Err(BUDGET);
                }
                *bytes_left -= chunk.len();
                bytes.extend_from_slice(&chunk);
            }
            drop(upstream);
            let _manifest_time = crate::health::Timer::start(crate::health::Stage::Manifest);
            let plan = hls::parse(
                &bytes,
                &final_url,
                &registration,
                self.config.limits.max_playlist_depth_count as usize,
                self.config.limits.max_manifest_registrations_count as usize,
                self.config.state.max_snapshot_bytes as usize,
            )?;
            let references = plan.registrations.clone();
            // Validate serialization and redaction before durable activation; these
            // placeholders are local only and are never returned as resource IDs.
            let ids = vec!["x".repeat(43); references.len()];
            let reserved_grant = "x".repeat(self.config.auth.max_grant_bytes as usize);
            let output = plan.publish(
                &ids,
                sid,
                &reserved_grant,
                self.config.limits.max_playlist_bytes as usize,
            )?;
            redact(&output, &registration, &final_url, &references)?;
            for reference in references {
                if reference.kind == ResourceKind::Manifest {
                    pending.push(reference.clone());
                }
                batch.push(reference);
                if batch.len() > self.config.limits.max_manifest_registrations_count as usize
                    || serde_json::to_vec(&batch).map_err(|_| STORE)?.len()
                        > self.config.state.max_snapshot_bytes as usize
                {
                    return Err(BUDGET);
                }
            }
        }
        Ok(batch)
    }
}
