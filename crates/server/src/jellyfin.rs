//! Opt-in Jellyfin library export. This reads a profile's My List and never changes it.
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use stream_core::{
    accounts::{Target, is_safe_identifier},
    catalog::{MediaType, Season, Title},
};
use stream_storage::accounts::ListKind;
use tokio::io::AsyncWriteExt;

use crate::state::AppState;

const MANIFEST: &str = ".mello-jellyfin.json";

/// Configuration deliberately has no Debug implementation: play_key is a capability.
pub struct JellyfinConfig {
    pub profile_id: String,
    pub library_dir: PathBuf,
    pub public_url: String,
    pub play_key: String,
}

impl JellyfinConfig {
    pub fn from_env() -> Result<Option<Self>, String> {
        let names = [
            "STREAM_JELLYFIN_PROFILE_ID",
            "STREAM_JELLYFIN_LIBRARY_DIR",
            "STREAM_JELLYFIN_PUBLIC_URL",
            "STREAM_JELLYFIN_PLAY_KEY",
        ];
        let values: Vec<_> = names.iter().map(|name| std::env::var(name).ok()).collect();
        if values.iter().all(Option::is_none) {
            return Ok(None);
        }
        let mut values = values.into_iter().zip(names).map(|(value, name)| {
            value
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("{name} is required when Jellyfin integration is enabled"))
        });
        let profile = values.next().ok_or("missing profile configuration")??;
        let directory = values.next().ok_or("missing directory configuration")??;
        let url = values.next().ok_or("missing URL configuration")??;
        let key = values.next().ok_or("missing key configuration")??;
        Self::new(profile, directory.into(), url, key).map(Some)
    }

    pub fn new(
        profile_id: String,
        library_dir: PathBuf,
        public_url: String,
        play_key: String,
    ) -> Result<Self, String> {
        if !is_safe_identifier(&profile_id) {
            return Err("invalid Jellyfin profile ID".into());
        }
        if library_dir.as_os_str().is_empty() {
            return Err("Jellyfin library directory must not be empty".into());
        }
        let url = reqwest::Url::parse(&public_url).map_err(|_| "invalid Jellyfin public URL")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err("Jellyfin public URL must be an HTTP(S) origin without credentials, query, fragment or path".into());
        }
        if play_key.len() < 32
            || play_key.len() > 128
            || !play_key.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(
                "Jellyfin play key must contain 32 to 128 random hexadecimal characters".into(),
            );
        }
        Ok(Self {
            profile_id,
            library_dir,
            public_url: url.as_str().trim_end_matches('/').to_owned(),
            play_key,
        })
    }
}

pub struct JellyfinBridge {
    pub config: JellyfinConfig,
    sync_lock: tokio::sync::Mutex<()>,
}

#[derive(Default, Debug, Serialize)]
pub struct SyncReport {
    pub files: usize,
    pub written: usize,
    pub removed: usize,
}

