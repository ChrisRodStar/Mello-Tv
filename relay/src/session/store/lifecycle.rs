use super::*;

impl Engine {
    pub(super) fn active(&self, s: &Session) -> Result<(), Error> {
        if s.revoked {
            Err(REVOKED)
        } else if s.expires <= Utc::now().timestamp() {
            Err(EXPIRED)
        } else {
            Ok(())
        }
    }
    pub(super) fn valid_generation(&self, g: &Generation) -> Result<(), Error> {
        let now = Utc::now().timestamp();
        if g.valid_until.is_some_and(|v| v <= now) {
            return Err(UNKNOWN);
        }
        if g.selection
            .selected
            .as_ref()
            .ok_or(STORE)?
            .candidate
            .source
            .expires_at
            .is_some_and(|v| v.timestamp() <= now)
        {
            return Err(Error(StatusCode::GONE, "source_expired"));
        }
        Ok(())
    }
    pub(super) fn ready(&self, s: &Session, resume: bool) -> Result<SessionReady, Error> {
        let format = match s.format.as_str() {
            "hls" => PlaybackFormat::Hls,
            "mp4" => PlaybackFormat::Mp4,
            _ => return Err(STORE),
        };
        let available_heights = if !s.revoked && s.expires > Utc::now().timestamp() {
            let generation = self.generation(&s.sid, s.generation)?;
            let root = self.resource_uncached(s, s.generation, &generation.root)?;
            quality_heights(&root, &generation.selection)
        } else {
            None
        };
        Ok(SessionReady {
            available_heights,
            contract_version: "relay.v1".into(),
            session_id: s.sid.clone(),
            entry_path: format!("/stream/{}/entry", s.sid),
            format,
            expires_at: DateTime::from_timestamp(s.expires, 0).ok_or(STORE)?,
            revision: s.revision,
            auth_revision: s.auth_revision,
            source_generation: s.generation,
            state: if s.revoked {
                SessionState::Revoked
            } else if s.expires <= Utc::now().timestamp() {
                SessionState::Expired
            } else {
                SessionState::Active
            },
            resume_required: resume.then_some(true),
        })
    }
    pub(super) fn create(
        &mut self,
        request: CreateSession,
        key: String,
    ) -> Result<SessionReady, Error> {
        if key.is_empty() || key.len() > 128 || !key.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(INVALID);
        }
        let normalized = serde_json::to_value(&request).map_err(|_| INVALID)?;
        let fingerprint =
            Sha256::digest(serde_json::to_vec(&normalized).map_err(|_| INVALID)?).to_vec();
        let id = B64.encode(Sha256::digest(
            serde_json::to_vec(&(&request.owner_id, &key)).map_err(|_| INVALID)?,
        ));
        let existing: Option<Vec<u8>> = self
            .db
            .query_row("SELECT blob FROM idempotency WHERE id=?1", [&id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|_| STORE)?;
        if let Some(blob) = existing {
            let item: Idempotency = self.crypto.open(&format!("idempotency:{id}"), &blob)?;
            if item.expires > Utc::now().timestamp() {
                if item.fingerprint != fingerprint {
                    return Err(Error(StatusCode::CONFLICT, "idempotency_conflict"));
                }
                return Ok(item.ready);
            }
        }
        validation::create(&request, &self.config, Utc::now())?;
        if request.expires_at.timestamp() <= Utc::now().timestamp() {
            return Err(INVALID);
        }
        self.writable()?;
        if self.count("sessions")? >= self.config.state.max_sessions_count
            || self.count("idempotency")? >= self.config.state.max_idempotency_count
        {
            return Err(LIMIT);
        }
        let s = Session {
            sid: random_id()?,
            owner: request.owner_id,
            expires: request.expires_at.timestamp(),
            revision: 1,
            auth_revision: 1,
            generation: 1,
            revoked: false,
            secret: random_id()?,
            created: Utc::now().timestamp(),
            title: Self::identity(&request.selection)?,
            format: request
                .selection
                .selected
                .as_ref()
                .ok_or(INVALID)?
                .candidate
                .observation
                .format
                .clone(),
        };
        let root = Self::root(&request.selection)?;
        let available_heights = quality_heights(&root, &request.selection);
        let rid = self.resource_id(&s, 1, &root)?;
        self.check_capacity(&s.sid, 1)?;
        self.check_generation_capacity(&s.sid)?;
        let g = Generation {
            selection: request.selection,
            root: rid.clone(),
            valid_until: None,
        };
        let session_blob = self.crypto.seal(&format!("session:{}", s.sid), &s)?;
        let generation_blob = self.crypto.seal(&format!("generation:{}:1", s.sid), &g)?;
        let resource_blob = self
            .crypto
            .seal(&format!("resource:{}:1:{rid}", s.sid), &root)?;
        let tx = self.db.transaction().map_err(|_| STORE)?;
        tx.execute(
            "INSERT INTO sessions VALUES(?1,?2,?3)",
            params![s.sid, s.expires, session_blob],
        )
        .map_err(|_| STORE)?;
        tx.execute(
            "INSERT INTO generations VALUES(?1,1,?2)",
            params![s.sid, generation_blob],
        )
        .map_err(|_| STORE)?;
        tx.execute(
            "INSERT INTO resources VALUES(?1,1,?2,?3)",
            params![s.sid, rid, resource_blob],
        )
        .map_err(|_| STORE)?;
        // Build redacted reply from the transaction's own snapshot.
        let ready = SessionReady {
            available_heights,
            contract_version: "relay.v1".into(),
            session_id: s.sid.clone(),
            entry_path: format!("/stream/{}/entry", s.sid),
            format: if root.kind == ResourceKind::Manifest {
                PlaybackFormat::Hls
            } else {
                PlaybackFormat::Mp4
            },
            expires_at: DateTime::from_timestamp(s.expires, 0).ok_or(STORE)?,
            revision: 1,
            auth_revision: 1,
            source_generation: 1,
            state: SessionState::Active,
            resume_required: None,
        };
        let blob = self.crypto.seal(
            &format!("idempotency:{id}"),
            &Idempotency {
                fingerprint,
                ready,
                expires: s.expires,
            },
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO idempotency VALUES(?1,?2,?3)",
            params![id, s.expires, blob],
        )
        .map_err(|_| STORE)?;
        tx.commit().map_err(|_| STORE)?;
        self.ready(&s, false)
    }
    pub(super) fn identity(
        selection: &Selection,
    ) -> Result<crate::contracts::providers::Title, Error> {
        let mut title = selection
            .selected
            .as_ref()
            .ok_or(INVALID)?
            .candidate
            .source
            .title
            .clone();
        for backup in selection.backups.iter().flatten() {
            let incoming = &backup.candidate.source.title;
            if !validation::same_title(&title, incoming) {
                return Err(INVALID);
            }
            if title.imdb_id.is_none() {
                title.imdb_id = incoming.imdb_id.clone();
            }
            if title.my_anime_list_id.is_none() {
                title.my_anime_list_id = incoming.my_anime_list_id;
            }
        }
        Ok(title)
    }
    pub(super) fn mutation(&self, sid: &str, owner: &str, revision: u64) -> Result<Session, Error> {
        let s = self.session(sid)?;
        if s.owner != owner || !validation::owner(owner) {
            return Err(Error(StatusCode::FORBIDDEN, "owner_mismatch"));
        }
        self.active(&s)?;
        if s.revision != revision {
            return Err(CONFLICT);
        }
        Ok(s)
    }
    pub(super) fn backups(&mut self, sid: String, request: UpdateBackups) -> Result<(), Error> {
        let session = self.session(&sid)?;
        if session.owner != request.owner_id || !validation::owner(&request.owner_id) {
            return Err(Error(StatusCode::FORBIDDEN, "owner_mismatch"));
        }
        self.active(&session)?;
        if session.generation != request.expected_generation {
            return Err(CONFLICT);
        }
        validation::selection(&request.selection, &self.config, Utc::now())?;
        let incoming_title = Self::identity(&request.selection)?;
        if !validation::same_title(&session.title, &incoming_title) {
            return Err(INVALID);
        }
        let mut generation = self.generation(&sid, session.generation)?;
        let selected_id = generation
            .selection
            .selected
            .as_ref()
            .ok_or(STORE)?
            .candidate
            .source
            .id
            .clone();
        let mut seen = std::collections::BTreeSet::from([selected_id]);
        let mut backups = Vec::new();
        // Preserve the complete round's ranking, then retain any previously
        // imported fallback absent from it. Never replace the playing source.
        for ranked in request
            .selection
            .selected
            .into_iter()
            .chain(request.selection.backups.into_iter().flatten())
            .chain(generation.selection.backups.take().into_iter().flatten())
        {
            if seen.insert(ranked.candidate.source.id.clone()) {
                backups.push(ranked);
            }
            if backups.len() + 1 >= self.config.state.max_sources_per_session_count as usize {
                break;
            }
        }
        generation.selection.backups = Some(backups);
        validation::selection(&generation.selection, &self.config, Utc::now())?;
        let blob = self.crypto.seal(
            &format!("generation:{sid}:{}", session.generation),
            &generation,
        )?;
        if blob.len() > self.config.state.max_snapshot_bytes as usize {
            return Err(Error(StatusCode::PAYLOAD_TOO_LARGE, "import_too_large"));
        }
        self.writable()?;
        self.db
            .execute(
                "UPDATE generations SET blob=?3 WHERE sid=?1 AND generation=?2",
                params![sid, sql_u64(session.generation)?, blob],
            )
            .map_err(|_| STORE)?;
        Ok(())
    }

