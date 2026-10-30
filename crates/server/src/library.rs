//! The Jellyfin library: a JSON list of wanted titles plus a sync job that
//! mirrors it into `.strm` files laid out the way Jellyfin expects:
//!
//! ```text
//! movies/Inception (2010) [tmdbid-27205]/Inception (2010) [tmdbid-27205].strm
//! shows/Severance (2022) [tmdbid-95396]/Season 01/Severance S01E01.strm
//! ```
//!
//! Jellyfin reads the `[tmdbid-…]` folder tag for metadata. Only folders this
//! server created (tracked in `sync-state.json`) are ever modified or removed.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use url::Url;

use crate::{
    auth::write_private,
    config::{AutoList, Config},
    tmdb::{self, Tmdb, TmdbError},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Movie,
    Tv,
}

impl Kind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "movie" | "movies" => Some(Self::Movie),
            "tv" | "show" | "shows" => Some(Self::Tv),
            _ => None,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Tv => "tv",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    #[serde(default)]
    pub title: String,
    pub added_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LibraryFile {
    #[serde(default)]
    pub movies: BTreeMap<i64, Entry>,
    #[serde(default)]
    pub shows: BTreeMap<i64, Entry>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct SyncState {
    /// Item key (`movie:27205`) -> folder relative to the library root.
    #[serde(default)]
    items: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SyncReport {
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub movies: usize,
    pub shows: usize,
    pub episodes: usize,
    pub files_written: usize,
    pub files_removed: usize,
    pub folders_removed: usize,
    pub failures: Vec<String>,
    pub jellyfin_refreshed: bool,
}

struct PlannedItem {
    dir: String,
    files: BTreeMap<String, String>,
}

pub struct Library {
    file_path: PathBuf,
    state_path: PathBuf,
    root: PathBuf,
    entries: Mutex<LibraryFile>,
    sync_lock: tokio::sync::Mutex<()>,
    wake: Notify,
    last_report: Mutex<SyncReport>,
    tmdb: Tmdb,
    settings: SyncSettings,
    http: reqwest::Client,
}

#[derive(Clone)]
struct SyncSettings {
    public_url: Url,
    play_key: String,
    include_specials: bool,
    auto_lists: Vec<AutoList>,
    tmdb_lists: Vec<u64>,
    interval: Duration,
    jellyfin_url: Option<Url>,
    jellyfin_api_key: Option<String>,
}

impl Library {
    pub async fn open(config: &Config, tmdb: Tmdb, play_key: &str) -> std::io::Result<Arc<Self>> {
        let file_path = config.data_dir.join("library.json");
        let state_path = config.data_dir.join("sync-state.json");
        let entries = match tokio::fs::read(&file_path).await {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
                std::io::Error::other(format!("{} is not valid: {e}", file_path.display()))
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => LibraryFile::default(),
            Err(e) => return Err(e),
        };
        tokio::fs::create_dir_all(&config.library_dir).await?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(std::io::Error::other)?;
        Ok(Arc::new(Self {
            file_path,
            state_path,
            root: config.library_dir.clone(),
            entries: Mutex::new(entries),
            sync_lock: tokio::sync::Mutex::new(()),
            wake: Notify::new(),
            last_report: Mutex::default(),
            tmdb,
            settings: SyncSettings {
                public_url: config.public_url.clone(),
                play_key: play_key.to_owned(),
                include_specials: config.include_specials,
                auto_lists: config.auto_lists.clone(),
                tmdb_lists: config.tmdb_lists.clone(),
                interval: config.sync_interval,
                jellyfin_url: config.jellyfin_url.clone(),
                jellyfin_api_key: config.jellyfin_api_key.clone(),
            },
            http,
        }))
    }

    pub fn entries(&self) -> LibraryFile {
        self.entries
            .lock()
            .map(|entries| entries.clone())
            .unwrap_or_default()
    }

    pub fn last_report(&self) -> SyncReport {
        self.last_report
            .lock()
            .map(|report| report.clone())
            .unwrap_or_default()
    }

    async fn save(&self) -> std::io::Result<()> {
        let bytes = {
            let entries = self
                .entries
                .lock()
                .map_err(|_| std::io::Error::other("library lock poisoned"))?;
            serde_json::to_vec_pretty(&*entries).map_err(std::io::Error::other)?
        };
        write_private(&self.file_path, &bytes).await
    }

    /// Adds a title (validated against TMDB) and schedules a sync.
    pub async fn add(&self, kind: Kind, id: i64) -> Result<Entry, TmdbError> {
        let title = match kind {
            Kind::Movie => self.tmdb.movie(id).await?.title,
            Kind::Tv => self.tmdb.show(id, false).await?.name,
        };
        let entry = Entry {
            title,
            added_at: Utc::now(),
        };
        if let Ok(mut entries) = self.entries.lock() {
            let map = match kind {
                Kind::Movie => &mut entries.movies,
                Kind::Tv => &mut entries.shows,
            };
            map.entry(id).or_insert_with(|| entry.clone());
        }
        self.save()
            .await
            .map_err(|e| TmdbError::Request(e.to_string()))?;
        self.wake.notify_one();
        Ok(entry)
    }

    pub async fn remove(&self, kind: Kind, id: i64) -> std::io::Result<bool> {
        let removed = match self.entries.lock() {
            Ok(mut entries) => match kind {
                Kind::Movie => entries.movies.remove(&id).is_some(),
                Kind::Tv => entries.shows.remove(&id).is_some(),
            },
            Err(_) => false,
        };
        if removed {
            self.save().await?;
            self.wake.notify_one();
        }
        Ok(removed)
    }

    /// Runs forever: once at start, then on every change and on the interval.
    pub async fn run_sync_loop(self: Arc<Self>, shutdown: tokio_util::sync::CancellationToken) {
        loop {
            // Short debounce so a burst of API changes becomes one sync.
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_secs(3)) => {}
            }
            let report = self.sync().await;
            tracing::info!(
                movies = report.movies,
                shows = report.shows,
                episodes = report.episodes,
                written = report.files_written,
                removed = report.files_removed + report.folders_removed,
                failures = report.failures.len(),
                "library sync finished"
            );
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = self.wake.notified() => {}
                _ = tokio::time::sleep(self.settings.interval) => {}
            }
        }
    }

    pub async fn sync(&self) -> SyncReport {
        let _guard = self.sync_lock.lock().await;
        let mut report = SyncReport {
            started_at: Some(Utc::now()),
            ..SyncReport::default()
        };
        if !self.tmdb.configured() {
            report
                .failures
                .push("TMDB_API_KEY or TMDB_READ_ACCESS_TOKEN is not set".into());
            report.finished_at = Some(Utc::now());
            self.store_report(&report);
            return report;
        }

        let wanted = self.wanted(&mut report).await;
        let mut plan: BTreeMap<String, PlannedItem> = BTreeMap::new();
        let mut failed: BTreeSet<String> = BTreeSet::new();
        let today = Utc::now().date_naive();
        for (kind, id) in wanted {
            let key = format!("{}:{id}", kind.as_str());
            let planned = match kind {
                Kind::Movie => self.plan_movie(id).await,
                Kind::Tv => self.plan_show(id, today).await,
            };
            match planned {
                Ok(item) => {
                    match kind {
                        Kind::Movie => report.movies += 1,
                        Kind::Tv => {
                            report.shows += 1;
                            report.episodes += item.files.len();
                        }
                    }
                    plan.insert(key, item);
                }
                Err(error) => {
                    report.failures.push(format!("{key}: {error}"));
                    failed.insert(key);
                }
            }
        }

        let previous = self.load_state().await;
        let mut next = SyncState::default();
        for (key, item) in &plan {
            if let Err(error) = self.write_item(item, &mut report).await {
                report.failures.push(format!("{key}: {error}"));
            }
            if let Some(old_dir) = previous.items.get(key)
                && old_dir != &item.dir
                && self.remove_managed_dir(old_dir).await
            {
                report.folders_removed += 1;
            }
            next.items.insert(key.clone(), item.dir.clone());
        }
        for (key, dir) in &previous.items {
            if plan.contains_key(key) {
                continue;
            }
            if failed.contains(key) {
                // Keep what Jellyfin already has while TMDB is unavailable.
                next.items.insert(key.clone(), dir.clone());
            } else if self.remove_managed_dir(dir).await {
                report.folders_removed += 1;
            }
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(&next)
            && let Err(error) = write_private(&self.state_path, &bytes).await
        {
            report.failures.push(format!("sync state: {error}"));
        }

        let changed = report.files_written + report.files_removed + report.folders_removed > 0;
        if changed {
            report.jellyfin_refreshed = self.refresh_jellyfin().await;
        }
        report.finished_at = Some(Utc::now());
        self.store_report(&report);
        report
    }

    fn store_report(&self, report: &SyncReport) {
        if let Ok(mut last) = self.last_report.lock() {
            *last = report.clone();
        }
    }

    /// Manual entries plus configured TMDB collections and lists, de-duplicated.
    async fn wanted(&self, report: &mut SyncReport) -> BTreeSet<(Kind, i64)> {
        let mut wanted = BTreeSet::new();
        let entries = self.entries();
        wanted.extend(entries.movies.keys().map(|id| (Kind::Movie, *id)));
        wanted.extend(entries.shows.keys().map(|id| (Kind::Tv, *id)));
        for list in &self.settings.auto_lists {
            let default_type = if list.kind.is_tv() { "tv" } else { "movie" };
            match self
                .tmdb
                .collection(list.kind.path(), Some(default_type), list.limit)
                .await
            {
                Ok(items) => extend_items(&mut wanted, items),
                Err(error) => report
                    .failures
                    .push(format!("list {}: {error}", list.kind.path())),
            }
        }
        for id in &self.settings.tmdb_lists {
            match self
                .tmdb
                .collection(&format!("/list/{id}"), None, 1000)
                .await
            {
                Ok(items) => extend_items(&mut wanted, items),
                Err(error) => report.failures.push(format!("TMDB list {id}: {error}")),
            }
        }
        wanted
    }

    fn play_url(&self, path: &str, imdb: Option<&str>) -> String {
        let mut url = self.settings.public_url.clone();
        let base_path = url.path().trim_end_matches('/').to_owned();
        url.set_path(&format!("{base_path}{path}"));
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("key", &self.settings.play_key);
            if let Some(imdb) = imdb.filter(|id| valid_imdb(id)) {
                query.append_pair("imdb", imdb);
            }
        }
        url.to_string()
    }

    async fn plan_movie(&self, id: i64) -> Result<PlannedItem, TmdbError> {
        let movie = self.tmdb.movie(id).await?;
        let name = folder_name(&movie.title, tmdb::year(movie.release_date.as_deref()), id);
        let dir = format!("movies/{name}");
        let url = self.play_url(&format!("/play/movie/{id}"), movie.imdb_id.as_deref());
        let mut files = BTreeMap::new();
        files.insert(format!("{dir}/{name}.strm"), url);
        Ok(PlannedItem { dir, files })
    }

    async fn plan_show(&self, id: i64, today: NaiveDate) -> Result<PlannedItem, TmdbError> {
        let show = self.tmdb.show(id, true).await?;
        let title = sanitize(&show.name);
        let name = folder_name(&show.name, tmdb::year(show.first_air_date.as_deref()), id);
        let dir = format!("shows/{name}");
        let imdb = show.external_ids.imdb_id.as_deref();
        let mut files = BTreeMap::new();
        for season in &show.seasons {
            let number = season.season_number;
            if number < 0 || (number == 0 && !self.settings.include_specials) {
                continue;
            }
            if season.episode_count == 0 {
                continue;
            }
            let details = self.tmdb.season(id, number, true).await?;
            for episode in details.episodes {
                let aired = episode
                    .air_date
                    .as_deref()
                    .and_then(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
                    .is_some_and(|date| date <= today);
                if !aired || episode.episode_number <= 0 {
                    continue;
                }
                let e = episode.episode_number;
                files.insert(
                    format!("{dir}/Season {number:02}/{title} S{number:02}E{e:02}.strm"),
                    self.play_url(&format!("/play/tv/{id}/{number}/{e}"), imdb),
                );
            }
        }
        Ok(PlannedItem { dir, files })
    }

    async fn write_item(&self, item: &PlannedItem, report: &mut SyncReport) -> std::io::Result<()> {
        for (relative, contents) in &item.files {
            let path = self.root.join(relative);
            let current = tokio::fs::read_to_string(&path).await.ok();
            if current.as_deref() == Some(contents.as_str()) {
                continue;
            }
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&path, contents).await?;
            report.files_written += 1;
        }
        // Remove stale .strm files (for example renamed episodes). Other files,
        // such as artwork Jellyfin saved next to the media, are left alone.
        let wanted: BTreeSet<PathBuf> = item.files.keys().map(|r| self.root.join(r)).collect();
        let mut stack = vec![(self.root.join(&item.dir), 0u8)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                let Ok(file_type) = entry.file_type().await else {
                    continue;
                };
                if file_type.is_dir() && depth < 2 {
                    stack.push((path, depth + 1));
                } else if file_type.is_file()
                    && path.extension().is_some_and(|ext| ext == "strm")
                    && !wanted.contains(&path)
                {
                    tokio::fs::remove_file(&path).await?;
                    report.files_removed += 1;
                }
            }
        }
        Ok(())
    }

    /// Deletes a folder only if it is exactly `movies/<name>` or `shows/<name>`.
    async fn remove_managed_dir(&self, relative: &str) -> bool {
        if !is_managed_dir(relative) {
            tracing::warn!(%relative, "refusing to remove an unmanaged path");
            return false;
        }
        let path = self.root.join(relative);
        match tokio::fs::remove_dir_all(&path).await {
            Ok(()) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => {
                tracing::warn!(%relative, %error, "could not remove library folder");
                false
            }
        }
    }

    async fn load_state(&self) -> SyncState {
        match tokio::fs::read(&self.state_path).await {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => SyncState::default(),
        }
    }

    async fn refresh_jellyfin(&self) -> bool {
        let (Some(base), Some(key)) =
            (&self.settings.jellyfin_url, &self.settings.jellyfin_api_key)
        else {
            return false;
        };
        let url = format!("{}/Library/Refresh", base.as_str().trim_end_matches('/'));
        match self
            .http
            .post(url)
            .header("Authorization", format!("MediaBrowser Token=\"{key}\""))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => true,
            Ok(response) => {
                tracing::warn!(status = %response.status(), "Jellyfin library refresh was rejected");
                false
            }
            Err(error) => {
                tracing::warn!(error = %error.without_url(), "could not reach Jellyfin");
                false
            }
        }
    }
}