impl JellyfinBridge {
    pub fn new(config: JellyfinConfig) -> Self {
        Self {
            config,
            sync_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// Resolve the whole desired library before touching output. Missing catalog
    /// data must never turn a temporary outage into deletion of an existing show.
    pub async fn sync(&self, state: &AppState) -> Result<SyncReport, String> {
        let _guard = self.sync_lock.lock().await;
        let titles = state
            .accounts
            .list_titles(
                &state.household_account_id,
                &self.config.profile_id,
                ListKind::Library,
            )
            .await
            .map_err(|e| format!("cannot read Jellyfin profile library: {e}"))?;
        let mut files = BTreeMap::new();
        let today = chrono::DateTime::<Utc>::from(std::time::SystemTime::now()).date_naive();
        for saved in titles {
            let identity = saved.identity;
            let title = self.load_title(state, saved).await?;
            title
                .validate()
                .map_err(|e| format!("invalid Jellyfin title: {e}"))?;
            if title.identity != identity {
                return Err("invalid Jellyfin title identity".into());
            }
            match title.identity.media_type() {
                MediaType::Movie => add_movie(&self.config, &title, &mut files),
                MediaType::Tv => {
                    if !title.details {
                        return Err(format!(
                            "show {} has no detailed catalog snapshot",
                            title.identity.id()
                        ));
                    }
                    for summary in &title.seasons {
                        let season = self
                            .load_season(state, title.identity.id(), summary.season)
                            .await?;
                        if season.season != summary.season {
                            return Err("invalid Jellyfin season number".into());
                        }
                        add_season(&self.config, &title, &season, today, &mut files)?;
                    }
                }
            }
        }
        apply_files(&self.config.library_dir, files).await
    }

    /// A play capability can only resolve titles still selected by this profile.
    pub async fn playback_title(&self, state: &AppState, target: &Target) -> Result<Title, String> {
        let titles = state
            .accounts
            .list_titles(
                &state.household_account_id,
                &self.config.profile_id,
                ListKind::Library,
            )
            .await
            .map_err(|e| format!("cannot read Jellyfin library: {e}"))?;
        let saved = titles
            .into_iter()
            .find(|title| title.identity == *target.identity())
            .ok_or("title is not in the Jellyfin profile library")?;
        let title = self.load_title(state, saved).await?;
        title
            .validate()
            .map_err(|e| format!("invalid Jellyfin title: {e}"))?;
        if &title.identity != target.identity() {
            return Err("invalid Jellyfin title identity".into());
        }
        if target.identity().media_type() == MediaType::Tv {
            let season_number = i32::try_from(target.season().ok_or("missing season")?)
                .map_err(|_| "invalid season")?;
            let episode_number = i32::try_from(target.episode().ok_or("missing episode")?)
                .map_err(|_| "invalid episode")?;
            let season = self
                .load_season(state, title.identity.id(), season_number)
                .await?;
            season.validate().map_err(|e| e.to_string())?;
            if season.show_id != title.identity.id() || season.season != season_number {
                return Err("invalid Jellyfin season identity".into());
            }
            let aired = season.episodes.iter().any(|episode| {
                episode.episode == episode_number
                    && NaiveDate::parse_from_str(&episode.air_date, "%Y-%m-%d").is_ok_and(|date| {
                        date <= chrono::DateTime::<Utc>::from(std::time::SystemTime::now())
                            .date_naive()
                    })
            });
            if !aired {
                return Err("episode is not available in the Jellyfin library".into());
            }
        }
        Ok(title)
    }

    async fn load_title(&self, state: &AppState, saved: Title) -> Result<Title, String> {
        if let Some(worker) = &state.catalog_worker {
            let lookup = worker
                .title(saved.identity)
                .await
                .map_err(|e| format!("Jellyfin title lookup failed: {e}"))?;
            if let Some(record) = lookup.value {
                return Ok(record.data);
            }
        }
        if let Ok(record) = state
            .catalog
            .get(
                "title",
                &saved.identity.key(),
                chrono::DateTime::<Utc>::from(std::time::SystemTime::now()),
            )
            .await
        {
            return serde_json::from_value(record.data)
                .map_err(|e| format!("invalid cached Jellyfin title: {e}"));
        }
        Ok(saved)
    }

    async fn load_season(
        &self,
        state: &AppState,
        show_id: i64,
        season: i32,
    ) -> Result<Season, String> {
        if let Some(worker) = &state.catalog_worker {
            let lookup = worker
                .season(show_id, season)
                .await
                .map_err(|e| format!("Jellyfin season lookup failed: {e}"))?;
            if let Some(record) = lookup.value {
                return Ok(record.data);
            }
        }
        let record = state
            .catalog
            .get(
                "season",
                &format!("tv:{show_id}:{season}"),
                chrono::DateTime::<Utc>::from(std::time::SystemTime::now()),
            )
            .await
            .map_err(|e| format!("Jellyfin season unavailable: {e}"))?;
        serde_json::from_value(record.data)
            .map_err(|e| format!("invalid cached Jellyfin season: {e}"))
    }
}

fn label(title: &Title) -> String {
    let mut name: String = title
        .title
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .take(100)
        .collect();
    name = name.trim_matches([' ', '.']).to_owned();
    if name.is_empty() {
        name = "Mello".into();
    }
    format!("{name} [tmdbid-{}]", title.identity.id())
}

fn play_url(config: &JellyfinConfig, kind: &str, id: i64, season: i32, episode: i32) -> String {
    format!(
        "{}/integrations/jellyfin/play/{kind}/{id}/{season}/{episode}?key={}\n",
        config.public_url, config.play_key
    )
}

fn add_movie(config: &JellyfinConfig, title: &Title, files: &mut BTreeMap<String, String>) {
    let name = label(title);
    files.insert(
        format!("movies/{name}/{name}.strm"),
        play_url(config, "movie", title.identity.id(), 0, 0),
    );
}

fn add_season(
    config: &JellyfinConfig,
    title: &Title,
    season: &Season,
    today: NaiveDate,
    files: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    season
        .validate()
        .map_err(|e| format!("invalid Jellyfin season: {e}"))?;
    if season.show_id != title.identity.id() {
        return Err("Jellyfin season belongs to another show".into());
    }
    let name = label(title);
    for episode in &season.episodes {
        let Ok(air_date) = NaiveDate::parse_from_str(&episode.air_date, "%Y-%m-%d") else {
            continue;
        };
        if air_date > today {
            continue;
        }
        files.insert(
            format!(
                "shows/{name}/Season {:02}/{name} - S{:02}E{:02}.strm",
                season.season, season.season, episode.episode
            ),
            play_url(config, "tv", season.show_id, season.season, episode.episode),
        );
    }
    Ok(())
}

#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    version: u32,
    files: BTreeMap<String, String>,
    #[serde(default)]
    pending: BTreeMap<String, String>,
}

fn fingerprint(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn managed_path(path: &str) -> bool {
    let mut components = Path::new(path).components();
    matches!(components.next(), Some(Component::Normal(name)) if name == "movies" || name == "shows")
        && components.all(|c| matches!(c, Component::Normal(_)))
        && path.ends_with(".strm")
}

/// Inspect every component before filesystem access; never traverse symlinks in
/// the export root, managed directories, manifest, or destination files.
async fn inspect_path(path: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err("Jellyfin export paths must not contain '..'".into());
        }
        current.push(component);
        match tokio::fs::symlink_metadata(&current).await {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err("Jellyfin export refuses symbolic links".into());
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(format!("cannot inspect Jellyfin export: {e}")),
        }
    }
    Ok(())
}

async fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    inspect_path(path).await?;
    match tokio::fs::read(path).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read Jellyfin export: {e}")),
    }
}

async fn atomic_write(path: &Path, content: &[u8]) -> Result<(), String> {
    inspect_path(path).await?;
    let parent = path
        .parent()
        .ok_or("Jellyfin export has no parent directory")?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|e| format!("cannot create Jellyfin export directory: {e}"))?;
    inspect_path(parent).await?;
    let mut bytes = [0u8; 12];
    getrandom::fill(&mut bytes).map_err(|_| "cannot generate Jellyfin temporary filename")?;
    let temp = parent.join(format!(".mello-{}.tmp", fingerprint(&bytes)));
    let result = async {
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .await?;
        file.write_all(content).await?;
        file.sync_all().await?;
        tokio::fs::rename(&temp, path).await
    }
    .await;
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&temp).await;
        return Err(format!("cannot write Jellyfin export: {e}"));
    }
    Ok(())
}

async fn save_manifest(root: &Path, files: BTreeMap<String, String>) -> Result<(), String> {
    let bytes = serde_json::to_vec(&Manifest {
        version: 1,
        files,
        pending: BTreeMap::new(),
    })
    .map_err(|e| e.to_string())?;
    atomic_write(&root.join(MANIFEST), &bytes).await
}

