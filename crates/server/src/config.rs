//! Environment configuration. Every option has a `MELLO_` variable; the most
//! common ones also accept a command-line flag.

use std::{path::PathBuf, time::Duration};

use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid value for {0}: {1}")]
    Invalid(&'static str, String),
    #[error("unknown option: {0}")]
    UnknownOption(String),
}

/// A built-in TMDB collection that is synced into the library automatically.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoList {
    pub kind: AutoListKind,
    pub limit: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoListKind {
    TrendingMovies,
    TrendingTv,
    PopularMovies,
    PopularTv,
    TopRatedMovies,
    TopRatedTv,
}

impl AutoListKind {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "trending_movies" => Self::TrendingMovies,
            "trending_tv" => Self::TrendingTv,
            "popular_movies" => Self::PopularMovies,
            "popular_tv" => Self::PopularTv,
            "top_rated_movies" => Self::TopRatedMovies,
            "top_rated_tv" => Self::TopRatedTv,
            _ => return None,
        })
    }

    /// TMDB v3 path, relative to the API root.
    pub fn path(self) -> &'static str {
        match self {
            Self::TrendingMovies => "/trending/movie/week",
            Self::TrendingTv => "/trending/tv/week",
            Self::PopularMovies => "/movie/popular",
            Self::PopularTv => "/tv/popular",
            Self::TopRatedMovies => "/movie/top_rated",
            Self::TopRatedTv => "/tv/top_rated",
        }
    }

    pub fn is_tv(self) -> bool {
        matches!(self, Self::TrendingTv | Self::PopularTv | Self::TopRatedTv)
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub data_dir: PathBuf,
    pub library_dir: PathBuf,
    /// Base URL Jellyfin (and its clients) use to reach this server. Written
    /// into every `.strm` file.
    pub public_url: Url,
    pub public_url_configured: bool,
    pub api_key: Option<String>,
    pub tmdb_api_key: Option<String>,
    pub tmdb_read_token: Option<String>,
    pub proxy: Option<String>,
    pub max_height: i64,
    pub audio_languages: Vec<String>,
    pub sync_interval: Duration,
    pub include_specials: bool,
    pub auto_lists: Vec<AutoList>,
    /// How long a title that dropped off an auto list stays in the library.
    pub auto_list_grace: Duration,
    pub tmdb_lists: Vec<u64>,
    pub jellyfin_url: Option<Url>,
    pub jellyfin_api_key: Option<String>,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn parse_url(name: &'static str, raw: &str) -> Result<Url, ConfigError> {
    let url = Url::parse(raw.trim_end_matches('/'))
        .map_err(|_| ConfigError::Invalid(name, raw.to_owned()))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ConfigError::Invalid(name, raw.to_owned()));
    }
    Ok(url)
}

