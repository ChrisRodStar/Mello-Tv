use super::*;

impl Engine {
    pub(super) fn validate_resource(&self, r: &Registration) -> Result<(), Error> {
        validation::source_url(&r.uri)?;
        if let Some(manifest) = &r.manifest {
            if r.kind != ResourceKind::Manifest
                || manifest.ancestors.len() > self.config.limits.max_playlist_depth_count as usize
            {
                return Err(INVALID);
            }
            for ancestor in &manifest.ancestors {
                validation::source_url(ancestor)?;
            }
            if let Some(selected) = &manifest.selected_variant {
                validation::source_url(selected)?;
            }
            if manifest.adaptive_variants.len() > 32 {
                return Err(INVALID);
            }
            for variant in &manifest.adaptive_variants {
                validation::source_url(&variant.url)?;
            }
        }
        validation::context(&r.context)?;
        if r.range
            .as_ref()
            .is_some_and(|v| v.length == 0 || v.offset.checked_add(v.length).is_none())
        {
            return Err(INVALID);
        }
        if serde_json::to_vec(r).map_err(|_| STORE)?.len()
            > self.config.state.max_snapshot_bytes as usize
        {
            return Err(INVALID);
        }
        Ok(())
    }
    pub(super) fn resource_id(
        &self,
        s: &Session,
        n: u64,
        r: &Registration,
    ) -> Result<String, Error> {
        let secret = zeroize::Zeroizing::new(B64.decode(&s.secret).map_err(|_| STORE)?);
        let mut mac = Hmac::<Sha256>::new_from_slice(&secret).map_err(|_| STORE)?;
        mac.update(&serde_json::to_vec(&("resource-v1", &s.sid, n, r)).map_err(|_| STORE)?);
        Ok(B64.encode(mac.finalize().into_bytes()))
    }
    pub(super) fn resource_uncached(
        &self,
        s: &Session,
        n: u64,
        rid: &str,
    ) -> Result<Registration, Error> {
        let blob: Vec<u8> = self
            .db
            .query_row(
                "SELECT blob FROM resources WHERE sid=?1 AND generation=?2 AND rid=?3",
                params![s.sid, sql_u64(n)?, rid],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| STORE)?
            .ok_or(UNKNOWN)?;
        let r: Registration = self
            .crypto
            .open(&format!("resource:{}:{n}:{rid}", s.sid), &blob)?;
        if self.resource_id(s, n, &r)? != rid {
            return Err(STORE);
        }
        Ok(r)
    }
    pub(super) fn resource(
        &mut self,
        s: &Session,
        n: u64,
        rid: &str,
    ) -> Result<Registration, Error> {
        let key = (s.sid.clone(), n, rid.to_owned());
        let r = if let Some((r, _)) = self.cache.get(&key) {
            serde_json::from_slice(r).map_err(|_| STORE)?
        } else {
            let r = self.resource_uncached(s, n, rid)?;
            let bytes = zeroize::Zeroizing::new(serde_json::to_vec(&r).map_err(|_| STORE)?);
            let size = bytes.capacity() + key.0.capacity() + key.2.capacity() + 512;
            if size <= self.config.state.hot_index_bytes as usize {
                while self.cache_bytes + size > self.config.state.hot_index_bytes as usize {
                    if let Some(old) = self.order.pop_front() {
                        if let Some((_, len)) = self.cache.remove(&old) {
                            self.cache_bytes -= len;
                        }
                    } else {
                        break;
                    }
                }
                self.order.push_back(key.clone());
                self.cache.insert(key, (bytes, size));
                self.cache_bytes += size;
            }
            r
        };
        if r.expires_at.is_some_and(|v| v <= Utc::now().timestamp()) {
            return Err(Error(StatusCode::GONE, "source_expired"));
        }
        Ok(r)
    }
    pub(crate) fn root(selection: &Selection) -> Result<Registration, Error> {
        let selected = selection.selected.as_ref().ok_or(INVALID)?;
        let source = &selected.candidate.source;
        let ladder = adaptive_variants(selection);
        let expiry = std::iter::once(selected)
            .chain(selection.backups.iter().flatten())
            .filter(|ranked| {
                ranked.candidate.source.id == source.id
                    || ladder.iter().any(|variant| {
                        Some(variant.url.as_str())
                            == ranked.candidate.source.selected_variant_url.as_deref()
                    })
            })
            .filter_map(|ranked| {
                ranked
                    .candidate
                    .source
                    .expires_at
                    .map(|expiry| expiry.timestamp())
            })
            .min();
        Ok(Registration {
            manifest: if selected.candidate.observation.format == "hls" {
                Some(ManifestContext {
                    ancestors: vec![],
                    selected_variant: source.selected_variant_url.clone(),
                    adaptive_variants: ladder,
                    media_only: false,
                    subtitles: false,
                })
            } else {
                None
            },
            encrypted: false,
            uri: source.url.clone(),
            kind: if selected.candidate.observation.format == "hls" {
                ResourceKind::Manifest
            } else {
                ResourceKind::Mp4
            },
            context: source.fetch_context.clone(),
            range: None,
            expires_at: expiry,
        })
    }
    pub(super) fn check_generation_capacity(&self, sid: &str) -> Result<(), Error> {
        let retained: i64 = self
            .db
            .query_row(
                "SELECT count(*) FROM generations WHERE sid=?1",
                [sid],
                |r| r.get(0),
            )
            .map_err(|_| STORE)?;
        // Empty rows awaiting the next cleanup batch still consume a snapshot
        // slot. Admission and startup must use the same retained-state bounds.
        if retained as u64 >= self.config.state.max_resources_per_session_count
            || self.count("generations")? >= self.config.state.max_resources_total_count
        {
            return Err(LIMIT);
        }
        Ok(())
    }
    pub(super) fn check_capacity(&self, sid: &str, new: usize) -> Result<(), Error> {
        let current: i64 = self
            .db
            .query_row("SELECT count(*) FROM resources WHERE sid=?1", [sid], |r| {
                r.get(0)
            })
            .map_err(|_| STORE)?;
        if current as u64 + new as u64 > self.config.state.max_resources_per_session_count
            || self.count("resources")? + new as u64 > self.config.state.max_resources_total_count
        {
            return Err(LIMIT);
        }
        Ok(())
    }
    pub(super) fn register(
        &mut self,
        sid: String,
        n: u64,
        batch: Vec<Registration>,
    ) -> Result<Vec<String>, Error> {
        let s = self.session(&sid)?;
        self.active(&s)?;
        let g = self.generation(&sid, n)?;
        self.valid_generation(&g)?;
        if batch.is_empty()
            || batch.len() as u64 > self.config.limits.max_manifest_registrations_count
        {
            return Err(LIMIT);
        }
        let total = serde_json::to_vec(&batch).map_err(|_| INVALID)?.len();
        if total > self.config.state.max_snapshot_bytes as usize {
            return Err(LIMIT);
        }
        let mut records = BTreeMap::new();
        let mut ids = Vec::with_capacity(batch.len());
        for r in batch {
            self.validate_resource(&r)?;
            let rid = self.resource_id(&s, n, &r)?;
            ids.push(rid.clone());
            let exists: bool = self
                .db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM resources WHERE sid=?1 AND rid=?2)",
                    params![sid, rid],
                    |r| r.get(0),
                )
                .map_err(|_| STORE)?;
            if !exists {
                let blob = self.crypto.seal(&format!("resource:{sid}:{n}:{rid}"), &r)?;
                records.insert(rid, blob);
            }
        }
        if n != s.generation && !records.is_empty() {
            return Err(CONFLICT);
        }
        self.check_capacity(&sid, records.len())?;
        self.writable()?;
        let tx = self.db.transaction().map_err(|_| STORE)?;
        for (rid, blob) in records {
            tx.execute(
                "INSERT INTO resources VALUES(?1,?2,?3,?4)",
                params![sid, sql_u64(n)?, rid, blob],
            )
            .map_err(|_| STORE)?;
        }
        tx.commit().map_err(|_| STORE)?;
        Ok(ids)
    }
    pub(super) fn lookup(
        &mut self,
        sid: String,
        resource: Resource,
    ) -> Result<AuthorizationRecord, Error> {
        let s = self.session(&sid)?;
        self.active(&s)?;
        let n = match &resource {
            Resource::Entry => s.generation,
            Resource::Registered(rid) => self
                .db
                .query_row(
                    "SELECT generation FROM resources WHERE sid=?1 AND rid=?2",
                    params![sid, rid],
                    |r| r.get::<_, i64>(0),
                )
                .optional()
                .map_err(|_| STORE)?
                .ok_or(UNKNOWN)?
                .try_into()
                .map_err(|_| STORE)?,
        };
        let g = self.generation(&sid, n)?;
        self.valid_generation(&g)?;
        let rid = match &resource {
            Resource::Entry => &g.root,
            Resource::Registered(rid) => rid,
        };
        let mut r = self.resource(&s, n, rid)?;
        if rid == &g.root && r.kind == ResourceKind::Manifest && r.manifest.is_none() {
            r.manifest = Self::root(&g.selection)?.manifest;
        }
        let lease = self
            .leases
            .entry((sid.clone(), n))
            .or_insert_with(|| GenerationLease {
                cancelled: CancellationToken::new(),
                readers: Arc::new(()),
            });
        Ok(AuthorizationRecord {
            registration: Some(r.clone()),
            session_id: sid.clone(),
            owner_id: s.owner,
            auth_revision: s.auth_revision,
            expires_at: s.expires,
            source_expires_at: g
                .selection
                .selected
                .as_ref()
                .ok_or(STORE)?
                .candidate
                .source
                .expires_at
                .map(|v| v.timestamp())
                .into_iter()
                .chain(r.expires_at)
                .min(),
            revoked: s.revoked,
            resource,
            resource_session_id: sid,
            source_generation: s.generation,
            resource_generation: n,
            generation_valid_until: g.valid_until,
            cancelled: lease.cancelled.clone(),
            reader: Some(lease.readers.clone()),
        })
    }
}

