//! Opt-in Jellyfin library export. This reads a profile's My List and never changes it.
use std::{
    collections::{BTreeMap, HashSet},
    path::{Component, Path, PathBuf},
};

use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use stream_core::{
    accounts::{PreferredFormat, Target, is_safe_identifier},
    catalog::{
        ContentPreference, Favorite, MediaType, RecommendationInputs, Recommendations, Season,
        Shelf, Title, TitleIdentity, ViewingSignal, valid_genre,
    },
};
use stream_storage::accounts::ListKind;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::state::AppState;

const MANIFEST: &str = ".mello-jellyfin.json";
const MAX_EXPORT_FILES: usize = 20_000;
const MAX_MANIFEST_BYTES: u64 = 32 * 1024 * 1024;
const MAX_STREAM_FILE_BYTES: u64 = 8 * 1024;

/// Configuration deliberately has no Debug implementation: play_key is a capability.
pub struct JellyfinConfig {
    pub profile_id: String,
    pub library_dir: PathBuf,
    pub public_url: String,
    pub play_key: String,
    pub server_url: Option<String>,
    pub api_key: Option<String>,
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
        let mut config = Self::new(profile, directory.into(), url, key)?;
        let server_url = std::env::var("STREAM_JELLYFIN_SERVER_URL")
            .or_else(|_| std::env::var("JELLYFIN_URL"))
            .ok()
            .filter(|v| !v.trim().is_empty());
        let api_key = std::env::var("STREAM_JELLYFIN_API_KEY")
            .or_else(|_| std::env::var("JELLYFIN_API_KEY"))
            .ok()
            .filter(|v| !v.trim().is_empty());
        config.server_url = server_url;
        config.api_key = api_key;
        Ok(Some(config))
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
            server_url: None,
            api_key: None,
        })
    }

    pub fn with_server_refresh(
        mut self,
        server_url: Option<String>,
        api_key: Option<String>,
    ) -> Self {
        self.server_url = server_url;
        self.api_key = api_key;
        self
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
        let labels = existing_labels(&self.config.library_dir).await?;
        let (titles, library_identities) = self.collect_desired_titles(state).await?;
        let mut files = BTreeMap::new();
        let today = chrono::DateTime::<Utc>::from(std::time::SystemTime::now()).date_naive();
        for saved in titles {
            let identity = saved.identity;
            let is_library = library_identities.contains(&identity.key());
            let title = match self.load_title(state, saved).await {
                Ok(t) => t,
                Err(e) if is_library => return Err(e),
                Err(e) => {
                    tracing::warn!(id = identity.id(), %e, "skipping auto-discovered Jellyfin title");
                    continue;
                }
            };
            if let Err(e) = title.validate() {
                if is_library {
                    return Err(format!("invalid Jellyfin title: {e}"));
                } else {
                    tracing::warn!(id = identity.id(), %e, "skipping invalid auto-discovered title");
                    continue;
                }
            }
            if title.identity != identity {
                if is_library {
                    return Err("invalid Jellyfin title identity".into());
                } else {
                    continue;
                }
            }
            match title.identity.media_type() {
                MediaType::Movie => add_movie(
                    &self.config,
                    &title,
                    labels
                        .get(&("movie".to_owned(), identity.id()))
                        .map(String::as_str),
                    &mut files,
                ),
                MediaType::Tv => {
                    if !title.details {
                        if is_library {
                            return Err(format!(
                                "show {} has no detailed catalog snapshot",
                                title.identity.id()
                            ));
                        } else {
                            tracing::warn!(
                                id = identity.id(),
                                "auto show has no details, skipping"
                            );
                            continue;
                        }
                    }
                    let mut show_files = BTreeMap::new();
                    let mut show_failed = false;
                    for summary in &title.seasons {
                        if summary.season < 1 && summary.episode_count == 0 {
                            continue;
                        }
                        let season = match self
                            .load_season(state, title.identity.id(), summary.season)
                            .await
                        {
                            Ok(s) => s,
                            Err(e) if is_library => return Err(e),
                            Err(e) => {
                                tracing::warn!(
                                    id = title.identity.id(),
                                    season = summary.season,
                                    %e,
                                    "failed to load auto season"
                                );
                                show_failed = true;
                                break;
                            }
                        };
                        if season.season != summary.season {
                            if is_library {
                                return Err("invalid Jellyfin season number".into());
                            } else {
                                show_failed = true;
                                break;
                            }
                        }
                        if season.episodes.len()
                            < usize::try_from(summary.episode_count)
                                .map_err(|_| "invalid Jellyfin episode count")?
                        {
                            if is_library {
                                return Err(format!(
                                    "Jellyfin season {}:{} is incomplete",
                                    season.show_id, season.season
                                ));
                            } else {
                                show_failed = true;
                                break;
                            }
                        }
                        if let Err(e) = add_season(
                            &self.config,
                            &title,
                            &season,
                            today,
                            labels
                                .get(&("tv".to_owned(), identity.id()))
                                .map(String::as_str),
                            &mut show_files,
                        ) {
                            if is_library {
                                return Err(e);
                            } else {
                                show_failed = true;
                                break;
                            }
                        }
                    }
                    if !show_failed {
                        files.extend(show_files);
                    }
                }
            }
        }
        let report = apply_files(&self.config.library_dir, files).await?;
        if (report.written > 0 || report.removed > 0)
            && let (Some(server_url), Some(api_key)) =
                (&self.config.server_url, &self.config.api_key)
        {
            self.refresh_jellyfin_server(server_url, api_key).await;
        }
        Ok(report)
    }

    async fn refresh_jellyfin_server(&self, server_url: &str, api_key: &str) {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .ok();
        if let Some(client) = client {
            let refresh_url = format!("{}/Library/Refresh", server_url.trim_end_matches('/'));
            let res = client
                .post(&refresh_url)
                .header(
                    "Authorization",
                    format!(r#"MediaBrowser Token="{api_key}""#),
                )
                .header("X-Emby-Token", api_key)
                .send()
                .await;
            match res {
                Ok(resp) if resp.status().is_success() => {
                    tracing::info!("triggered Jellyfin library refresh");
                }
                Ok(resp) => {
                    tracing::warn!(
                        status = %resp.status(),
                        "Jellyfin library refresh returned unexpected status"
                    );
                }
                Err(err) => {
                    tracing::warn!(%err, "failed to trigger Jellyfin library refresh");
                }
            }
        }
    }

    async fn collect_desired_titles(
        &self,
        state: &AppState,
    ) -> Result<(Vec<Title>, HashSet<String>), String> {
        let mut desired_map = BTreeMap::<String, Title>::new();
        let mut library_identities = HashSet::<String>::new();

        // 1. Explicit library
        let library_titles = state
            .accounts
            .list_titles(
                &state.household_account_id,
                &self.config.profile_id,
                ListKind::Library,
            )
            .await
            .map_err(|e| format!("cannot read Jellyfin profile library: {e}"))?;
        for title in library_titles {
            let key = title.identity.key();
            library_identities.insert(key.clone());
            desired_map.insert(key, title);
        }

        let now: chrono::DateTime<Utc> = std::time::SystemTime::now().into();

        // 2. Personalized recommendations
        let mut recs: Option<Recommendations> = None;
        if let Ok(record) = state
            .catalog
            .get("recommendations", &self.config.profile_id, now)
            .await
            && record.expires_at > now
        {
            recs = serde_json::from_value::<Recommendations>(record.data).ok();
        }
        if recs.is_none()
            && let Some(worker) = &state.catalog_worker
            && worker.available()
            && let Some(inputs) = self.build_recommendation_inputs(state).await
            && let Ok(fresh) = worker
                .refresh_recommendations(&self.config.profile_id, inputs)
                .await
        {
            recs = Some(fresh.data);
        }
        if let Some(recs) = recs {
            for title in recs.pool {
                desired_map.entry(title.identity.key()).or_insert(title);
            }
            for section in recs.sections {
                for title in section.titles {
                    desired_map.entry(title.identity.key()).or_insert(title);
                }
            }
        }

        // 3. Trending & Popular shelves
        let home_shelves = ["trending", "movie-popular", "tv-popular"];
        for key in home_shelves {
            if let Ok(record) = state.catalog.get("shelf", key, now).await
                && let Ok(shelf) = serde_json::from_value::<Shelf>(record.data)
            {
                for title in shelf.titles {
                    desired_map.entry(title.identity.key()).or_insert(title);
                }
            }
        }

        // 4. Any other shelves in catalog store
        if let Ok(shelves) = state.catalog.list("shelf", 50, now).await {
            for record in shelves {
                if let Ok(shelf) = serde_json::from_value::<Shelf>(record.data) {
                    for title in shelf.titles {
                        desired_map.entry(title.identity.key()).or_insert(title);
                    }
                }
            }
        }

        let titles: Vec<Title> = desired_map.into_values().collect();
        Ok((titles, library_identities))
    }

    async fn build_recommendation_inputs(&self, state: &AppState) -> Option<RecommendationInputs> {
        let profile = state
            .accounts
            .get_profile(&state.household_account_id, &self.config.profile_id)
            .await
            .ok()?;
        let ignored: HashSet<String> = state
            .accounts
            .list_titles(
                &state.household_account_id,
                &self.config.profile_id,
                ListKind::RecommendationIgnored,
            )
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|title| title.identity.key())
            .collect();
        let mut favorites: Vec<Favorite> = state
            .accounts
            .list_titles(
                &state.household_account_id,
                &self.config.profile_id,
                ListKind::Favorites,
            )
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|title| !ignored.contains(&title.identity.key()))
            .map(|title| Favorite {
                identity: title.identity,
                title: title.title,
            })
            .collect();
        favorites.truncate(50);
        let mut genres: Vec<String> = profile
            .preferences
            .genres
            .into_iter()
            .filter(|g| valid_genre(g))
            .take(9)
            .collect();
        if genres.is_empty() {
            genres = vec![
                "action".into(),
                "comedy".into(),
                "drama".into(),
                "scifi".into(),
            ];
        }
        let mut history = state
            .accounts
            .history(&state.household_account_id, &self.config.profile_id, 0)
            .await
            .unwrap_or_default();
        history.retain(|progress| !ignored.contains(&progress.target.identity().key()));
        let mut grouped_signals = std::collections::HashMap::<String, ViewingSignal>::new();
        for progress in history.iter().take(100) {
            let identity = *progress.target.identity();
            let key = identity.key();
            let signal = grouped_signals.entry(key).or_insert_with(|| ViewingSignal {
                identity,
                title: String::new(),
                watched_seconds: 0.0,
                duration_seconds: 0.0,
                completed: false,
                meaningful_episodes: 0,
                updated_at: progress.updated_at,
            });
            signal.watched_seconds += progress.watched_seconds;
            signal.duration_seconds = signal.duration_seconds.max(progress.duration_seconds);
            signal.completed |= progress.completed;
            if progress.target.season().is_some() && progress.completed {
                signal.meaningful_episodes = signal.meaningful_episodes.saturating_add(1);
            }
            signal.updated_at = signal.updated_at.max(progress.updated_at);
        }
        let mut signals: Vec<ViewingSignal> = grouped_signals.into_values().collect();
        signals.truncate(100);
        let format = match profile.preferences.format {
            PreferredFormat::Both => ContentPreference::Both,
            PreferredFormat::Movie => ContentPreference::Movie,
            PreferredFormat::Tv => ContentPreference::Tv,
        };
        Some(RecommendationInputs {
            completed: true,
            format,
            genres,
            favorites,
            signals,
            activity_days: Vec::new(),
        })
    }

    /// A play capability can resolve titles in library, recommendations, or shelves.
    /// Titles played from recommendations or shelves are automatically added to the library.
    pub async fn playback_title(&self, state: &AppState, target: &Target) -> Result<Title, String> {
        let (saved, is_library) = self.find_playback_title(state, target.identity()).await?;
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
        if !is_library {
            let _ = state
                .accounts
                .put_title(
                    &state.household_account_id,
                    &self.config.profile_id,
                    ListKind::Library,
                    title.clone(),
                )
                .await;
        }
        Ok(title)
    }

    async fn find_playback_title(
        &self,
        state: &AppState,
        identity: &TitleIdentity,
    ) -> Result<(Title, bool), String> {
        let titles = state
            .accounts
            .list_titles(
                &state.household_account_id,
                &self.config.profile_id,
                ListKind::Library,
            )
            .await
            .map_err(|e| format!("cannot read Jellyfin library: {e}"))?;
        if let Some(saved) = titles.into_iter().find(|t| &t.identity == identity) {
            return Ok((saved, true));
        }

        let now: chrono::DateTime<Utc> = std::time::SystemTime::now().into();
        if let Ok(record) = state
            .catalog
            .get("recommendations", &self.config.profile_id, now)
            .await
            && let Ok(recs) = serde_json::from_value::<Recommendations>(record.data)
        {
            if let Some(saved) = recs.pool.into_iter().find(|t| &t.identity == identity) {
                return Ok((saved, false));
            }
            for section in recs.sections {
                if let Some(saved) = section
                    .titles
                    .into_iter()
                    .find(|t| &t.identity == identity)
                {
                    return Ok((saved, false));
                }
            }
        }

        let home_shelves = ["trending", "movie-popular", "tv-popular"];
        for key in home_shelves {
            if let Ok(record) = state.catalog.get("shelf", key, now).await
                && let Ok(shelf) = serde_json::from_value::<Shelf>(record.data)
                && let Some(saved) = shelf.titles.into_iter().find(|t| &t.identity == identity)
            {
                return Ok((saved, false));
            }
        }

        if let Ok(shelves) = state.catalog.list("shelf", 50, now).await {
            for record in shelves {
                if let Ok(shelf) = serde_json::from_value::<Shelf>(record.data)
                    && let Some(saved) = shelf.titles.into_iter().find(|t| &t.identity == identity)
                {
                    return Ok((saved, false));
                }
            }
        }

        Err("title is not in the Jellyfin profile library".into())
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
        // Reserve space for the TMDB suffix and episode filename suffix within
        // the common 255-byte component limit, including multibyte title text.
        .scan(0usize, |length, character| {
            *length += character.len_utf8();
            (*length <= 128).then_some(character)
        })
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

fn add_movie(
    config: &JellyfinConfig,
    title: &Title,
    existing_label: Option<&str>,
    files: &mut BTreeMap<String, String>,
) {
    let name = existing_label
        .map(str::to_owned)
        .unwrap_or_else(|| label(title));
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
    existing_label: Option<&str>,
    files: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    season
        .validate()
        .map_err(|e| format!("invalid Jellyfin season: {e}"))?;
    if season.show_id != title.identity.id() {
        return Err("Jellyfin season belongs to another show".into());
    }
    let name = existing_label
        .map(str::to_owned)
        .unwrap_or_else(|| label(title));
    for episode in &season.episodes {
        let Ok(air_date) = NaiveDate::parse_from_str(&episode.air_date, "%Y-%m-%d") else {
            continue;
        };
        if air_date > today {
            continue;
        }
        if files.len() >= MAX_EXPORT_FILES {
            return Err(format!("Jellyfin export exceeds {MAX_EXPORT_FILES} files"));
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

/// Preserve the existing provider-ID folder and filename when TMDB corrects or
/// localizes a title. Jellyfin must not see a removed item followed by a new path.
async fn existing_labels(root: &Path) -> Result<BTreeMap<(String, i64), String>, String> {
    let manifest = read_manifest(root).await?;
    let mut labels = BTreeMap::new();
    for path in manifest.files.keys().chain(manifest.pending.keys()) {
        let components: Vec<_> = path.split('/').collect();
        let (kind, folder) = match components.as_slice() {
            ["movies", folder, file] if *file == format!("{folder}.strm") => ("movie", *folder),
            ["shows", folder, season_folder, file] => {
                let season = season_folder
                    .strip_prefix("Season ")
                    .and_then(|s| s.parse::<i32>().ok())
                    .filter(|s| (0..=10_000).contains(s))
                    .ok_or("invalid Jellyfin show ownership path")?;
                let prefix = format!("{folder} - S{season:02}E");
                let episode = file
                    .strip_prefix(&prefix)
                    .and_then(|s| s.strip_suffix(".strm"))
                    .and_then(|s| s.parse::<i32>().ok())
                    .filter(|n| (1..=100_000).contains(n))
                    .ok_or("invalid Jellyfin episode ownership path")?;
                if *season_folder != format!("Season {season:02}")
                    || *file != format!("{prefix}{episode:02}.strm")
                {
                    return Err("invalid Jellyfin episode ownership path".into());
                }
                ("tv", *folder)
            }
            _ => return Err("invalid Jellyfin ownership folder structure".into()),
        };
        let (name, suffix) = folder
            .rsplit_once(" [tmdbid-")
            .ok_or("missing Jellyfin ownership TMDB ID")?;
        let id = suffix
            .strip_suffix(']')
            .and_then(|s| s.parse::<i64>().ok())
            .filter(|id| *id > 0)
            .ok_or("invalid Jellyfin ownership TMDB ID")?;
        if name.is_empty() || folder != format!("{name} [tmdbid-{id}]") {
            return Err("invalid Jellyfin ownership folder name".into());
        }
        let key = (kind.to_owned(), id);
        if let Some(previous) = labels.insert(key, folder.to_owned())
            && previous != folder
        {
            return Err("ambiguous Jellyfin ownership folders for one TMDB ID".into());
        }
    }
    Ok(labels)
}

async fn read_manifest(root: &Path) -> Result<Manifest, String> {
    inspect_path(root).await?;
    let manifest = match read_optional(&root.join(MANIFEST)).await? {
        Some(bytes) => serde_json::from_slice::<Manifest>(&bytes)
            .map_err(|_| "invalid Jellyfin ownership manifest")?,
        None => Manifest {
            version: 1,
            ..Manifest::default()
        },
    };
    if manifest.version != 1
        || manifest.pending.len() > MAX_EXPORT_FILES
        || manifest
            .files
            .keys()
            .chain(manifest.pending.keys())
            .any(|path| !managed_path(path))
    {
        return Err("invalid Jellyfin ownership manifest paths".into());
    }
    Ok(manifest)
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
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot inspect Jellyfin export file: {e}")),
    };
    if !metadata.is_file() {
        return Err("Jellyfin export refuses non-regular files".into());
    }
    let limit = if path.file_name().is_some_and(|name| name == MANIFEST) {
        MAX_MANIFEST_BYTES
    } else {
        MAX_STREAM_FILE_BYTES
    };
    if metadata.len() > limit {
        return Err("Jellyfin export file exceeds size limit".into());
    }
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|e| format!("cannot read Jellyfin export: {e}"))?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| format!("cannot read Jellyfin export: {e}"))?;
    if bytes.len() as u64 > limit {
        return Err("Jellyfin export file exceeds size limit".into());
    }
    Ok(Some(bytes))
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
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("Jellyfin ownership manifest exceeds size limit".into());
    }
    atomic_write(&root.join(MANIFEST), &bytes).await
}

