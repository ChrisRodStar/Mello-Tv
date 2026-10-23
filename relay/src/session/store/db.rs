use super::*;

impl Engine {
    pub(super) fn writable(&self) -> Result<(), Error> {
        let parent = Path::new(&self.config.state.database_path)
            .parent()
            .ok_or(STORE)?;
        if fs2::available_space(parent).map_err(|_| STORE)?
            < self.config.state.min_free_space_bytes
                + self.config.state.max_snapshot_bytes * 16
                + 65536
        {
            return Err(STORE);
        }
        Ok(())
    }
    pub(super) fn migrate(&mut self) -> Result<(), Error> {
        let version: i64 = self
            .db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|_| STORE)?;
        if version == 0 {
            let tables: i64 = self
                .db
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                    [],
                    |r| r.get(0),
                )
                .map_err(|_| STORE)?;
            if tables != 0 {
                return Err(STORE);
            }
            self.writable()?;
            let sentinel = self.crypto.seal("metadata", &"relay-store-v1")?;
            let tx = self.db.transaction().map_err(|_| STORE)?;
            tx.execute_batch(include_str!("schema-v1.sql"))
                .map_err(|_| STORE)?;
            tx.execute("INSERT INTO metadata VALUES(1,?1)", [sentinel])
                .map_err(|_| STORE)?;
            tx.commit().map_err(|_| STORE)?;
        } else if version != 1 {
            return Err(STORE);
        }
        Ok(())
    }
    pub(super) fn restore(&mut self) -> Result<(), Error> {
        let check: String = self
            .db
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(|_| STORE)?;
        if check != "ok" {
            return Err(STORE);
        }
        let foreign = self
            .db
            .prepare("PRAGMA foreign_key_check")
            .map_err(|_| STORE)?
            .exists([])
            .map_err(|_| STORE)?;
        if foreign {
            return Err(STORE);
        }
        let blob: Vec<u8> = self
            .db
            .query_row("SELECT blob FROM metadata WHERE id=1", [], |r| r.get(0))
            .map_err(|_| STORE)?;
        let sentinel: String = self.crypto.open("metadata", &blob)?;
        if sentinel != "relay-store-v1" {
            return Err(STORE);
        }
        for (table, limit) in [
            ("sessions", self.config.state.max_sessions_count),
            ("resources", self.config.state.max_resources_total_count),
            ("idempotency", self.config.state.max_idempotency_count),
            ("generations", self.config.state.max_resources_total_count),
        ] {
            if self.count(table)? > limit {
                return Err(STORE);
            }
        }
        // Stream validation row by row; never load the complete database into RAM.
        let mut stmt = self
            .db
            .prepare("SELECT sid,expires,blob FROM sessions")
            .map_err(|_| STORE)?;
        let mut rows = stmt.query([]).map_err(|_| STORE)?;
        while let Some(row) = rows.next().map_err(|_| STORE)? {
            let sid: String = row.get(0).map_err(|_| STORE)?;
            let exp: i64 = row.get(1).map_err(|_| STORE)?;
            let s: Session = self.crypto.open(
                &format!("session:{sid}"),
                &row.get::<_, Vec<u8>>(2).map_err(|_| STORE)?,
            )?;
            if s.sid != sid
                || s.expires != exp
                || !crate::auth::opaque(&sid)
                || !validation::owner(&s.owner)
                || s.revision == 0
                || s.auth_revision == 0
                || s.generation == 0
                || s.expires <= s.created
                || !matches!(s.format.as_str(), "hls" | "mp4")
                || B64.decode(&s.secret).map_err(|_| STORE)?.len() != 32
            {
                return Err(STORE);
            }
            if s.expires > Utc::now().timestamp() {
                let _ = self.generation(&sid, s.generation)?;
            }
            let resources: i64 = self
                .db
                .query_row("SELECT count(*) FROM resources WHERE sid=?1", [&sid], |r| {
                    r.get(0)
                })
                .map_err(|_| STORE)?;
            let generations: i64 = self
                .db
                .query_row(
                    "SELECT count(*) FROM generations WHERE sid=?1",
                    [&sid],
                    |r| r.get(0),
                )
                .map_err(|_| STORE)?;
            if resources as u64 > self.config.state.max_resources_per_session_count
                || generations as u64 > self.config.state.max_resources_per_session_count
            {
                return Err(STORE);
            }
        }
        drop(rows);
        drop(stmt);
        let mut stmt = self
            .db
            .prepare("SELECT sid,generation,blob FROM generations")
            .map_err(|_| STORE)?;
        let mut rows = stmt.query([]).map_err(|_| STORE)?;
        while let Some(row) = rows.next().map_err(|_| STORE)? {
            let sid: String = row.get(0).map_err(|_| STORE)?;
            let n = read_u64(row, 1)?;
            let g: Generation = self.crypto.open(
                &format!("generation:{sid}:{n}"),
                &row.get::<_, Vec<u8>>(2).map_err(|_| STORE)?,
            )?;
            let s = self.session(&sid)?;
            // Revalidate static invariants with a time before the source's expiry.
            let selected = g.selection.selected.as_ref().ok_or(STORE)?;
            if !validation::same_title(&s.title, &Self::identity(&g.selection).map_err(|_| STORE)?)
                || (n == s.generation && s.format != selected.candidate.observation.format)
            {
                return Err(STORE);
            }
            let at = selected
                .candidate
                .source
                .expires_at
                .map(|v| v.timestamp() - 1)
                .unwrap_or(s.created);
            validation::selection(
                &g.selection,
                &self.config,
                DateTime::from_timestamp(at, 0).ok_or(STORE)?,
            )
            .map_err(|_| STORE)?;
            if n == 0
                || n > s.generation
                || (n == s.generation && g.valid_until.is_some())
                || (n != s.generation && g.valid_until.is_none())
            {
                return Err(STORE);
            }
            match self.resource_uncached(&s, n, &g.root) {
                Ok(root) if root.uri == selected.candidate.source.url => {}
                Err(e)
                    if e.0 == StatusCode::NOT_FOUND
                        && (s.expires <= Utc::now().timestamp()
                            || g.valid_until.is_some_and(|v| v <= Utc::now().timestamp())) => {}
                _ => return Err(STORE),
            }
        }
        drop(rows);
        drop(stmt);
        let mut stmt = self
            .db
            .prepare("SELECT sid,generation,rid,blob FROM resources")
            .map_err(|_| STORE)?;
        let mut rows = stmt.query([]).map_err(|_| STORE)?;
        while let Some(row) = rows.next().map_err(|_| STORE)? {
            let sid: String = row.get(0).map_err(|_| STORE)?;
            let n = read_u64(row, 1)?;
            let rid: String = row.get(2).map_err(|_| STORE)?;
            let r: Registration = self.crypto.open(
                &format!("resource:{sid}:{n}:{rid}"),
                &row.get::<_, Vec<u8>>(3).map_err(|_| STORE)?,
            )?;
            let s = self.session(&sid)?;
            self.validate_resource(&r)?;
            if self.resource_id(&s, n, &r)? != rid {
                return Err(STORE);
            }
        }
        drop(rows);
        drop(stmt);
        let mut stmt = self
            .db
            .prepare("SELECT id,expires,blob FROM idempotency")
            .map_err(|_| STORE)?;
        let mut rows = stmt.query([]).map_err(|_| STORE)?;
        while let Some(row) = rows.next().map_err(|_| STORE)? {
            let id: String = row.get(0).map_err(|_| STORE)?;
            let exp: i64 = row.get(1).map_err(|_| STORE)?;
            let item: Idempotency = self.crypto.open(
                &format!("idempotency:{id}"),
                &row.get::<_, Vec<u8>>(2).map_err(|_| STORE)?,
            )?;
            if item.expires != exp || item.fingerprint.len() != 32 {
                return Err(STORE);
            }
        }
        Ok(())
    }
    pub(super) fn count(&self, table: &str) -> Result<u64, Error> {
        self.db
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| {
                read_u64(r, 0).map_err(|_| rusqlite::Error::InvalidQuery)
            })
            .map_err(|_| STORE)
    }
    pub(super) fn session(&self, sid: &str) -> Result<Session, Error> {
        let blob: Vec<u8> = self
            .db
            .query_row("SELECT blob FROM sessions WHERE sid=?1", [sid], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|_| STORE)?
            .ok_or(UNKNOWN)?;
        let s: Session = self.crypto.open(&format!("session:{sid}"), &blob)?;
        if s.sid != sid {
            return Err(STORE);
        }
        Ok(s)
    }
    pub(super) fn generation(&self, sid: &str, n: u64) -> Result<Generation, Error> {
        let blob: Vec<u8> = self
            .db
            .query_row(
                "SELECT blob FROM generations WHERE sid=?1 AND generation=?2",
                params![sid, sql_u64(n)?],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| STORE)?
            .ok_or(UNKNOWN)?;
        self.crypto.open(&format!("generation:{sid}:{n}"), &blob)
    }
}
