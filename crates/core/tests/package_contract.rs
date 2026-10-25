use std::error::Error;

use semver::Version;
use stream_core::packages::{AllowedOrigin, ArtifactDigest, PackageError, ProviderManifest};

fn manifest() -> Result<ProviderManifest, Box<dyn Error>> {
    Ok(serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "provider_id": "vidcore",
        "version": "2.1.0",
        "host_version": ">=0.1.0, <0.2.0",
        "interface": {"major": 1, "minor": 0},
        "artifact_sha256": "a".repeat(64),
        "artifact_bytes": 4096,
        "allowed_origins": ["https://example.com"],
        "limits": {
            "memory_bytes": 16777216,
            "max_http_requests": 16,
            "max_response_bytes": 1048576,
            "timeout_ms": 18000
        }
    }))?)
}

#[test]
fn independent_package_version_and_host_compatibility() -> Result<(), Box<dyn Error>> {
    let package = manifest()?;
    package.validate_for_host(&Version::parse("0.1.0")?)?;
    assert_eq!(package.version, Version::parse("2.1.0")?);
    assert_eq!(
        package.validate_for_host(&Version::parse("0.2.0")?),
        Err(PackageError::IncompatibleHost)
    );
    Ok(())
}

#[test]
fn normalizes_origins_and_rejects_ambient_authority() -> Result<(), Box<dyn Error>> {
    assert_eq!(
        AllowedOrigin::new("https://EXAMPLE.com:443/")?.as_str(),
        "https://example.com"
    );
    for value in [
        "https://user:password@example.com",
        "https://example.com/private",
        "https://example.com?token=secret",
        "https://example.com/#fragment",
        "file:///etc/passwd",
        "*.example.com",
        "https://example.com\\",
        " https://example.com",
    ] {
        assert!(AllowedOrigin::new(value).is_err());
    }
    let mut package = manifest()?;
    package
        .allowed_origins
        .push(AllowedOrigin::new("https://EXAMPLE.com:443/")?);
    assert_eq!(
        package.validate_for_host(&Version::new(0, 1, 0)),
        Err(PackageError::DuplicateOrigin)
    );
    Ok(())
}

#[test]
fn rejects_unknown_interfaces_and_unbounded_resource_requests() -> Result<(), Box<dyn Error>> {
    let host = Version::new(0, 1, 0);
    let mut package = manifest()?;
    package.interface.minor = 1;
    assert_eq!(
        package.validate_for_host(&host),
        Err(PackageError::IncompatibleInterface)
    );
    package.interface.minor = 0;
    package.limits.memory_bytes = u64::MAX;
    assert_eq!(
        package.validate_for_host(&host),
        Err(PackageError::ResourceLimit)
    );
    package.limits.memory_bytes = 16 * 1024 * 1024;
    package.artifact_bytes = 0;
    assert_eq!(
        package.validate_for_host(&host),
        Err(PackageError::ArtifactSize)
    );
    Ok(())
}

#[test]
fn invalid_digests_fail_during_deserialization() {
    for value in ["", "abc", &"A".repeat(64), &"g".repeat(64)] {
        assert!(ArtifactDigest::new(value).is_err());
        assert!(serde_json::from_value::<ArtifactDigest>(serde_json::json!(value)).is_err());
    }
}