// Only measured rungs from the same master/context form a ladder. Different
// providers and timelines stay explicit whole-source failover generations.
fn adaptive_variants(selection: &Selection) -> Vec<crate::contracts::providers::Variant> {
    let Some(selected) = &selection.selected else {
        return vec![];
    };
    let top = &selected.candidate;
    let Some(top_variant) = &top.observation.variant else {
        return vec![];
    };
    let codecs =
        |variant: &crate::contracts::providers::Variant| -> std::collections::BTreeSet<String> {
            variant
                .codecs
                .iter()
                .flatten()
                .map(|codec| codec.split('.').next().unwrap_or("").to_ascii_lowercase())
                .collect()
        };
    let families = codecs(top_variant);
    if families.is_empty()
        || top.source.selected_variant_url.as_deref() != Some(top_variant.url.as_str())
    {
        return vec![];
    }
    let mut verified = Vec::new();
    for ranked in std::iter::once(selected).chain(selection.backups.iter().flatten()) {
        let candidate = &ranked.candidate;
        let observation = &candidate.observation;
        let Some(variant) = &observation.variant else {
            continue;
        };
        if observation.format != "hls"
            || !observation.accessible
            || !observation.protection_known
            || observation.failure.is_some()
            || observation.samples_completed < 3
            || observation.encrypted != top.observation.encrypted
            || candidate.source.url != top.source.url
            || candidate.source.provider != top.source.provider
            || candidate.source.fetch_context != top.source.fetch_context
            || candidate.source.selected_variant_url.as_deref() != Some(variant.url.as_str())
            || variant.audio_group != top_variant.audio_group
            || variant.subtitle_group != top_variant.subtitle_group
            || codecs(variant) != families
            || variant
                .height
                .is_none_or(|height| height <= 0 || Some(height) > top_variant.height)
            || variant.width.is_none_or(|width| width <= 0)
            || variant.bitrate.is_none_or(|bitrate| {
                bitrate <= 0
                    || observation
                        .delivery_bps
                        .is_none_or(|speed| !speed.is_finite() || speed < bitrate as f64 * 1.5)
            })
            || candidate
                .source
                .expires_at
                .is_some_and(|expiry| expiry <= Utc::now())
        {
            continue;
        }
        if !verified
            .iter()
            .any(|old: &crate::contracts::providers::Variant| old.url == variant.url)
        {
            verified.push(variant.clone());
        }
    }
    if verified.len() > 1
        && verified
            .iter()
            .any(|variant| variant.url == top_variant.url)
    {
        verified
    } else {
        vec![]
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod adaptive_tests {
    use super::*;
    #[test]
    fn root_ladder_is_same_source_measured_compatible_and_expiry_bounded() {
        let mut input: serde_json::Value = serde_json::from_str(include_str!(
            "../../../contracts/examples/create-session.json"
        ))
        .unwrap();
        let expires = Utc::now() + chrono::Duration::hours(1);
        let ranked = &mut input["selection"]["selected"];
        ranked["candidate"]["source"]["url"] =
            serde_json::json!("https://media.example.com/master.m3u8");
        ranked["candidate"]["source"]["expires_at"] = serde_json::json!(expires.to_rfc3339());
        ranked["candidate"]["source"]["selected_variant_url"] =
            serde_json::json!("https://media.example.com/1080.m3u8");
        ranked["candidate"]["observation"]["variant"] = serde_json::json!({"url":"https://media.example.com/1080.m3u8","width":1920,"height":1080,"bitrate":2000000,"codecs":["avc1.640028"]});
        ranked["candidate"]["observation"]["samples_completed"] = serde_json::json!(3);
        ranked["candidate"]["observation"]["delivery_bps"] = serde_json::json!(8000000);
        let mut lower = ranked.clone();
        lower["candidate"]["source"]["id"] = serde_json::json!("lower");
        lower["candidate"]["source"]["selected_variant_url"] =
            serde_json::json!("https://media.example.com/720.m3u8");
        lower["candidate"]["source"]["expires_at"] =
            serde_json::json!((expires - chrono::Duration::minutes(10)).to_rfc3339());
        lower["candidate"]["observation"]["variant"] = serde_json::json!({"url":"https://media.example.com/720.m3u8","width":1280,"height":720,"bitrate":1000000,"codecs":["avc1.64001f"]});
        input["selection"]["backups"] = serde_json::json!([lower]);
        let selection: Selection = serde_json::from_value(input["selection"].clone()).unwrap();
        let root = Engine::root(&selection).unwrap();
        assert_eq!(root.manifest.as_ref().unwrap().adaptive_variants.len(), 2);
        assert_eq!(
            root.expires_at,
            Some((expires - chrono::Duration::minutes(10)).timestamp())
        );
        for (path, value) in [
            (
                "/candidate/source/provider",
                serde_json::json!("other-provider"),
            ),
            (
                "/candidate/source/url",
                serde_json::json!("https://different.example.com/master.m3u8"),
            ),
            (
                "/candidate/observation/samples_completed",
                serde_json::json!(0),
            ),
            (
                "/candidate/observation/variant/codecs",
                serde_json::json!(["hev1"]),
            ),
            ("/candidate/source/fetch_context", serde_json::json!({})),
        ] {
            let mut changed = input["selection"].clone();
            *changed["backups"][0].pointer_mut(path).unwrap() = value;
            let changed: Selection = serde_json::from_value(changed).unwrap();
            assert!(
                Engine::root(&changed)
                    .unwrap()
                    .manifest
                    .unwrap()
                    .adaptive_variants
                    .is_empty()
            );
        }
        let legacy = serde_json::to_string(&ManifestContext {
            ancestors: vec![],
            selected_variant: None,
            adaptive_variants: vec![],
            media_only: false,
            subtitles: false,
        })
        .unwrap();
        assert!(!legacy.contains("adaptive_variants"));
    }
}