fn extend_items(wanted: &mut BTreeSet<(Kind, i64)>, items: Vec<tmdb::ListItem>) {
    for item in items {
        if let Some(kind) = Kind::parse(&item.media_type) {
            wanted.insert((kind, item.id));
        }
    }
}

fn valid_imdb(id: &str) -> bool {
    id.len() <= 12
        && id.starts_with("tt")
        && id.len() > 2
        && id[2..].bytes().all(|b| b.is_ascii_digit())
}

fn is_managed_dir(relative: &str) -> bool {
    let path = Path::new(relative);
    let components: Vec<_> = path.components().collect();
    matches!(
        components.as_slice(),
        [Component::Normal(top), Component::Normal(_)]
            if *top == "movies" || *top == "shows"
    )
}

/// Makes a title safe as a single path component on Linux, macOS and SMB shares.
pub fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim_matches(|c: char| c == '.' || c == ' ');
    let mut out: String = trimmed.chars().take(120).collect();
    if out.is_empty() {
        out.push_str("Untitled");
    }
    out
}

fn folder_name(title: &str, year: Option<&str>, id: i64) -> String {
    match year {
        Some(year) => format!("{} ({year}) [tmdbid-{id}]", sanitize(title)),
        None => format!("{} [tmdbid-{id}]", sanitize(title)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(data: &Path, library: &Path) -> Config {
        Config {
            host: "127.0.0.1".into(),
            port: 0,
            data_dir: data.to_path_buf(),
            library_dir: library.to_path_buf(),
            public_url: Url::parse("http://192.168.1.50:8080").unwrap_or_else(|_| unreachable!()),
            public_url_configured: true,
            api_key: None,
            tmdb_api_key: Some("test".into()),
            tmdb_read_token: None,
            proxy: None,
            max_height: 1080,
            audio_languages: Vec::new(),
            sync_interval: Duration::from_secs(3600),
            include_specials: false,
            auto_lists: Vec::new(),
            tmdb_lists: Vec::new(),
            jellyfin_url: None,
            jellyfin_api_key: None,
        }
    }

    async fn mock_tmdb() -> std::io::Result<String> {
        use axum::{Json, Router, routing::get};
        use serde_json::json;
        let app = Router::new()
            .route(
                "/movie/27205",
                get(|| async {
                    Json(json!({"title":"Inception","release_date":"2010-07-15","imdb_id":"tt1375666"}))
                }),
            )
            .route(
                "/tv/1",
                get(|| async {
                    Json(json!({
                        "name":"Show: One","first_air_date":"2020-01-01",
                        "seasons":[{"season_number":0,"episode_count":1},{"season_number":1,"episode_count":3}],
                        "external_ids":{"imdb_id":"tt0000001"}
                    }))
                }),
            )
            .route(
                "/tv/1/season/1",
                get(|| async {
                    Json(json!({"episodes":[
                        {"episode_number":1,"air_date":"2020-01-01"},
                        {"episode_number":2,"air_date":"2020-01-08"},
                        {"episode_number":3,"air_date":"2999-01-01"}
                    ]}))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(format!("http://{address}"))
    }

    #[tokio::test]
    async fn sync_writes_jellyfin_layout_and_only_removes_its_own_folders()
    -> Result<(), Box<dyn std::error::Error>> {
        let data = tempfile::tempdir()?;
        let root = tempfile::tempdir()?;
        let base = mock_tmdb().await?;
        let tmdb = Tmdb::new(Some("test".into()), None)?.with_base(&base);
        let config = test_config(data.path(), root.path());
        let library = Library::open(&config, tmdb, "playkey").await?;

        library.add(Kind::Movie, 27205).await?;
        library.add(Kind::Tv, 1).await?;
        let foreign = root.path().join("movies/My Own Rip (1999)");
        tokio::fs::create_dir_all(&foreign).await?;
        tokio::fs::write(foreign.join("rip.mkv"), b"x").await?;

        let report = library.sync().await;
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert_eq!((report.movies, report.shows, report.episodes), (1, 1, 2));

        let movie = root
            .path()
            .join("movies/Inception (2010) [tmdbid-27205]/Inception (2010) [tmdbid-27205].strm");
        assert_eq!(
            tokio::fs::read_to_string(&movie).await?,
            "http://192.168.1.50:8080/play/movie/27205?key=playkey&imdb=tt1375666"
        );
        let show = root.path().join("shows/Show One (2020) [tmdbid-1]");
        assert_eq!(
            tokio::fs::read_to_string(show.join("Season 01/Show One S01E02.strm")).await?,
            "http://192.168.1.50:8080/play/tv/1/1/2?key=playkey&imdb=tt0000001"
        );
        assert!(!show.join("Season 01/Show One S01E03.strm").exists());
        assert!(!show.join("Season 00").exists());

        // A second sync with nothing changed touches nothing.
        assert_eq!(library.sync().await.files_written, 0);

        // Stale .strm files go; files Jellyfin saved next to media stay.
        tokio::fs::write(show.join("Season 01/Old S01E09.strm"), "x").await?;
        tokio::fs::write(show.join("poster.jpg"), "x").await?;
        assert_eq!(library.sync().await.files_removed, 1);
        assert!(show.join("poster.jpg").exists());

        // Removing a title deletes only its folder.
        assert!(library.remove(Kind::Movie, 27205).await?);
        let report = library.sync().await;
        assert_eq!(report.folders_removed, 1);
        assert!(!movie.exists());
        assert!(foreign.join("rip.mkv").exists());
        assert!(show.exists());
        Ok(())
    }

    #[test]
    fn names_follow_jellyfin_conventions() {
        assert_eq!(
            folder_name("Mission: Impossible", Some("1996"), 954),
            "Mission Impossible (1996) [tmdbid-954]"
        );
        assert_eq!(folder_name("../..", None, 1), "Untitled [tmdbid-1]");
        assert_eq!(sanitize("  A/B\\C  "), "A B C");
    }

    #[test]
    fn only_top_level_item_folders_are_removable() {
        assert!(is_managed_dir("movies/Inception (2010) [tmdbid-27205]"));
        assert!(is_managed_dir("shows/X [tmdbid-1]"));
        assert!(!is_managed_dir("movies"));
        assert!(!is_managed_dir("movies/../etc"));
        assert!(!is_managed_dir("/etc/passwd"));
        assert!(!is_managed_dir("other/x"));
        assert!(!is_managed_dir("movies/a/b"));
    }

    #[test]
    fn imdb_ids_are_validated() {
        assert!(valid_imdb("tt1375666"));
        assert!(!valid_imdb("tt"));
        assert!(!valid_imdb("nm123"));
        assert!(!valid_imdb("tt12&x=1"));
    }
}
