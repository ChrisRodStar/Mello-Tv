use std::time::SystemTime;
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use stream_storage::catalog::{CatalogStore, CatalogWrite};

pub async fn seed_if_empty(catalog: &CatalogStore) -> Result<(), Box<dyn std::error::Error>> {
    let now: DateTime<Utc> = SystemTime::now().into();
    let existing = catalog.list("shelf", 1, now).await?;
    if !existing.is_empty() {
        return Ok(());
    }

    tracing::info!("catalog is empty, seeding onboarding shelves, popular titles, and curated shelves");

    let infinity_war = json!({
        "type": "movie",
        "id": 299536,
        "title": "Avengers: Infinity War",
        "overview": "As the Avengers and their allies have continued to protect the world from threats too large for any one hero to handle, a new danger has emerged from the cosmic shadows: Thanos.",
        "date": "2018-04-25",
        "rating": 8.3,
        "poster": "https://image.tmdb.org/t/p/w500/7WsyChQLEftFiDOVTGkv3hFpyyt.jpg",
        "backdrop": "https://image.tmdb.org/t/p/w1280/lmZFxXgJE3vgrciwuDib0N8CfQo.jpg",
        "imdb_id": "tt4154756",
        "runtime": 149,
        "genres": ["Action", "Adventure", "Science Fiction"],
        "details": true
    });

    let barbie = json!({
        "type": "movie",
        "id": 346698,
        "title": "Barbie",
        "overview": "To live in Barbie Land is to be a perfect being in a perfect place. Unless you have a full-on existential crisis. Or you're a Ken.",
        "date": "2023-07-19",
        "rating": 7.1,
        "poster": "https://image.tmdb.org/t/p/w500/iuFNMS8U5cb6xfzi51Dbkovj7vM.jpg",
        "backdrop": "https://image.tmdb.org/t/p/w1280/nHf61UzkfFno5X1ofIhugCPus2R.jpg",
        "imdb_id": "tt1517268",
        "runtime": 114,
        "genres": ["Comedy", "Adventure", "Fantasy"],
        "details": true
    });

    let shawshank = json!({
        "type": "movie",
        "id": 278,
        "title": "The Shawshank Redemption",
        "overview": "Imprisoned in the 1940s for the double murder of his wife and her lover, upstanding banker Andy Dufresne begins a new life at the Shawshank prison.",
        "date": "1994-09-23",
        "rating": 8.7,
        "poster": "https://image.tmdb.org/t/p/w500/9cqNxx0GxF0bflZmeSMuL5tnGzr.jpg",
        "backdrop": "https://image.tmdb.org/t/p/w1280/tSPT36ZKlP2WVHJLM4cQPLSzv3b.jpg",
        "imdb_id": "tt0111161",
        "runtime": 142,
        "genres": ["Drama", "Crime"],
        "details": true
    });

    let the_batman = json!({
        "type": "movie",
        "id": 414906,
        "title": "The Batman",
        "overview": "In his second year of fighting crime, Batman uncovers corruption in Gotham City that connects to his own family while facing a serial killer known as the Riddler.",
        "date": "2022-03-01",
        "rating": 7.7,
        "poster": "https://image.tmdb.org/t/p/w500/74xTEgt7R36Fpooo50r9T25onhq.jpg",
        "backdrop": "https://image.tmdb.org/t/p/w1280/rvtdN5XkWAfGX6xDuPL6yYS2seK.jpg",
        "imdb_id": "tt1877830",
        "runtime": 176,
        "genres": ["Crime", "Mystery", "Action"],
        "details": true
    });

    let inception = json!({
        "type": "movie",
        "id": 27205,
        "title": "Inception",
        "overview": "Cobb, a skilled thief who commits corporate espionage by infiltrating the subconscious of his targets is offered a chance to regain his old life.",
        "date": "2010-07-15",
        "rating": 8.4,
        "poster": "https://image.tmdb.org/t/p/w500/edv5CZvWj09upOsy2Y6IwDhK8bt.jpg",
        "backdrop": "https://image.tmdb.org/t/p/w1280/8ZTVqvKDQ8emSGUEMjsS4yHAwrp.jpg",
        "imdb_id": "tt1375666",
        "runtime": 148,
        "genres": ["Action", "Science Fiction", "Adventure"],
        "details": true
    });

    let spirited_away = json!({
        "type": "movie",
        "id": 129,
        "title": "Spirited Away",
        "overview": "A young girl, Chihiro, becomes trapped in a strange new world of spirits. When her parents undergo a mysterious transformation, she must call upon the courage she never knew she had to free her family.",
        "date": "2001-07-20",
        "rating": 8.5,
        "poster": "https://image.tmdb.org/t/p/w500/39wmItIWsg5sZMyRUHLkWBcuVCM.jpg",
        "backdrop": "https://image.tmdb.org/t/p/w1280/dyJvKsNs2KP8qQnAXbRwDjblViy.jpg",
        "imdb_id": "tt0245429",
        "runtime": 125,
        "genres": ["Animation", "Family", "Fantasy"],
        "details": true
    });

    let free_solo = json!({
        "type": "movie",
        "id": 515042,
        "title": "Free Solo",
        "overview": "Follow Alex Honnold as he attempts to become the first person to ever free solo climb the 3,000 foot El Capitan in Yosemite National Park.",
        "date": "2018-09-28",
        "rating": 8.0,
        "poster": "https://image.tmdb.org/t/p/w500/z2uuQasY4gQJ8VDAFki746JWeQJ.jpg",
        "backdrop": "https://image.tmdb.org/t/p/w1280/z2uuQasY4gQJ8VDAFki746JWeQJ.jpg",
        "imdb_id": "tt7775622",
        "runtime": 100,
        "genres": ["Documentary", "Adventure"],
        "details": true
    });

    let the_conjuring = json!({
        "type": "movie",
        "id": 138843,
        "title": "The Conjuring",
        "overview": "Paranormal investigators Ed and Lorraine Warren work to help a family terrorized by a dark presence in their farmhouse.",
        "date": "2013-07-18",
        "rating": 7.5,
        "poster": "https://image.tmdb.org/t/p/w500/wVYREutTvI2tmxr6ujrHT704wGF.jpg",
        "backdrop": "https://image.tmdb.org/t/p/w1280/ecKQlAEG95k62SMGhvX83oEqANK.jpg",
        "imdb_id": "tt1457767",
        "runtime": 112,
        "genres": ["Horror", "Thriller"],
        "details": true
    });

    let breaking_bad = json!({
        "type": "tv",
        "id": 1396,
        "title": "Breaking Bad",
        "overview": "Walter White, a New Mexico chemistry teacher, is diagnosed with Stage III cancer and given a prognosis of two years left to live. He chooses to enter a dangerous world of drugs and crime.",
        "date": "2008-01-20",
        "rating": 8.9,
        "poster": "https://image.tmdb.org/t/p/w500/ztkUQFLlC19CCMYHW9o1zWhJRNq.jpg",
        "backdrop": "https://image.tmdb.org/t/p/w1280/tsRy63Mu5cu8etL1X7ZLyf7UP1M.jpg",
        "imdb_id": "tt0903747",
        "runtime": 47,
        "genres": ["Drama", "Crime"],
        "details": true,
        "seasons": [
            { "season": 1, "name": "Season 1", "episode_count": 7 }
        ]
    });

    let stranger_things = json!({
        "type": "tv",
        "id": 66732,
        "title": "Stranger Things",
        "overview": "When a young boy vanishes, a small town uncovers a mystery involving secret experiments, terrifying supernatural forces and one strange little girl.",
        "date": "2016-07-15",
        "rating": 8.6,
        "poster": "https://image.tmdb.org/t/p/w500/49WJfeN0moxb9IPfGn8AIqMGskD.jpg",
        "backdrop": "https://image.tmdb.org/t/p/w1280/56v2KjBlU4XaOv9rVYEQypROD7P.jpg",
        "imdb_id": "tt4574334",
        "runtime": 50,
        "genres": ["Sci-Fi & Fantasy", "Drama", "Mystery"],
        "details": true,
        "seasons": [
            { "season": 1, "name": "Season 1", "episode_count": 8 }
        ]
    });

    let all_titles = [
        infinity_war.clone(),
        barbie.clone(),
        shawshank.clone(),
        the_batman.clone(),
        inception.clone(),
        spirited_away.clone(),
        free_solo.clone(),
        the_conjuring.clone(),
        breaking_bad.clone(),
        stranger_things.clone(),
    ];

    let mut writes = Vec::new();
    let ttl = Duration::days(365);

    // 1. Seed Onboarding Shelves
    let onboarding_specs: Vec<(&str, &str, Value)> = vec![
        ("onboarding-action", "Action & adventure", infinity_war.clone()),
        ("onboarding-comedy", "Comedy", barbie.clone()),
        ("onboarding-drama", "Drama", shawshank.clone()),
        ("onboarding-crime", "Crime & mystery", the_batman.clone()),
        ("onboarding-scifi", "Sci-fi & fantasy", inception.clone()),
        ("onboarding-animation", "Anime", spirited_away.clone()),
        ("onboarding-documentary", "Documentaries", free_solo.clone()),
        ("onboarding-horror", "Horror", the_conjuring.clone()),
    ];

    for (shelf_id, heading, title_obj) in onboarding_specs {
        writes.push(CatalogWrite {
            kind: "shelf".to_string(),
            key: shelf_id.to_string(),
            data: json!({
                "id": shelf_id,
                "heading": heading,
                "titles": [title_obj]
            }),
            ttl,
        });
    }

    // 2. Seed Curated Home Shelves
    let trending_titles = all_titles.clone();
    writes.push(CatalogWrite {
        kind: "shelf".to_string(),
        key: "trending".to_string(),
        data: json!({
            "id": "trending",
            "heading": "Trending this week",
            "titles": trending_titles
        }),
        ttl,
    });

    writes.push(CatalogWrite {
        kind: "shelf".to_string(),
        key: "movie-popular".to_string(),
        data: json!({
            "id": "movie-popular",
            "heading": "Popular movies",
            "titles": [
                infinity_war.clone(),
                barbie.clone(),
                shawshank.clone(),
                the_batman.clone(),
                inception.clone(),
                spirited_away.clone(),
            ]
        }),
        ttl,
    });

    writes.push(CatalogWrite {
        kind: "shelf".to_string(),
        key: "tv-popular".to_string(),
        data: json!({
            "id": "tv-popular",
            "heading": "Popular TV shows",
            "titles": [
                breaking_bad.clone(),
                stranger_things.clone(),
            ]
        }),
        ttl,
    });

    // 3. Seed individual Title records
    for t in &all_titles {
        let media_type = t.get("type").and_then(|v| v.as_str()).unwrap_or("movie");
        let id = t.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        writes.push(CatalogWrite {
            kind: "title".to_string(),
            key: format!("{media_type}:{id}"),
            data: t.clone(),
            ttl,
        });
    }

    // 4. Seed Season 1 for Breaking Bad
    writes.push(CatalogWrite {
        kind: "season".to_string(),
        key: "tv:1396:1".to_string(),
        data: json!({
            "show_id": 1396,
            "season": 1,
            "name": "Season 1",
            "episodes": [
                {
                    "tmdb_episode_id": 62085,
                    "season": 1,
                    "episode": 1,
                    "name": "Pilot",
                    "overview": "When an unassuming high school chemistry teacher discovers he has a rare form of lung cancer, he decides to team up with an unruly former student to create and sell the purest methamphetamine he can.",
                    "runtime": 58,
                    "air_date": "2008-01-20"
                },
                {
                    "tmdb_episode_id": 62086,
                    "season": 1,
                    "episode": 2,
                    "name": "Cat's in the Bag...",
                    "overview": "Walt and Jesse attempt to tie up loose ends. The desperate situation gets more complicated with the disposal of the bodies and the arrival of an unannounced guest.",
                    "runtime": 48,
                    "air_date": "2008-01-27"
                }
            ]
        }),
        ttl,
    });

    catalog.put_batch(writes, now).await?;
    tracing::info!("catalog successfully seeded with initial shelves and titles");

    Ok(())
}