    pub(super) fn renew(
        &mut self,
        sid: String,
        request: RenewSession,
    ) -> Result<SessionReady, Error> {
        let mut s = self.mutation(&sid, &request.owner_id, request.expected_revision)?;
        let now = Utc::now().timestamp();
        let expiry = request.expires_at.timestamp();
        if expiry <= s.expires || expiry - now > self.config.state.max_session_lifetime_s as i64 {
            return Err(INVALID);
        }
        s.expires = expiry;
        s.revision = s.revision.checked_add(1).ok_or(STORE)?;
        self.writable()?;
        let blob = self.crypto.seal(&format!("session:{sid}"), &s)?;
        self.db
            .execute(
                "UPDATE sessions SET expires=?2,blob=?3 WHERE sid=?1",
                params![sid, s.expires, blob],
            )
            .map_err(|_| STORE)?;
        self.ready(&s, false)
    }
    pub(super) fn validate_replacement(
        &self,
        s: &Session,
        selection: &Selection,
    ) -> Result<crate::contracts::providers::Title, Error> {
        validation::selection(selection, &self.config, Utc::now())?;
        // Compare every known cross-ID across both snapshots, including backups.
        let titles = |selection: &Selection| {
            std::iter::once(selection.selected.as_ref())
                .flatten()
                .chain(selection.backups.iter().flatten())
                .map(|r| r.candidate.source.title.clone())
                .collect::<Vec<_>>()
        };
        let mut identity = s.title.clone();
        for incoming in titles(selection) {
            if !validation::same_title(&identity, &incoming) {
                return Err(INVALID);
            }
            if identity.imdb_id.is_none() {
                identity.imdb_id = incoming.imdb_id;
            }
            if identity.my_anime_list_id.is_none() {
                identity.my_anime_list_id = incoming.my_anime_list_id;
            }
        }
        Ok(identity)
    }