fn parse_auto_lists(raw: &str) -> Result<Vec<AutoList>, ConfigError> {
    let mut lists = Vec::new();
    for entry in raw.split(',').map(str::trim).filter(|e| !e.is_empty()) {
        let (name, limit) = entry.split_once(':').unwrap_or((entry, "20"));
        let kind = AutoListKind::parse(name.trim())
            .ok_or_else(|| ConfigError::Invalid("MELLO_AUTO_LISTS", entry.to_owned()))?;
        let limit = limit
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|limit| (1..=200).contains(limit))
            .ok_or_else(|| ConfigError::Invalid("MELLO_AUTO_LISTS", entry.to_owned()))?;
        lists.push(AutoList { kind, limit });
    }
    Ok(lists)
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let mut host = env("MELLO_HOST").unwrap_or_else(|| "0.0.0.0".to_owned());
        let mut port = match env("MELLO_PORT") {
            Some(raw) => raw
                .parse::<u16>()
                .map_err(|_| ConfigError::Invalid("MELLO_PORT", raw))?,
            None => 8080,
        };
        let mut data_dir = PathBuf::from(env("MELLO_DATA_DIR").unwrap_or_else(|| "data".into()));
        let mut library_dir =
            PathBuf::from(env("MELLO_LIBRARY_DIR").unwrap_or_else(|| "library".into()));

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            let (flag, inline) = match arg.split_once('=') {
                Some((flag, value)) => (flag.to_owned(), Some(value.to_owned())),
                None => (arg.clone(), None),
            };
            let mut value = |name: &'static str| {
                inline
                    .clone()
                    .or_else(|| args.next())
                    .ok_or_else(|| ConfigError::Invalid(name, "missing value".into()))
            };
            match flag.as_str() {
                "-h" | "--help" => {
                    println!("{HELP}");
                    std::process::exit(0);
                }
                "--host" => host = value("--host")?,
                "-p" | "--port" => {
                    let raw = value("--port")?;
                    port = raw
                        .parse()
                        .map_err(|_| ConfigError::Invalid("--port", raw))?;
                }
                "--data-dir" => data_dir = PathBuf::from(value("--data-dir")?),
                "--library-dir" => library_dir = PathBuf::from(value("--library-dir")?),
                other => return Err(ConfigError::UnknownOption(other.to_owned())),
            }
        }

        let (public_url, public_url_configured) = match env("MELLO_PUBLIC_URL") {
            Some(raw) => (parse_url("MELLO_PUBLIC_URL", &raw)?, true),
            None => (
                parse_url("MELLO_PUBLIC_URL", &format!("http://localhost:{port}"))?,
                false,
            ),
        };

        let max_height = match env("MELLO_MAX_HEIGHT") {
            Some(raw) => raw
                .parse::<i64>()
                .ok()
                .filter(|h| (360..=4320).contains(h))
                .ok_or(ConfigError::Invalid("MELLO_MAX_HEIGHT", raw))?,
            None => 1080,
        };
        let sync_hours = match env("MELLO_SYNC_INTERVAL_HOURS") {
            Some(raw) => raw
                .parse::<u64>()
                .ok()
                .filter(|h| (1..=168).contains(h))
                .ok_or(ConfigError::Invalid("MELLO_SYNC_INTERVAL_HOURS", raw))?,
            None => 6,
        };
        let grace_days = match env("MELLO_AUTO_LIST_GRACE_DAYS") {
            Some(raw) => raw
                .parse::<u64>()
                .ok()
                .filter(|d| *d <= 365)
                .ok_or(ConfigError::Invalid("MELLO_AUTO_LIST_GRACE_DAYS", raw))?,
            None => 14,
        };
        let auto_lists = match env("MELLO_AUTO_LISTS") {
            Some(raw) => parse_auto_lists(&raw)?,
            None => Vec::new(),
        };
        let tmdb_lists = match env("MELLO_TMDB_LISTS") {
            Some(raw) => raw
                .split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(|id| {
                    id.parse::<u64>()
                        .map_err(|_| ConfigError::Invalid("MELLO_TMDB_LISTS", id.to_owned()))
                })
                .collect::<Result<_, _>>()?,
            None => Vec::new(),
        };
        let jellyfin_url = env("JELLYFIN_URL")
            .map(|raw| parse_url("JELLYFIN_URL", &raw))
            .transpose()?;

        Ok(Self {
            host,
            port,
            data_dir,
            library_dir,
            public_url,
            public_url_configured,
            api_key: env("MELLO_API_KEY"),
            tmdb_api_key: env("TMDB_API_KEY"),
            tmdb_read_token: env("TMDB_READ_ACCESS_TOKEN"),
            proxy: env("MELLO_PROXY"),
            max_height,
            audio_languages: env("MELLO_AUDIO_LANGUAGES")
                .map(|raw| {
                    raw.split(',')
                        .map(|lang| lang.trim().to_ascii_lowercase())
                        .filter(|lang| !lang.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            sync_interval: Duration::from_secs(sync_hours * 3600),
            include_specials: env("MELLO_INCLUDE_SPECIALS")
                .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes")),
            auto_lists,
            auto_list_grace: Duration::from_secs(grace_days * 86_400),
            tmdb_lists,
            jellyfin_url,
            jellyfin_api_key: env("JELLYFIN_API_KEY"),
        })
    }
}

const HELP: &str = "\
Usage: mello-jellyfin [OPTIONS]

Options:
      --host <HOST>          Address to bind [env MELLO_HOST, default 0.0.0.0]
  -p, --port <PORT>          Port to listen on [env MELLO_PORT, default 8080]
      --data-dir <PATH>      State directory [env MELLO_DATA_DIR, default ./data]
      --library-dir <PATH>   Where .strm files are written [env MELLO_LIBRARY_DIR, default ./library]
  -h, --help                 Print help

See README.md for the full list of environment variables.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_lists_parse_names_and_limits() {
        let lists = parse_auto_lists("trending_movies:40, popular_tv").unwrap_or_default();
        assert_eq!(
            lists,
            vec![
                AutoList {
                    kind: AutoListKind::TrendingMovies,
                    limit: 40
                },
                AutoList {
                    kind: AutoListKind::PopularTv,
                    limit: 20
                },
            ]
        );
        assert!(parse_auto_lists("nope:3").is_err());
        assert!(parse_auto_lists("popular_tv:0").is_err());
    }

    #[test]
    fn urls_must_be_plain_http_bases() {
        assert!(parse_url("X", "http://192.168.1.50:8080/").is_ok());
        assert!(parse_url("X", "ftp://host").is_err());
        assert!(parse_url("X", "http://host/?a=b").is_err());
    }
}