async fn apply_files(root: &Path, files: BTreeMap<String, String>) -> Result<SyncReport, String> {
    if files.len() > MAX_EXPORT_FILES {
        return Err(format!("Jellyfin export exceeds {MAX_EXPORT_FILES} files"));
    }
    let mut old = read_manifest(root).await?;
    if files.keys().any(|path| !managed_path(path)) {
        return Err("invalid Jellyfin export paths".into());
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
    if journal_bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("Jellyfin ownership manifest exceeds size limit".into());
    }
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
            .put_title(&household, "jellyfin", ListKind::Library, title.clone())
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
                    data: season.clone(),
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
        let mut renamed = title;
        renamed.title = "Corrected show title".into();
        accounts
            .put_title(&household, "jellyfin", ListKind::Library, renamed)
            .await?;
        let unchanged = bridge.sync(&state).await?;
        assert_eq!((unchanged.written, unchanged.removed), (0, 0));
        assert_eq!(tokio::fs::read_to_string(&path).await?, content);
        let target = Target::new(TitleIdentity::new(MediaType::Tv, 42)?, Some(1), Some(1))?;
        assert!(bridge.playback_title(&state, &target).await.is_ok());
        let future = Target::new(TitleIdentity::new(MediaType::Tv, 42)?, Some(1), Some(2))?;
        assert!(bridge.playback_title(&state, &future).await.is_err());
        // A syntactically valid, partial season response must not delete aired
        // episodes that were exported from an earlier complete snapshot.
        let mut incomplete = season;
        incomplete["episodes"] = serde_json::json!([]);
        catalog
            .put(
                CatalogWrite {
                    kind: "season".into(),
                    key: "tv:42:1".into(),
                    data: incomplete,
                    ttl: chrono::Duration::hours(1),
                },
                std::time::SystemTime::now().into(),
            )
            .await?;
        assert!(bridge.sync(&state).await.is_err());
        assert_eq!(tokio::fs::read_to_string(&path).await?, content);
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

    #[tokio::test]
    async fn movie_metadata_rename_keeps_exact_owned_path() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::Builder::new().tempdir_in(std::env::temp_dir().canonicalize()?)?;
        let mut title: Title = serde_json::from_value(
            serde_json::json!({"type":"movie","id":42,"title":"Original title","overview":"","rating":0.0,"details":true}),
        )?;
        let config = JellyfinConfig::new(
            "profile".into(),
            dir.path().into(),
            "http://mello:8080".into(),
            "0123456789abcdef0123456789abcdef".into(),
        )?;
        let mut files = BTreeMap::new();
        add_movie(&config, &title, None, &mut files);
        let original_path = files.keys().next().ok_or("missing movie fixture")?.clone();
        apply_files(dir.path(), files).await?;
        let original_body = tokio::fs::read_to_string(dir.path().join(&original_path)).await?;
        title.title = "Localized or corrected title".into();
        let labels = existing_labels(dir.path()).await?;
        let mut files = BTreeMap::new();
        add_movie(
            &config,
            &title,
            labels.get(&("movie".to_owned(), 42)).map(String::as_str),
            &mut files,
        );
        let report = apply_files(dir.path(), files).await?;
        assert_eq!((report.written, report.removed), (0, 0));
        assert_eq!(
            tokio::fs::read_to_string(dir.path().join(original_path)).await?,
            original_body
        );
        Ok(())
    }

    #[tokio::test]
    async fn ambiguous_owned_title_folders_are_rejected() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::Builder::new().tempdir_in(std::env::temp_dir().canonicalize()?)?;
        let files = BTreeMap::from([
            (
                "movies/Old [tmdbid-42]/Old [tmdbid-42].strm".into(),
                "old".into(),
            ),
            (
                "movies/New [tmdbid-42]/New [tmdbid-42].strm".into(),
                "new".into(),
            ),
        ]);
        apply_files(dir.path(), files).await?;
        assert!(existing_labels(dir.path()).await.is_err());
        assert!(
            dir.path()
                .join("movies/Old [tmdbid-42]/Old [tmdbid-42].strm")
                .is_file()
        );
        assert!(
            dir.path()
                .join("movies/New [tmdbid-42]/New [tmdbid-42].strm")
                .is_file()
        );
        Ok(())
    }

    #[tokio::test]
    async fn multibyte_titles_fit_real_filesystem_component_limits()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::Builder::new().tempdir_in(std::env::temp_dir().canonicalize()?)?;
        let title: Title = serde_json::from_value(
            serde_json::json!({"type":"movie", "id":42,"title":"🎬映画".repeat(100),"overview":"","rating":0.0,"details":true}),
        )?;
        let config = JellyfinConfig::new(
            "profile".into(),
            dir.path().into(),
            "http://mello:8080".into(),
            "0123456789abcdef0123456789abcdef".into(),
        )?;
        let mut files = BTreeMap::new();
        add_movie(&config, &title, None, &mut files);
        assert_eq!(apply_files(dir.path(), files).await?.written, 1);
        Ok(())
    }

    #[tokio::test]
    async fn oversized_manifest_and_stream_files_are_rejected_before_reading()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::Builder::new().tempdir_in(std::env::temp_dir().canonicalize()?)?;
        let manifest = dir.path().join(MANIFEST);
        std::fs::File::create(&manifest)?.set_len(MAX_MANIFEST_BYTES + 1)?;
        assert!(apply_files(dir.path(), BTreeMap::new()).await.is_err());
        tokio::fs::remove_file(manifest).await?;
        let movie_dir = dir.path().join("movies/Example");
        tokio::fs::create_dir_all(&movie_dir).await?;
        std::fs::File::create(movie_dir.join("Example.strm"))?
            .set_len(MAX_STREAM_FILE_BYTES + 1)?;
        let desired = BTreeMap::from([("movies/Example/Example.strm".into(), "ours".into())]);
        assert!(apply_files(dir.path(), desired).await.is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fifo_manifest_is_rejected_without_waiting_for_writer()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::Builder::new().tempdir_in(std::env::temp_dir().canonicalize()?)?;
        let result = std::process::Command::new("mkfifo")
            .arg(dir.path().join(MANIFEST))
            .status()?;
        assert!(result.success());
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            apply_files(dir.path(), BTreeMap::new()),
        )
        .await?;
        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn sync_includes_trending_and_recommendations_and_promotes_on_play()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::Arc;
        use stream_core::accounts::{Preferences, Profile};
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

        // 1. Library title
        let lib_movie: Title = serde_json::from_value(
            serde_json::json!({"type":"movie", "id":42, "title":"Library Movie", "overview":"", "rating":0.0, "details":true}),
        )?;
        accounts
            .put_title(&household, "jellyfin", ListKind::Library, lib_movie)
            .await?;

        // 2. Shelf title (trending)
        let trend_movie: Title = serde_json::from_value(
            serde_json::json!({"type":"movie", "id":100, "title":"Trending Movie", "overview":"", "rating":0.0, "details":true}),
        )?;
        let shelf = Shelf {
            id: "trending".into(),
            heading: "Trending this week".into(),
            kind: "trending".into(),
            source: "tmdb".into(),
            reason: String::new(),
            titles: vec![trend_movie.clone()],
        };
        catalog
            .put(
                CatalogWrite {
                    kind: "shelf".into(),
                    key: "trending".into(),
                    data: serde_json::to_value(&shelf)?,
                    ttl: chrono::Duration::hours(1),
                },
                std::time::SystemTime::now().into(),
            )
            .await?;

        // 3. Recommendation title
        let rec_movie: Title = serde_json::from_value(
            serde_json::json!({"type":"movie", "id":200, "title":"Recommended Movie", "overview":"", "rating":0.0, "details":true}),
        )?;
        let recs = Recommendations {
            fingerprint: "test".into(),
            pool: vec![rec_movie.clone()],
            sections: Vec::new(),
        };
        catalog
            .put(
                CatalogWrite {
                    kind: "recommendations".into(),
                    key: "jellyfin".into(),
                    data: serde_json::to_value(&recs)?,
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
        assert_eq!(report.files, 3);
        assert_eq!(report.written, 3);

        // Trending movie is playable even though not in library yet
        let trend_target = Target::new(TitleIdentity::new(MediaType::Movie, 100)?, None, None)?;
        let played = bridge.playback_title(&state, &trend_target).await;
        assert!(played.is_ok());

        // Keep-on-play promoted trending movie to the permanent library
        let lib_titles = accounts
            .list_titles(&household, "jellyfin", ListKind::Library)
            .await?;
        assert!(lib_titles.iter().any(|t| t.identity.id() == 100));

        // Unknown title is rejected
        let unknown = Target::new(TitleIdentity::new(MediaType::Movie, 999)?, None, None)?;
        assert!(bridge.playback_title(&state, &unknown).await.is_err());

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
