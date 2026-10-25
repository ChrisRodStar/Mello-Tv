use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use stream_core::catalog::{
    CANONICAL_GENRES, CatalogRecord, CatalogSnapshot, CatalogValidationError, CrossIds, Episode,
    EpisodeDetail, MediaType, RecommendationInputs, Season, Title, TitleIdentity, season_key,
    valid_genre,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn time(value: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(value).map(|value| value.with_timezone(&Utc))
}

fn title_json() -> Value {
    // Existing Go TestTitle_JSONSerialization vector, retaining Go wire names.
    json!({
        "type": "movie", "id": 550, "title": "Fight Club",
        "overview": "An insomniac office worker...", "rating": 8.4,
        "genres": ["Drama"], "details": true
    })
}

fn recommendation_json() -> Value {
    json!({
        "completed": true, "format": "both", "genres": ["drama"],
        "favorites": [{"type": "movie", "id": 550, "title": "Fight Club"}],
        "signals": [{
            "type": "tv", "id": 60625, "title": "Example show",
            "watched_seconds": 3600.0, "duration_seconds": 1800.0,
            "completed": false, "meaningful_episodes": 2,
            "updated_at": "2026-10-24T19:00:00Z"
        }],
        "activity_days": ["2026-10-25", "2026-10-24"]
    })
}

#[test]
fn canonical_identity_keys_match_go_vectors() -> TestResult {
    for (media_type, id, expected) in [
        (MediaType::Movie, 27205, "movie:27205"),
        (MediaType::Tv, 60625, "tv:60625"),
    ] {
        let identity = TitleIdentity::new(media_type, id)?;
        assert_eq!(identity.key(), expected);
        assert_eq!(identity.media_type(), media_type);
        assert_eq!(identity.id(), id);
        let round_trip: TitleIdentity = serde_json::from_value(serde_json::to_value(identity)?)?;
        assert_eq!(round_trip, identity);
    }
    Ok(())
}

#[test]
fn invalid_identity_vectors_fail_at_deserialization() {
    for invalid in [
        json!({"type": "music", "id": 100}),
        json!({"type": "movie", "id": 0}),
        json!({"type": "movie", "id": -5}),
        json!({"type": "", "id": 100}),
        json!({"type": "tv", "id": "60625"}),
        json!({"type": "tv", "id": 1.5}),
    ] {
        assert!(serde_json::from_value::<TitleIdentity>(invalid).is_err());
    }
    assert!(TitleIdentity::new(MediaType::Movie, -1).is_err());
    assert!(TitleIdentity::new(MediaType::Movie, 0).is_err());
}

#[test]
fn identity_preserves_signed_64_bit_limit() -> TestResult {
    let identity = TitleIdentity::new(MediaType::Movie, i64::MAX)?;
    assert_eq!(
        serde_json::to_value(identity)?,
        json!({"type": "movie", "id": i64::MAX})
    );
    assert!(
        serde_json::from_str::<TitleIdentity>(r#"{"type":"movie","id":9223372036854775808}"#)
            .is_err()
    );
    Ok(())
}

#[test]
fn season_keys_include_specials_and_reject_invalid_coordinates() -> TestResult {
    assert_eq!(season_key(1399, 1)?, "tv:1399:1");
    assert_eq!(season_key(1399, 0)?, "tv:1399:0");
    assert!(season_key(0, 1).is_err());
    assert!(season_key(1399, -1).is_err());
    Ok(())
}

#[test]
fn genre_mapping_matches_go_vectors() {
    for genre in [
        "action",
        "comedy",
        "drama",
        "crime",
        "scifi",
        "animation",
        "documentary",
        "horror",
        "family",
    ] {
        assert!(valid_genre(genre));
    }
    for genre in ["western", "thriller", "news", "reality", "", "123"] {
        assert!(!valid_genre(genre));
    }
    assert_eq!(CANONICAL_GENRES.len(), 9);
}

#[test]
fn title_json_matches_existing_go_vector_exactly() -> TestResult {
    let title: Title = serde_json::from_value(title_json())?;
    title.validate()?;
    assert_eq!(title.identity.key(), "movie:550");
    assert_eq!(title.title, "Fight Club");
    assert_eq!(title.rating, 8.4);
    assert_eq!(serde_json::to_value(title)?, title_json());
    Ok(())
}

#[test]
fn title_retains_native_client_metadata_and_optional_omission() -> TestResult {
    let mut expected = title_json();
    expected["poster"] = json!("https://image.tmdb.org/t/p/w500/poster.jpg");
    expected["backdrop"] = json!("https://image.tmdb.org/t/p/w1280/backdrop.jpg");
    expected["date"] = json!("1999-10-15");
    expected["imdb_id"] = json!("tt0137523");
    expected["mal_id"] = json!(12345);
    expected["runtime"] = json!(139);
    let title: Title = serde_json::from_value(expected.clone())?;
    title.validate()?;
    assert_eq!(serde_json::to_value(title)?, expected);
    Ok(())
}

#[test]
fn null_go_slices_decode_without_losing_client_contract() -> TestResult {
    let mut value = title_json();
    value["genres"] = Value::Null;
    value["seasons"] = Value::Null;
    let title: Title = serde_json::from_value(value)?;
    assert!(title.genres.is_empty());
    assert!(title.seasons.is_empty());
    assert!(serde_json::to_value(title)?.get("genres").is_none());
    let inputs: RecommendationInputs = serde_json::from_value(json!({
        "completed": false, "format": "both", "genres": null,
        "favorites": null, "signals": null, "activity_days": null
    }))?;
    inputs.validate(time("2026-10-25T20:00:00Z")?)?;
    Ok(())
}

#[test]
fn nonfinite_and_negative_ratings_cannot_be_emitted_as_null() -> TestResult {
    let mut title: Title = serde_json::from_value(title_json())?;
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        title.rating = invalid;
        assert!(title.validate().is_err());
        assert!(serde_json::to_string(&title).is_err());
    }
    Ok(())
}

fn episode(season: i32, number: i32) -> Episode {
    Episode {
        tmdb_episode_id: i64::from(number),
        season,
        episode: number,
        name: "Special".into(),
        overview: String::new(),
        runtime: 30,
        still: String::new(),
        air_date: String::new(),
    }
}

#[test]
fn season_validation_handles_specials_and_rejects_cross_season_or_duplicate_episodes() -> TestResult
{
    let mut season = Season {
        show_id: 60625,
        season: 0,
        name: "Specials".into(),
        episodes: vec![episode(0, 1)],
    };
    season.validate()?;
    assert_eq!(season.key()?, "tv:60625:0");
    season.episodes.push(episode(0, 1));
    assert_eq!(
        season.validate(),
        Err(CatalogValidationError::Duplicate("episode"))
    );
    season.episodes = vec![episode(1, 1)];
    assert!(season.validate().is_err());
    season.episodes = vec![episode(0, 0)];
    assert!(season.validate().is_err());
    Ok(())
}

#[test]
fn episode_detail_validates_parent_identity() -> TestResult {
    EpisodeDetail {
        show_id: 60625,
        episode: episode(0, 1),
    }
    .validate()?;
    assert!(
        EpisodeDetail {
            show_id: 0,
            episode: episode(0, 1)
        }
        .validate()
        .is_err()
    );
    Ok(())
}

fn snapshot() -> Result<CatalogSnapshot, serde_json::Error> {
    serde_json::from_value(json!({
        "version": "1", "revision": 9, "exported_at": "2026-10-25T20:00:00Z",
        "records": [{
            "kind": "title", "key": "movie:550", "data": title_json(),
            "updated_at": "2026-10-01T00:00:00Z", "expires_at": "2026-10-02T00:00:00Z",
            "revision": 8, "state": "stale"
        }]
    }))
}

#[test]
fn stale_catalog_snapshot_remains_valid_and_retains_data_envelope() -> TestResult {
    let snapshot = snapshot()?;
    snapshot.validate()?;
    let round_trip: CatalogSnapshot = serde_json::from_value(serde_json::to_value(&snapshot)?)?;
    assert_eq!(round_trip, snapshot);
    let record: CatalogRecord<Title> =
        serde_json::from_value(serde_json::to_value(&snapshot.records[0])?)?;
    assert_eq!(record.data.identity.key(), "movie:550");
    assert_eq!(record.state, "stale");
    Ok(())
}

#[test]
fn snapshot_accepts_go_records_written_with_negative_ttl() -> TestResult {
    let mut snapshot = snapshot()?;
    snapshot.records[0].expires_at = time("2026-09-30T00:00:00Z")?;
    snapshot.validate()?;
    Ok(())
}

#[test]
fn snapshot_rejects_mixed_revisions_duplicates_and_unknown_versions() -> TestResult {
    let original = snapshot()?;
    let mut invalid = original.clone();
    invalid.version = "2".into();
    assert_eq!(
        invalid.validate(),
        Err(CatalogValidationError::UnsupportedSnapshotVersion)
    );
    invalid = original.clone();
    invalid.records[0].revision = 10;
    assert!(invalid.validate().is_err());
    invalid = original;
    invalid.records.push(invalid.records[0].clone());
    assert_eq!(
        invalid.validate(),
        Err(CatalogValidationError::Duplicate("record"))
    );
    Ok(())
}

#[test]
fn recommendation_input_accepts_rewatch_seconds_above_duration() -> TestResult {
    let inputs: RecommendationInputs = serde_json::from_value(recommendation_json())?;
    inputs.validate(time("2026-10-25T20:00:00Z")?)?;
    assert_eq!(serde_json::to_value(inputs)?, recommendation_json());
    Ok(())
}

#[test]
fn recommendation_identity_keeps_movie_and_tv_seeds_distinct() -> TestResult {
    let mut inputs: RecommendationInputs = serde_json::from_value(recommendation_json())?;
    let mut television = inputs.favorites[0].clone();
    television.identity = TitleIdentity::new(MediaType::Tv, television.identity.id())?;
    inputs.favorites.push(television);
    inputs.validate(time("2026-10-25T20:00:00Z")?)?;
    Ok(())
}

#[test]
fn recommendation_input_rejects_duplicates_limits_and_future_signals() -> TestResult {
    let now = time("2026-10-25T20:00:00Z")?;
    for (field, invalid) in [
        ("genres", json!(["drama", "drama"])),
        ("genres", json!(["western"])),
        ("activity_days", json!(["2026-10-24", "2026-10-24"])),
        ("activity_days", json!(["2026-10-26"])),
        ("activity_days", json!(["2026-09-23"])),
        ("activity_days", json!(["2026-02-30"])),
        ("activity_days", json!(["2026-1-01"])),
    ] {
        let mut value = recommendation_json();
        value[field] = invalid;
        let inputs: RecommendationInputs = serde_json::from_value(value)?;
        assert!(inputs.validate(now).is_err(), "accepted invalid {field}");
    }
    let mut inputs: RecommendationInputs = serde_json::from_value(recommendation_json())?;
    inputs.favorites.push(inputs.favorites[0].clone());
    assert!(inputs.validate(now).is_err());
    inputs.favorites.pop();
    inputs.signals[0].updated_at = time("2026-10-26T00:00:00Z")?;
    assert!(inputs.validate(now).is_err());
    inputs.signals[0].updated_at = time("0001-01-01T00:00:00Z")?;
    assert!(inputs.validate(now).is_err());
    Ok(())
}

#[test]
fn recommendation_input_rejects_nonfinite_negative_and_excessive_signals() -> TestResult {
    let now = time("2026-10-25T20:00:00Z")?;
    let mut inputs: RecommendationInputs = serde_json::from_value(recommendation_json())?;
    for invalid in [f64::NAN, f64::INFINITY, -1.0] {
        inputs.signals[0].watched_seconds = invalid;
        assert!(inputs.validate(now).is_err());
        assert!(serde_json::to_string(&inputs).is_err());
    }
    inputs.signals[0].watched_seconds = 10.0;
    inputs.signals[0].meaningful_episodes = 100_001;
    assert!(inputs.validate(now).is_err());
    inputs.signals[0].meaningful_episodes = 1;
    inputs.signals[0].title = "é".repeat(151); // Go bounds title bytes, not characters.
    assert!(inputs.validate(now).is_err());
    inputs.signals.clear();
    inputs.favorites = vec![inputs.favorites[0].clone(); 51];
    assert!(inputs.validate(now).is_err());
    Ok(())
}

#[test]
fn catalog_cross_ids_retain_zero_as_absent_and_reject_malformed_values() -> TestResult {
    let partial: CrossIds = serde_json::from_value(json!({"imdb_id": "tt0137523"}))?;
    assert_eq!(partial.mal_id, 0);
    partial.validate()?;
    CrossIds {
        imdb_id: String::new(),
        mal_id: 0,
    }
    .validate()?;
    CrossIds {
        imdb_id: "tt0137523".into(),
        mal_id: 123,
    }
    .validate()?;
    for imdb_id in ["tt", "0137523", "ttabc", " tt123", "tt1234567890123456789"] {
        assert!(
            CrossIds {
                imdb_id: imdb_id.into(),
                mal_id: 0
            }
            .validate()
            .is_err()
        );
    }
    assert!(
        CrossIds {
            imdb_id: String::new(),
            mal_id: -1
        }
        .validate()
        .is_err()
    );
    Ok(())
}