async fn apply_files(root: &Path, files: BTreeMap<String, String>) -> Result<SyncReport, String> {
    inspect_path(root).await?;
    let mut old = match read_optional(&root.join(MANIFEST)).await? {
        Some(bytes) => serde_json::from_slice::<Manifest>(&bytes)
            .map_err(|_| "invalid Jellyfin ownership manifest")?,
        None => Manifest {
            version: 1,
            ..Manifest::default()
        },
    };
    if old.version != 1
        || old
            .files
            .keys()
            .chain(old.pending.keys())
            .chain(files.keys())
            .any(|path| !managed_path(path))
    {
        return Err("invalid Jellyfin ownership manifest paths".into());
    }
    // A journal written before any file mutation allows either the previous or
    // intended digest after a crash, without authorizing arbitrary replacements.
    for (path, pending_hash) in &old.pending {
        if let Some(bytes) = read_optional(&root.join(path)).await? {
            let actual = fingerprint(&bytes);
            if &actual == pending_hash {
                old.files.insert(path.clone(), pending_hash.clone());
            } else if old.files.get(path) != Some(&actual) {
                return Err(format!(
                    "Jellyfin export refuses modified interrupted file: {path}"
                ));
            }
        } else {
            old.files.insert(path.clone(), pending_hash.clone());
        }
    }
    // Preflight everything before mutation: foreign or modified files are never replaced.
    for path in old.files.keys().chain(files.keys()) {
        if let Some(bytes) = read_optional(&root.join(path)).await?
            && old
                .files
                .get(path)
                .is_none_or(|expected| expected != &fingerprint(&bytes))
        {
            return Err(format!(
                "Jellyfin export refuses foreign or modified file: {path}"
            ));
        }
    }
    inspect_path(&root.join("movies")).await?;
    inspect_path(&root.join("shows")).await?;
    tokio::fs::create_dir_all(root.join("movies"))
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::create_dir_all(root.join("shows"))
        .await
        .map_err(|e| e.to_string())?;
    let mut report = SyncReport {
        files: files.len(),
        ..SyncReport::default()
    };
    let desired: BTreeMap<_, _> = files
        .iter()
        .map(|(path, body)| (path.clone(), fingerprint(body.as_bytes())))
        .collect();
    let journal = Manifest {
        version: 1,
        files: old.files.clone(),
        pending: desired.clone(),
    };
    let journal_bytes = serde_json::to_vec(&journal).map_err(|e| e.to_string())?;
    atomic_write(&root.join(MANIFEST), &journal_bytes).await?;
    for (path, content) in &files {
        if read_optional(&root.join(path)).await?.as_deref() != Some(content.as_bytes()) {
            atomic_write(&root.join(path), content.as_bytes()).await?;
            report.written += 1;
        }
    }
    for path in old.files.keys().filter(|path| !desired.contains_key(*path)) {
        if let Some(bytes) = read_optional(&root.join(path)).await? {
            if old.files.get(path) != Some(&fingerprint(&bytes)) {
                return Err("Jellyfin export file changed during synchronization".into());
            }
            tokio::fs::remove_file(root.join(path))
                .await
                .map_err(|e| e.to_string())?;
            report.removed += 1;
        }
    }
    save_manifest(root, desired).await?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn export_updates_prunes_and_preserves_foreign_files()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::Builder::new().tempdir_in(std::env::temp_dir().canonicalize()?)?;
        let path = "movies/Example [tmdbid-1]/Example.strm".to_owned();
        let initial = BTreeMap::from([(
            path.clone(),
            "https://mello.example/play?key=secret\n".into(),
        )]);
        assert_eq!(apply_files(dir.path(), initial.clone()).await?.written, 1);
        assert_eq!(apply_files(dir.path(), initial).await?.written, 0);
        tokio::fs::write(dir.path().join("movies/foreign.txt"), "keep").await?;
        let report = apply_files(dir.path(), BTreeMap::new()).await?;
        assert_eq!(report.removed, 1);
        assert_eq!(
            tokio::fs::read_to_string(dir.path().join("movies/foreign.txt")).await?,
            "keep"
        );
        assert!(!dir.path().join(path).exists());
        Ok(())
    }

    #[tokio::test]
    async fn export_refuses_modified_and_unowned_destinations()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::Builder::new().tempdir_in(std::env::temp_dir().canonicalize()?)?;
        let path = "movies/Example/Example.strm".to_owned();
        let desired = BTreeMap::from([(path.clone(), "ours\n".into())]);
        apply_files(dir.path(), desired.clone()).await?;
        tokio::fs::write(dir.path().join(&path), "user edit").await?;
        assert!(apply_files(dir.path(), BTreeMap::new()).await.is_err());
        assert_eq!(
            tokio::fs::read_to_string(dir.path().join(&path)).await?,
            "user edit"
        );
        tokio::fs::remove_file(dir.path().join(MANIFEST)).await?;
        assert!(apply_files(dir.path(), desired).await.is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn export_refuses_symlink_directories() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::Builder::new().tempdir_in(std::env::temp_dir().canonicalize()?)?;
        let outside = tempfile::Builder::new().tempdir_in(std::env::temp_dir().canonicalize()?)?;
        std::os::unix::fs::symlink(outside.path(), dir.path().join("movies"))?;
        let desired = BTreeMap::from([("movies/Example/Example.strm".into(), "ours".into())]);
        assert!(apply_files(dir.path(), desired).await.is_err());
        assert_eq!(std::fs::read_dir(outside.path())?.count(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn interrupted_updates_and_new_files_recover_without_claiming_foreign_files()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::Builder::new().tempdir_in(std::env::temp_dir().canonicalize()?)?;
        let previous = "movies/Previous/Previous.strm".to_owned();
        let added = "movies/Added/Added.strm".to_owned();
        apply_files(
            dir.path(),
            BTreeMap::from([(previous.clone(), "before".into())]),
        )
        .await?;
        let desired = BTreeMap::from([
            (previous.clone(), "after".to_owned()),
            (added.clone(), "added".to_owned()),
        ]);
        let journal = Manifest {
            version: 1,
            files: BTreeMap::from([(previous.clone(), fingerprint(b"before"))]),
            pending: desired
                .iter()
                .map(|(path, content)| (path.clone(), fingerprint(content.as_bytes())))
                .collect(),
        };
        atomic_write(&dir.path().join(MANIFEST), &serde_json::to_vec(&journal)?).await?;
        // Crash after updating one old file and creating one new file, before commit.
        atomic_write(&dir.path().join(&previous), b"after").await?;
        atomic_write(&dir.path().join(&added), b"added").await?;
        let result = apply_files(dir.path(), desired).await?;
        assert_eq!(result.written, 0);
        assert_eq!(result.files, 2);
        let committed: Manifest =
            serde_json::from_slice(&tokio::fs::read(dir.path().join(MANIFEST)).await?)?;
        assert!(committed.pending.is_empty());
        tokio::fs::write(dir.path().join(&added), "foreign edit").await?;
        assert!(apply_files(dir.path(), BTreeMap::new()).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn sync_exports_aired_episodes_and_retains_export_when_catalog_is_incomplete()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::Arc;
        use stream_core::{
            accounts::{Preferences, Profile},
            catalog::TitleIdentity,
        };
        use stream_storage::{AccountStore, CatalogStore, catalog::CatalogWrite, db::Database};
        let dir = tempfile::Builder::new().tempdir_in(std::env::temp_dir().canonicalize()?)?;
        let accounts = Arc::new(
            AccountStore::from_database(Database::in_memory(64).await?, [0x5c; 32]).await?,
        );
        let household = accounts.select_household(None).await?;
        accounts
            .create_profile(
                &household,
                Profile {
                    id: "jellyfin".into(),
                    name: "Jellyfin".into(),
                    color: "blue".into(),
                    preferences: Preferences::default(),
                },
            )
            .await?;
        let catalog = Arc::new(CatalogStore::from_database(Database::in_memory(64).await?).await?);
        let state = AppState::new(
            catalog.clone(),
            accounts.clone(),
            Arc::new(crate::state::NoopMediaEngine),
            household.clone(),
        );
        let title: Title = serde_json::from_value(
            serde_json::json!({"type":"tv", "id":42, "title":"Example/Show", "overview":"", "rating":0.0, "details":true,"seasons":[{"season":1,"name":"Season 1","episode_count":3}]}),
        )?;
        accounts
            .put_title(&household, "jellyfin", ListKind::Library, title)
            .await?;
        let season = serde_json::json!({"show_id":42,"season":1,"name":"Season 1","episodes":[
            {"tmdb_episode_id":1,"season":1,"episode":1,"name":"Aired","overview":"","runtime":24,"air_date":"2020-01-01"},
            {"tmdb_episode_id":2,"season":1,"episode":2,"name":"Future","overview":"","runtime":24,"air_date":"2999-01-01"},
            {"tmdb_episode_id":3,"season":1,"episode":3,"name":"Unknown","overview":"","runtime":24,"air_date":""}
        ]});
        catalog
            .put(
                CatalogWrite {
                    kind: "season".into(),
                    key: "tv:42:1".into(),
                    data: season,
                    ttl: chrono::Duration::hours(1),
                },
                std::time::SystemTime::now().into(),
            )
            .await?;
        let bridge = JellyfinBridge::new(JellyfinConfig::new(
            "jellyfin".into(),
            dir.path().into(),
            "http://mello:8080".into(),
            "0123456789abcdef0123456789abcdef".into(),
        )?);
        let report = bridge.sync(&state).await?;
        assert_eq!(report.files, 1);
        let path = dir.path().join(
            "shows/Example_Show [tmdbid-42]/Season 01/Example_Show [tmdbid-42] - S01E01.strm",
        );
        let content = tokio::fs::read_to_string(&path).await?;
        assert!(content.starts_with("http://mello:8080/integrations/jellyfin/play/tv/42/1/1?key="));
        let target = Target::new(TitleIdentity::new(MediaType::Tv, 42)?, Some(1), Some(1))?;
        assert!(bridge.playback_title(&state, &target).await.is_ok());
        let future = Target::new(TitleIdentity::new(MediaType::Tv, 42)?, Some(1), Some(2))?;
        assert!(bridge.playback_title(&state, &future).await.is_err());
        catalog.delete("season", "tv:42:1").await?;
        assert!(bridge.sync(&state).await.is_err());
        assert_eq!(tokio::fs::read_to_string(&path).await?, content);
        accounts
            .remove_title(
                &household,
                "jellyfin",
                ListKind::Library,
                TitleIdentity::new(MediaType::Tv, 42)?,
            )
            .await?;
        assert!(bridge.playback_title(&state, &target).await.is_err());
        assert_eq!(bridge.sync(&state).await?.removed, 1);
        Ok(())
    }

    #[test]
    fn rejects_unsafe_configuration() {
        assert!(
            JellyfinConfig::new(
                "profile".into(),
                "export".into(),
                "http://mello:8080".into(),
                "0123456789abcdef0123456789abcdef".into()
            )
            .is_ok()
        );
        for url in [
            "file:///tmp",
            "http://user:pass@mello",
            "http://mello/path",
            "http://mello?key=x",
        ] {
            assert!(
                JellyfinConfig::new(
                    "profile".into(),
                    "export".into(),
                    url.into(),
                    "0123456789abcdef0123456789abcdef".into()
                )
                .is_err()
            );
        }
    }
}