    pub(super) fn source(
        &mut self,
        sid: String,
        request: UpdateSource,
    ) -> Result<SessionReady, Error> {
        self.source_prepared(sid, request, vec![], None)
    }
    pub(super) fn source_prepared(
        &mut self,
        sid: String,
        request: UpdateSource,
        batch: Vec<Registration>,
        preparation: Option<(CancellationToken, CancellationToken, tokio::time::Instant)>,
    ) -> Result<SessionReady, Error> {
        let mut s = self.mutation(&sid, &request.owner_id, request.expected_revision)?;
        s.title = self.validate_replacement(&s, &request.selection)?;
        if serde_json::to_vec(&request.selection)
            .map_err(|_| INVALID)?
            .len()
            > self.config.state.max_snapshot_bytes as usize
        {
            return Err(INVALID);
        }
        let mut old = self.generation(&sid, s.generation)?;
        old.valid_until = Some(
            (Utc::now().timestamp() + self.config.state.old_generation_grace_s as i64)
                .min(s.expires),
        );
        let old_blob = self
            .crypto
            .seal(&format!("generation:{sid}:{}", s.generation), &old)?;
        let previous = s.generation;
        s.generation = s.generation.checked_add(1).ok_or(STORE)?;
        s.revision = s.revision.checked_add(1).ok_or(STORE)?;
        let root = Self::root(&request.selection)?;
        let rid = self.resource_id(&s, s.generation, &root)?;
        if batch.len() as u64 > self.config.limits.max_manifest_registrations_count {
            return Err(LIMIT);
        }
        let mut records = BTreeMap::new();
        for r in batch {
            self.validate_resource(&r)?;
            let id = self.resource_id(&s, s.generation, &r)?;
            if id != rid {
                records.insert(
                    id.clone(),
                    self.crypto
                        .seal(&format!("resource:{sid}:{}:{id}", s.generation), &r)?,
                );
            }
        }
        self.check_capacity(&sid, records.len() + 1)?;
        self.check_generation_capacity(&sid)?;
        self.writable()?;
        s.format = request
            .selection
            .selected
            .as_ref()
            .ok_or(INVALID)?
            .candidate
            .observation
            .format
            .clone();
        let g = Generation {
            selection: request.selection,
            root: rid.clone(),
            valid_until: None,
        };
        let gen_blob = self
            .crypto
            .seal(&format!("generation:{sid}:{}", s.generation), &g)?;
        let session_blob = self.crypto.seal(&format!("session:{sid}"), &s)?;
        let resource_blob = self
            .crypto
            .seal(&format!("resource:{sid}:{}:{rid}", s.generation), &root)?;
        let tx = self.db.transaction().map_err(|_| STORE)?;
        tx.execute(
            "UPDATE generations SET blob=?3 WHERE sid=?1 AND generation=?2",
            params![sid, sql_u64(previous)?, old_blob],
        )
        .map_err(|_| STORE)?;
        tx.execute(
            "INSERT INTO generations VALUES(?1,?2,?3)",
            params![sid, sql_u64(s.generation)?, gen_blob],
        )
        .map_err(|_| STORE)?;
        tx.execute(
            "INSERT INTO resources VALUES(?1,?2,?3,?4)",
            params![sid, sql_u64(s.generation)?, rid, resource_blob],
        )
        .map_err(|_| STORE)?;
        tx.execute(
            "UPDATE sessions SET blob=?2 WHERE sid=?1",
            params![sid, session_blob],
        )
        .map_err(|_| STORE)?;
        for (id, blob) in records {
            tx.execute(
                "INSERT INTO resources VALUES(?1,?2,?3,?4)",
                params![sid, sql_u64(s.generation)?, id, blob],
            )
            .map_err(|_| STORE)?;
        }
        if let Some((cancel, epoch, deadline)) = preparation {
            if cancel.is_cancelled() {
                return Err(Error(StatusCode::REQUEST_TIMEOUT, "preparation_cancelled"));
            }
            if epoch.is_cancelled() {
                return Err(Error(StatusCode::SERVICE_UNAVAILABLE, "vpn_unavailable"));
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error(StatusCode::GATEWAY_TIMEOUT, "upstream_timeout"));
            }
        }
        tx.commit().map_err(|_| STORE)?;
        crate::health::event(&crate::health::GENERATIONS);
        self.ready(&s, true)
    }
    pub(super) fn revoke(&mut self, sid: String) -> Result<(), Error> {
        let mut s = match self.session(&sid) {
            Ok(s) => s,
            Err(e) if e.0 == StatusCode::NOT_FOUND => return Ok(()),
            Err(e) => return Err(e),
        };
        if s.revoked {
            return Ok(());
        }
        self.writable()?;
        s.revoked = true;
        s.auth_revision = s.auth_revision.checked_add(1).ok_or(STORE)?;
        s.revision = s.revision.checked_add(1).ok_or(STORE)?;
        let blob = self.crypto.seal(&format!("session:{sid}"), &s)?;
        self.db
            .execute(
                "UPDATE sessions SET blob=?2 WHERE sid=?1",
                params![sid, blob],
            )
            .map_err(|_| STORE)?;
        for ((id, _), lease) in &self.leases {
            if *id == sid {
                lease.cancelled.cancel();
            }
        }
        Ok(())
    }
    pub(super) fn expire_readers(&mut self) {
        for ((sid, n), lease) in &self.leases {
            let valid = self.session(sid).and_then(|s| {
                self.active(&s)?;
                if *n == 0 {
                    return Ok(());
                }
                let g = self.generation(sid, *n)?;
                self.valid_generation(&g)
            });
            if valid.is_err() {
                lease.cancelled.cancel();
            }
        }
        // Tokens with no readers can be recreated on demand. No per-request DB write.
        self.leases.retain(|_, v| Arc::strong_count(&v.readers) > 1);
    }
    pub(super) fn cleanup(&mut self) -> Result<(), Error> {
        if self.failed {
            return Err(STORE);
        }
        self.expire_readers();
        self.writable()?;
        let now = Utc::now().timestamp();
        let limit = sql_u64(self.config.state.cleanup_batch_count)?;
        // Page the generation scan too: repeated sweeps eventually reach every
        // generation, without scanning all resources or deleting a large cascade.
        let mut stmt=self.db.prepare("SELECT sid,generation FROM generations WHERE (sid,generation)>(?1,?2) ORDER BY sid,generation LIMIT ?3").map_err(|_|STORE)?;
        let ids = stmt
            .query_map(
                params![self.cleanup_cursor.0, self.cleanup_cursor.1, limit],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .map_err(|_| STORE)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| STORE)?;
        drop(stmt);
        if let Some(last) = ids.last() {
            self.cleanup_cursor = last.clone();
        } else {
            self.cleanup_cursor = (String::new(), 0);
        }
        let mut budget = limit;
        for (sid, n) in ids {
            let n: u64 = n.try_into().map_err(|_| STORE)?;
            let s = self.session(&sid)?;
            let g = self.generation(&sid, n)?;
            if s.expires > now && !g.valid_until.is_some_and(|v| v <= now) {
                continue;
            }
            if self
                .leases
                .get(&(sid.clone(), n))
                .is_some_and(|v| Arc::strong_count(&v.readers) > 1)
            {
                continue;
            }
            let deleted=self.db.execute("DELETE FROM resources WHERE (sid,rid) IN (SELECT sid,rid FROM resources WHERE sid=?1 AND generation=?2 LIMIT ?3)",params![sid,sql_u64(n)?,budget]).map_err(|_|STORE)?;
            budget -= deleted as i64;
            let left: bool = self
                .db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM resources WHERE sid=?1 AND generation=?2)",
                    params![sid, sql_u64(n)?],
                    |r| r.get(0),
                )
                .map_err(|_| STORE)?;
            if !left && budget > 0 {
                self.db
                    .execute(
                        "DELETE FROM generations WHERE sid=?1 AND generation=?2",
                        params![sid, sql_u64(n)?],
                    )
                    .map_err(|_| STORE)?;
                budget -= 1;
            }
            if budget == 0 {
                break;
            }
        }
        self.db.execute("DELETE FROM sessions WHERE sid IN (SELECT sid FROM sessions WHERE expires<=?1 AND NOT EXISTS(SELECT 1 FROM generations WHERE generations.sid=sessions.sid) ORDER BY expires LIMIT ?2)",params![now,limit]).map_err(|_|STORE)?;
        self.db.execute("DELETE FROM idempotency WHERE id IN (SELECT id FROM idempotency WHERE expires<=?1 ORDER BY expires LIMIT ?2)",params![now,limit]).map_err(|_|STORE)?;
        self.clear_cache();
        Ok(())
    }
}

fn quality_heights(root: &Registration, selection: &Selection) -> Option<Vec<i64>> {
    let manifest = root.manifest.as_ref()?;
    let mut heights: Vec<i64> = if manifest.adaptive_variants.is_empty() {
        selection
            .selected
            .as_ref()?
            .candidate
            .observation
            .variant
            .as_ref()?
            .height
            .into_iter()
            .collect()
    } else {
        manifest
            .adaptive_variants
            .iter()
            .filter_map(|variant| variant.height)
            .collect()
    };
    heights.sort_unstable();
    heights.dedup();
    (!heights.is_empty()).then_some(heights)
}
