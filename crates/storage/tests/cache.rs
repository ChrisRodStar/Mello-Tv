use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use semver::Version;
use stream_core::providers::ProviderId;
use stream_storage::cache::{
    ByteBoundCache, CacheError, CacheNamespace, CacheStats, InsertOutcome, NamespacedKey,
    source_cache_deadline,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn later(now: Instant, seconds: u64) -> Result<Instant, CacheError> {
    now.checked_add(Duration::from_secs(seconds))
        .ok_or(CacheError::DeadlineOverflow)
}

fn wall(value: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(value).map(|time| time.with_timezone(&Utc))
}

fn namespace(
    provider: &str,
    version: &str,
    context: &str,
) -> Result<CacheNamespace, Box<dyn std::error::Error>> {
    Ok(CacheNamespace::new(
        ProviderId::new(provider)?,
        Version::parse(version)?,
        context,
    )?)
}

#[test]
fn limits_and_accounted_weights_must_be_valid() -> TestResult {
    assert!(matches!(
        ByteBoundCache::<u32, u32>::new(0, 10, Duration::from_secs(10)),
        Err(CacheError::InvalidLimits)
    ));
    assert!(matches!(
        ByteBoundCache::<u32, u32>::new(10, 0, Duration::from_secs(10)),
        Err(CacheError::InvalidLimits)
    ));
    let cache = ByteBoundCache::new(2, 10, Duration::from_secs(10))?;
    assert_eq!(
        cache.insert(1, Arc::new(1), 0, None),
        Err(CacheError::InvalidWeight)
    );
    assert_eq!(
        cache.insert(1, Arc::new(1), 11, None),
        Err(CacheError::EntryTooLarge {
            bytes: 11,
            limit: 10
        })
    );
    assert_eq!(
        cache.stats()?,
        CacheStats {
            entries: 0,
            accounted_bytes: 0
        }
    );
    Ok(())
}

#[test]
fn values_share_the_same_allocation_without_serialization_or_clone() -> TestResult {
    struct NotClone(String);
    let now = Instant::now();
    let cache = ByteBoundCache::new(2, 64, Duration::from_secs(10))?;
    let value = Arc::new(NotClone("provider result".to_owned()));
    assert_eq!(
        cache.insert_at(1, Arc::clone(&value), 32, now, None)?,
        InsertOutcome::Inserted
    );
    let retained = cache.get_at(&1, now)?.ok_or("missing retained value")?;
    assert!(Arc::ptr_eq(&value, &retained));
    assert_eq!(retained.0, "provider result");
    cache.remove(&1)?;
    assert_eq!(retained.0, "provider result");
    assert_eq!(cache.stats_at(now)?.accounted_bytes, 0);
    Ok(())
}

#[test]
fn ttl_expiry_is_inclusive_and_does_not_refresh_on_reads() -> TestResult {
    let now = Instant::now();
    let cache = ByteBoundCache::new(2, 16, Duration::from_secs(10))?;
    cache.insert_at(1, Arc::new(123), 8, now, None)?;
    assert_eq!(cache.get_at(&1, later(now, 9)?)?.as_deref(), Some(&123));
    assert!(cache.get_at(&1, later(now, 10)?)?.is_none());
    assert_eq!(
        cache.stats_at(later(now, 10)?)?,
        CacheStats {
            entries: 0,
            accounted_bytes: 0
        }
    );
    Ok(())
}

#[test]
fn source_deadlines_only_shorten_ttl_and_elapsed_replacements_invalidate() -> TestResult {
    let now = Instant::now();
    let cache = ByteBoundCache::new(3, 30, Duration::from_secs(10))?;
    cache.insert_at(1, Arc::new(1), 10, now, Some(later(now, 3)?))?;
    cache.insert_at(2, Arc::new(2), 10, now, Some(later(now, 100)?))?;
    assert!(cache.get_at(&1, later(now, 3)?)?.is_none());
    assert_eq!(cache.get_at(&2, later(now, 9)?)?.as_deref(), Some(&2));
    assert!(cache.get_at(&2, later(now, 10)?)?.is_none());
    cache.insert_at(3, Arc::new(3), 10, later(now, 10)?, None)?;
    assert_eq!(
        cache.insert_at(3, Arc::new(4), 10, later(now, 10)?, Some(later(now, 10)?))?,
        InsertOutcome::NotRetained
    );
    assert!(cache.get_at(&3, later(now, 10)?)?.is_none());
    Ok(())
}

#[test]
fn zero_ttl_never_retains_values() -> TestResult {
    let now = Instant::now();
    let cache = ByteBoundCache::new(2, 10, Duration::ZERO)?;
    assert_eq!(
        cache.insert_at(1, Arc::new(1), 1, now, None)?,
        InsertOutcome::NotRetained
    );
    assert!(cache.get_at(&1, now)?.is_none());
    assert_eq!(cache.stats_at(now)?.entries, 0);
    Ok(())
}

#[test]
fn replacement_at_capacity_does_not_evict_unrelated_entries() -> TestResult {
    let now = Instant::now();
    let cache = ByteBoundCache::new(2, 10, Duration::from_secs(10))?;
    cache.insert_at(1, Arc::new(11), 4, now, None)?;
    cache.insert_at(2, Arc::new(22), 4, now, None)?;
    assert_eq!(
        cache.insert_at(1, Arc::new(33), 5, now, None)?,
        InsertOutcome::Replaced
    );
    assert_eq!(cache.get_at(&1, now)?.as_deref(), Some(&33));
    assert_eq!(cache.get_at(&2, now)?.as_deref(), Some(&22));
    assert_eq!(
        cache.stats_at(now)?,
        CacheStats {
            entries: 2,
            accounted_bytes: 9
        }
    );
    assert!(cache.insert_at(1, Arc::new(99), 11, now, None).is_err());
    assert_eq!(cache.get_at(&1, now)?.as_deref(), Some(&33));
    assert_eq!(cache.get_at(&2, now)?.as_deref(), Some(&22));
    Ok(())
}

#[test]
fn entry_and_byte_budgets_evict_the_actual_least_recently_used_value() -> TestResult {
    let now = Instant::now();
    let cache = ByteBoundCache::new(3, 10, Duration::from_secs(10))?;
    for key in [1, 2, 3] {
        cache.insert_at(key, Arc::new(key), 3, now, None)?;
    }
    cache.get_at(&1, now)?;
    // All operations use the same clock sample; the access sequence still
    // distinguishes the recently accessed key without relying on clock ticks.
    cache.insert_at(4, Arc::new(4), 4, now, None)?;
    assert!(cache.get_at(&2, now)?.is_none());
    assert!(cache.get_at(&1, now)?.is_some());
    assert!(cache.get_at(&3, now)?.is_some());
    assert_eq!(cache.stats_at(now)?.accounted_bytes, 10);
    cache.insert_at(5, Arc::new(5), 10, now, None)?;
    assert_eq!(
        cache.stats_at(now)?,
        CacheStats {
            entries: 1,
            accounted_bytes: 10
        }
    );
    Ok(())
}

#[test]
fn expired_entries_are_reclaimed_before_live_capacity_eviction() -> TestResult {
    let now = Instant::now();
    let cache = ByteBoundCache::new(2, 10, Duration::from_secs(10))?;
    cache.insert_at(1, Arc::new(1), 5, now, None)?;
    cache.insert_at(2, Arc::new(2), 5, now, Some(later(now, 1)?))?;
    cache.insert_at(3, Arc::new(3), 5, later(now, 1)?, None)?;
    assert!(cache.get_at(&1, later(now, 1)?)?.is_some());
    assert!(cache.get_at(&2, later(now, 1)?)?.is_none());
    assert_eq!(
        cache.stats_at(later(now, 1)?)?,
        CacheStats {
            entries: 2,
            accounted_bytes: 10
        }
    );
    cache.clear()?;
    assert_eq!(
        cache.stats_at(later(now, 1)?)?,
        CacheStats {
            entries: 0,
            accounted_bytes: 0
        }
    );
    Ok(())
}

#[test]
fn provider_versions_credentials_and_provider_ids_are_separate_namespaces() -> TestResult {
    let now = Instant::now();
    let cache = ByteBoundCache::new(8, 80, Duration::from_secs(10))?;
    let scopes = [
        namespace("vidcore", "1.0.0", "secret-user-a")?,
        namespace("vidcore", "1.0.1", "secret-user-a")?,
        namespace("vidcore", "1.0.0", "secret-user-b")?,
        namespace("videasy", "1.0.0", "secret-user-a")?,
    ];
    for (index, scope) in scopes.iter().enumerate() {
        cache.insert_at(
            NamespacedKey::new(scope.clone(), "movie:603"),
            Arc::new(index),
            10,
            now,
            None,
        )?;
    }
    for (index, scope) in scopes.iter().enumerate() {
        assert_eq!(
            cache
                .get_at(&NamespacedKey::new(scope.clone(), "movie:603"), now)?
                .as_deref(),
            Some(&index)
        );
    }
    assert_eq!(cache.invalidate_namespace(&scopes[0])?, 1);
    assert!(
        cache
            .get_at(&NamespacedKey::new(scopes[0].clone(), "movie:603"), now)?
            .is_none()
    );
    for scope in &scopes[1..] {
        assert!(
            cache
                .get_at(&NamespacedKey::new(scope.clone(), "movie:603"), now)?
                .is_some()
        );
    }
    let debug = format!(
        "{:?}",
        NamespacedKey::new(scopes[0].clone(), "sensitive-key")
    );
    assert!(!debug.contains("secret-user-a"));
    assert!(!debug.contains("sensitive-key"));
    assert_eq!(scopes[0].provider().as_str(), "vidcore");
    assert_eq!(scopes[0].version(), &Version::new(1, 0, 0));
    assert!(namespace("vidcore", "1.0.0", "").is_err());
    assert!(namespace("vidcore", "1.0.0", "bad\nkey").is_err());
    assert!(namespace("vidcore", "1.0.0", &"a".repeat(129)).is_err());
    Ok(())
}

#[test]
fn source_expiry_margin_uses_earliest_candidate_and_a_fixed_clock_sample() -> TestResult {
    let now = Instant::now();
    let wall_now = wall("2026-10-25T12:00:00Z")?;
    let ttl = Duration::from_secs(20);
    let sources = [
        None,
        Some(wall("2026-10-25T12:00:15Z")?),
        Some(wall("2026-10-25T12:00:10Z")?),
    ];
    assert_eq!(
        source_cache_deadline(now, wall_now, ttl, sources)?,
        Some(later(now, 5)?)
    );
    for expiry in [
        "2026-10-25T11:59:59Z",
        "2026-10-25T12:00:04Z",
        "2026-10-25T12:00:05Z",
    ] {
        assert!(source_cache_deadline(now, wall_now, ttl, [Some(wall(expiry)?)])?.is_none());
    }
    assert_eq!(
        source_cache_deadline(now, wall_now, ttl, [None])?,
        Some(later(now, 20)?)
    );
    assert_eq!(
        source_cache_deadline(now, wall_now, ttl, [Some(wall("0001-01-01T00:00:00Z")?)])?,
        Some(later(now, 20)?)
    );
    assert!(source_cache_deadline(now, wall_now, Duration::ZERO, [None])?.is_none());
    Ok(())
}

#[test]
fn overflowing_monotonic_deadlines_are_errors_unless_clamped() -> TestResult {
    let now = Instant::now();
    let cache = ByteBoundCache::new(1, 10, Duration::MAX)?;
    assert_eq!(
        cache.insert_at(1, Arc::new(1), 1, now, None),
        Err(CacheError::DeadlineOverflow)
    );
    cache.insert_at(1, Arc::new(1), 1, now, Some(later(now, 1)?))?;
    assert!(cache.get_at(&1, later(now, 1)?)?.is_none());
    assert_eq!(
        source_cache_deadline(now, wall("2026-10-25T12:00:00Z")?, Duration::MAX, [None]),
        Err(CacheError::DeadlineOverflow)
    );
    Ok(())
}

#[test]
fn concurrent_access_never_publishes_over_budget_state() -> TestResult {
    let now = Instant::now();
    let cache = ByteBoundCache::new(8, 80, Duration::from_secs(60))?;
    std::thread::scope(|scope| -> TestResult {
        let handles: Vec<_> = (0..8)
            .map(|worker| {
                let cache = &cache;
                scope.spawn(move || -> Result<(), CacheError> {
                    for offset in 0..100 {
                        let key = worker * 100 + offset;
                        cache.insert_at(key, Arc::new(key), 10, now, None)?;
                        cache.get_at(&key, now)?;
                        let stats = cache.stats_at(now)?;
                        assert!(stats.entries <= 8);
                        assert!(stats.accounted_bytes <= 80);
                        if offset % 3 == 0 {
                            cache.remove(&key)?;
                        }
                    }
                    Ok(())
                })
            })
            .collect();
        for handle in handles {
            handle.join().map_err(|_| "cache worker panicked")??;
        }
        Ok(())
    })?;
    let stats = cache.stats_at(now)?;
    assert!(stats.entries <= 8);
    assert!(stats.accounted_bytes <= 80);
    Ok(())
}
