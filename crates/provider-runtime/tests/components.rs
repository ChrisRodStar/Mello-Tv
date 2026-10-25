use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use semver::Version;
use sha2::{Digest, Sha256};
use stream_core::{
    packages::{AllowedOrigin, ArtifactDigest, InterfaceVersion, ProviderManifest, ResourceLimits},
    providers::{Lookup, ProviderId},
};
use stream_provider_runtime::{
    ProviderRuntime, RuntimeConfig, RuntimeError,
    http::{HttpError, HttpPolicy, HttpRequest, HttpResponse, HttpTransport},
};
use tokio_util::sync::CancellationToken;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct NoHttp;
#[async_trait]
impl HttpTransport for NoHttp {
    async fn fetch(
        &self,
        _: HttpRequest,
        _: HttpPolicy,
        _: CancellationToken,
    ) -> Result<HttpResponse, HttpError> {
        Err(HttpError::Denied)
    }
}

fn runtime() -> Result<ProviderRuntime, RuntimeError> {
    ProviderRuntime::new(
        Version::new(0, 1, 0),
        Arc::new(NoHttp),
        RuntimeConfig::default(),
    )
}

fn lookup() -> Result<Lookup, Box<dyn std::error::Error>> {
    let mut lookup: Lookup = serde_json::from_str(
        r#"{"title":{"type":"movie","tmdb_id":603},"device":{"formats":["hls"]}}"#,
    )?;
    lookup.context_key = "profile-session".into();
    Ok(lookup)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut result = String::new();
    for byte in bytes {
        let _ = write!(&mut result, "{byte:02x}");
    }
    result
}

fn escaped(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut result = String::new();
    for byte in bytes {
        let _ = write!(&mut result, "\\{byte:02x}");
    }
    result
}

fn component(
    output: &str,
    body: &str,
    imports: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut returned = vec![0u8; 4];
    returned.extend_from_slice(&64u32.to_le_bytes());
    returned.extend_from_slice(&u32::try_from(output.len())?.to_le_bytes());
    Ok(wat::parse_str(format!(
        r#"
        (component
            {imports}
            (core module $guest
                (memory (export "memory") 1)
                (data (i32.const 16) "{}")
                (data (i32.const 64) "{}")
                (func (export "realloc") (param i32 i32 i32 i32) (result i32) (i32.const 4096))
                (func (export "resolve") (param i32 i32) (result i32) {body} (i32.const 16))
            )
            (core instance $guest-instance (instantiate $guest))
            (type $resolve-type (func (param "lookup-json" string) (result (result string (error string)))))
            (func (export "resolve") (type $resolve-type)
                (canon lift (core func $guest-instance "resolve")
                    (memory (core memory $guest-instance "memory")) (realloc (core func $guest-instance "realloc"))))
        )
    "#,
        escaped(&returned),
        escaped(output.as_bytes())
    ))?)
}

fn manifest(
    bytes: &[u8],
    version: Version,
) -> Result<ProviderManifest, Box<dyn std::error::Error>> {
    Ok(ProviderManifest {
        schema_version: 1,
        provider_id: ProviderId::new("fixture")?,
        version,
        host_version: ">=0.1.0, <1.0.0".parse()?,
        interface: InterfaceVersion { major: 1, minor: 0 },
        artifact_sha256: ArtifactDigest::new(hex(&Sha256::digest(bytes)))?,
        artifact_bytes: bytes.len() as u64,
        allowed_origins: vec![AllowedOrigin::new("https://fixture.example")?],
        limits: ResourceLimits {
            memory_bytes: 65_536,
            max_http_requests: 4,
            max_response_bytes: 4096,
            timeout_ms: 200,
        },
    })
}

const EMPTY_RESULT: &str = r#"{"candidates":[],"resolver_duration":123}"#;

#[tokio::test]
async fn real_rust_component_delegates_through_generated_abi_to_safe_implementation() -> TestResult
{
    let runtime = runtime()?;
    let bytes = include_bytes!("fixtures/guest/component.wasm").to_vec();
    let mut package_manifest = manifest(&bytes, Version::new(1, 0, 0))?;
    package_manifest.limits.memory_bytes = 4 * 1024 * 1024;
    let handle = runtime.activate(runtime.stage(package_manifest, bytes).await?)?;
    let result = runtime
        .resolve_handle(&handle, &lookup()?, CancellationToken::new())
        .await?;
    assert_eq!(result.resolver_duration, 789);
    assert_eq!(result.candidates.as_ref().map(Vec::len), Some(0));
    Ok(())
}

fn http_component(
    url: &str,
    method: &str,
    calls: usize,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut returned = vec![0u8; 4];
    returned.extend_from_slice(&64u32.to_le_bytes());
    returned.extend_from_slice(&u32::try_from(EMPTY_RESULT.len())?.to_le_bytes());
    let call = format!(
        "(call $fetch (i32.const 256) (i32.const {}) (i32.const 512) (i32.const {}) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 2048) (i32.const 0) (i32.const 1024))",
        url.len(),
        method.len()
    );
    Ok(wat::parse_str(format!(
        r#"
    (component
        (import "stream-tv:provider/http@1.0.0" (instance $http
            (type $header (record (field "name" string) (field "value" string)))
            (export "header" (type $header-export (eq $header)))
            (type $request (record (field "url" string) (field "method" string) (field "headers" (list $header-export)) (field "body" (list u8)) (field "max-response-bytes" u32) (field "timeout-ms" u32)))
            (export "request" (type $request-export (eq $request)))
            (type $response (record (field "url" string) (field "status" u16) (field "headers" (list $header-export)) (field "body" (list u8))))
            (export "response" (type $response-export (eq $response)))
            (type $failure (enum "invalid-request" "denied" "limit-exceeded" "cancelled" "timeout" "upstream-failure"))
            (export "failure" (type $failure-export (eq $failure)))
            (export "fetch" (func (param "input" $request-export) (result (result $response-export (error $failure-export)))))
        ))
        (alias export $http "fetch" (func $fetch-component))
        (core module $memory-module
            (memory (export "memory") 1)
            (global $heap (mut i32) (i32.const 4096))
            (func (export "realloc") (param i32 i32 i32 i32) (result i32) (local $ptr i32)
                (local.set $ptr (i32.and (i32.add (global.get $heap) (i32.const 3)) (i32.const -4)))
                (global.set $heap (i32.add (local.get $ptr) (local.get 3))) (local.get $ptr))
        )
        (core instance $memory-instance (instantiate $memory-module))
        (alias core export $memory-instance "memory" (core memory $memory))
        (alias core export $memory-instance "realloc" (core func $realloc))
        (core func $fetch-lowered (canon lower (func $fetch-component) (memory $memory) (realloc $realloc)))
        (core instance $host (export "memory" (memory $memory)) (export "fetch" (func $fetch-lowered)))
        (core module $guest
            (import "host" "memory" (memory 1))
            (import "host" "fetch" (func $fetch (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)))
            (data (i32.const 16) "{}") (data (i32.const 64) "{}")
            (data (i32.const 256) "{}") (data (i32.const 512) "{}")
            (func (export "resolve") (param i32 i32) (result i32) {} (i32.const 16))
        )
        (core instance $guest-instance (instantiate $guest (with "host" (instance $host))))
        (type $resolve-type (func (param "lookup-json" string) (result (result string (error string)))))
        (func (export "resolve") (type $resolve-type) (canon lift (core func $guest-instance "resolve") (memory $memory) (realloc $realloc)))
    )"#,
        escaped(&returned),
        escaped(EMPTY_RESULT.as_bytes()),
        escaped(url.as_bytes()),
        escaped(method.as_bytes()),
        call.repeat(calls)
    ))?)
}

struct CountingHttp {
    calls: AtomicUsize,
    dropped: Arc<AtomicUsize>,
    block: bool,
}
struct Dropped(Arc<AtomicUsize>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[async_trait]
impl HttpTransport for CountingHttp {
    async fn fetch(
        &self,
        request: HttpRequest,
        policy: HttpPolicy,
        _: CancellationToken,
    ) -> Result<HttpResponse, HttpError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let _drop = Dropped(self.dropped.clone());
        assert_eq!(&*policy.context_key, "profile-session");
        if self.block {
            return std::future::pending().await;
        }
        Ok(HttpResponse {
            url: request.url,
            status: 200,
            headers: Vec::new(),
            body: b"ok".to_vec(),
        })
    }
}

#[tokio::test]
async fn typed_component_http_import_checks_origins_and_request_budget() -> TestResult {
    let transport = Arc::new(CountingHttp {
        calls: AtomicUsize::new(0),
        dropped: Arc::new(AtomicUsize::new(0)),
        block: false,
    });
    let runtime = ProviderRuntime::new(
        Version::new(0, 1, 0),
        transport.clone(),
        RuntimeConfig::default(),
    )?;
    let bytes = http_component("https://fixture.example/resolve", "GET", 3)?;
    let mut package_manifest = manifest(&bytes, Version::new(1, 0, 0))?;
    package_manifest.limits.max_http_requests = 2;
    let handle = runtime.activate(runtime.stage(package_manifest, bytes).await?)?;
    runtime
        .resolve_handle(&handle, &lookup()?, CancellationToken::new())
        .await?;
    assert_eq!(transport.calls.load(Ordering::SeqCst), 2);
    let bytes = http_component("https://unapproved.example/resolve", "GET", 1)?;
    let handle = runtime.activate(
        runtime
            .stage(manifest(&bytes, Version::new(1, 1, 0))?, bytes)
            .await?,
    )?;
    runtime
        .resolve_handle(&handle, &lookup()?, CancellationToken::new())
        .await?;
    assert_eq!(transport.calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn blocked_host_http_is_dropped_when_component_deadline_expires() -> TestResult {
    let dropped = Arc::new(AtomicUsize::new(0));
    let transport = Arc::new(CountingHttp {
        calls: AtomicUsize::new(0),
        dropped: dropped.clone(),
        block: true,
    });
    let runtime = ProviderRuntime::new(
        Version::new(0, 1, 0),
        transport.clone(),
        RuntimeConfig::default(),
    )?;
    let bytes = http_component("https://fixture.example/resolve", "GET", 1)?;
    let mut package_manifest = manifest(&bytes, Version::new(1, 0, 0))?;
    package_manifest.limits.timeout_ms = 30;
    let handle = runtime.activate(runtime.stage(package_manifest, bytes).await?)?;
    let result = runtime
        .resolve_handle(&handle, &lookup()?, CancellationToken::new())
        .await;
    assert!(matches!(result, Err(RuntimeError::Timeout)));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn real_component_executes_validated_contract_without_ambient_access() -> TestResult {
    let runtime = runtime()?;
    let bytes = component(EMPTY_RESULT, "", "")?;
    let staged = runtime
        .stage(manifest(&bytes, Version::new(1, 0, 0))?, bytes)
        .await?;
    let handle = runtime.activate(staged)?;
    let result = runtime
        .resolve_handle(&handle, &lookup()?, CancellationToken::new())
        .await?;
    assert_eq!(result.resolver_duration, 123);
    assert_eq!(result.candidates.as_ref().map(Vec::len), Some(0));
    assert_eq!(handle.generation(), 1);
    Ok(())
}

#[tokio::test]
async fn activation_pins_old_generation_and_explicit_recovery_keeps_watermark() -> TestResult {
    let runtime = runtime()?;
    let bytes = component(EMPTY_RESULT, "", "")?;
    let first = runtime.activate(
        runtime
            .stage(manifest(&bytes, Version::new(1, 0, 0))?, bytes.clone())
            .await?,
    )?;
    let provider = first.manifest().provider_id.clone();
    let second_bytes = component(r#"{"candidates":[],"resolver_duration":456}"#, "", "")?;
    let second = runtime.activate(
        runtime
            .stage(
                manifest(&second_bytes, Version::new(1, 1, 0))?,
                second_bytes,
            )
            .await?,
    )?;
    assert_eq!(second.generation(), 2);
    assert_eq!(
        runtime
            .resolve_handle(&first, &lookup()?, CancellationToken::new())
            .await?
            .resolver_duration,
        123
    );
    assert_eq!(
        runtime
            .resolve(&provider, &lookup()?, CancellationToken::new())
            .await?
            .resolver_duration,
        456
    );
    let previous = runtime.recover_previous(&provider)?;
    assert_eq!(previous.generation(), 3);
    assert_eq!(
        runtime
            .resolve(&provider, &lookup()?, CancellationToken::new())
            .await?
            .resolver_duration,
        123
    );
    let replay = runtime
        .stage(manifest(&bytes, Version::new(1, 1, 0))?, bytes)
        .await?;
    assert!(matches!(
        runtime.activate(replay),
        Err(RuntimeError::VersionConflict)
    ));
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn epoch_interruption_bounds_an_infinite_cpu_loop_on_single_thread_executor() -> TestResult {
    let runtime = runtime()?;
    let bytes = component(EMPTY_RESULT, "(loop $forever (br $forever))", "")?;
    let mut manifest = manifest(&bytes, Version::new(1, 0, 0))?;
    manifest.limits.timeout_ms = 30;
    let handle = runtime.activate(runtime.stage(manifest, bytes).await?)?;
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        runtime.resolve_handle(&handle, &lookup()?, CancellationToken::new()),
    )
    .await?;
    assert!(matches!(result, Err(RuntimeError::Timeout)));
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_interrupts_running_cpu_work_and_releases_admission() -> TestResult {
    let runtime = runtime()?;
    let bytes = component(EMPTY_RESULT, "(loop $forever (br $forever))", "")?;
    let mut package_manifest = manifest(&bytes, Version::new(1, 0, 0))?;
    package_manifest.limits.timeout_ms = 2000;
    let handle = runtime.activate(runtime.stage(package_manifest, bytes).await?)?;
    let cancellation = CancellationToken::new();
    let cancel = cancellation.clone();
    let input = lookup()?;
    let (result, ()) = tokio::join!(
        runtime.resolve_handle(&handle, &input, cancellation),
        async move {
            tokio::time::sleep(Duration::from_millis(15)).await;
            cancel.cancel();
        }
    );
    assert!(matches!(result, Err(RuntimeError::Cancelled)));
    let bytes = component(EMPTY_RESULT, "", "")?;
    let next = runtime.activate(
        runtime
            .stage(manifest(&bytes, Version::new(1, 1, 0))?, bytes)
            .await?,
    )?;
    runtime
        .resolve_handle(&next, &lookup()?, CancellationToken::new())
        .await?;
    Ok(())
}

#[tokio::test]
async fn memory_growth_is_trapped_and_invalid_result_cannot_escape() -> TestResult {
    let runtime = runtime()?;
    let bytes = component(EMPTY_RESULT, "(drop (memory.grow (i32.const 1)))", "")?;
    let handle = runtime.activate(
        runtime
            .stage(manifest(&bytes, Version::new(1, 0, 0))?, bytes)
            .await?,
    )?;
    assert!(matches!(
        runtime
            .resolve_handle(&handle, &lookup()?, CancellationToken::new())
            .await,
        Err(RuntimeError::Component(_))
    ));
    let bytes = component(r#"{"candidates":[],"resolver_duration":-1}"#, "", "")?;
    let handle = runtime.activate(
        runtime
            .stage(manifest(&bytes, Version::new(1, 1, 0))?, bytes)
            .await?,
    )?;
    assert!(matches!(
        runtime
            .resolve_handle(&handle, &lookup()?, CancellationToken::new())
            .await,
        Err(RuntimeError::Contract)
    ));
    Ok(())
}

#[tokio::test]
async fn incompatible_packages_and_unknown_imports_leave_active_version_intact() -> TestResult {
    let runtime = runtime()?;
    let bytes = component(EMPTY_RESULT, "", "")?;
    let handle = runtime.activate(
        runtime
            .stage(manifest(&bytes, Version::new(1, 0, 0))?, bytes.clone())
            .await?,
    )?;
    let provider = handle.manifest().provider_id.clone();
    let mut incompatible = manifest(&bytes, Version::new(1, 1, 0))?;
    incompatible.interface.major = 2;
    assert!(matches!(
        runtime.stage(incompatible, bytes.clone()).await,
        Err(RuntimeError::Manifest(_))
    ));
    let unknown = component(
        EMPTY_RESULT,
        "",
        r#"(import "wasi:cli/environment@0.2.0" (instance))"#,
    )?;
    assert!(matches!(
        runtime
            .stage(manifest(&unknown, Version::new(1, 1, 0))?, unknown)
            .await,
        Err(RuntimeError::UnsupportedImport)
    ));
    let mut tampered = bytes.clone();
    tampered.push(0);
    assert!(matches!(
        runtime
            .stage(manifest(&bytes, Version::new(1, 1, 0))?, tampered)
            .await,
        Err(RuntimeError::ArtifactMismatch)
    ));
    let mut same_length_tamper = bytes.clone();
    *same_length_tamper.first_mut().ok_or("empty fixture")? ^= 1;
    assert!(matches!(
        runtime
            .stage(manifest(&bytes, Version::new(1, 1, 0))?, same_length_tamper)
            .await,
        Err(RuntimeError::ArtifactMismatch)
    ));
    assert_eq!(runtime.snapshot(&provider)?.generation(), 1);
    assert_eq!(
        runtime
            .resolve(&provider, &lookup()?, CancellationToken::new())
            .await?
            .resolver_duration,
        123
    );
    Ok(())
}

struct GatedHttp {
    started: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
#[async_trait]
impl HttpTransport for GatedHttp {
    async fn fetch(
        &self,
        request: HttpRequest,
        _: HttpPolicy,
        _: CancellationToken,
    ) -> Result<HttpResponse, HttpError> {
        self.started.notify_one();
        self.release.notified().await;
        Ok(HttpResponse {
            url: request.url,
            status: 200,
            headers: Vec::new(),
            body: b"ok".to_vec(),
        })
    }
}

#[tokio::test]
async fn activation_during_running_host_call_keeps_old_lookup_generation_alive() -> TestResult {
    let transport = Arc::new(GatedHttp {
        started: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let runtime = ProviderRuntime::new(
        Version::new(0, 1, 0),
        transport.clone(),
        RuntimeConfig::default(),
    )?;
    let bytes = http_component("https://fixture.example/resolve", "GET", 1)?;
    let mut first_manifest = manifest(&bytes, Version::new(1, 0, 0))?;
    first_manifest.limits.timeout_ms = 5000;
    let first = runtime.activate(runtime.stage(first_manifest, bytes).await?)?;
    let input = lookup()?;
    let (old_result, updated) = tokio::join!(
        runtime.resolve_handle(&first, &input, CancellationToken::new()),
        async {
            transport.started.notified().await;
            let bytes = component(r#"{"candidates":[],"resolver_duration":456}"#, "", "")?;
            let package = runtime
                .stage(manifest(&bytes, Version::new(1, 1, 0))?, bytes)
                .await?;
            let current = runtime.activate(package)?;
            transport.release.notify_one();
            Ok::<_, Box<dyn std::error::Error>>(current)
        }
    );
    assert_eq!(old_result?.resolver_duration, 123);
    let current = updated?;
    assert_eq!(current.generation(), 2);
    assert_eq!(
        runtime
            .resolve_handle(&current, &input, CancellationToken::new())
            .await?
            .resolver_duration,
        456
    );
    Ok(())
}

#[tokio::test]
async fn staged_package_and_handle_cannot_cross_runtime_engines() -> TestResult {
    let first = runtime()?;
    let second = runtime()?;
    let bytes = component(EMPTY_RESULT, "", "")?;
    let staged = first
        .stage(manifest(&bytes, Version::new(1, 0, 0))?, bytes.clone())
        .await?;
    assert!(matches!(
        second.activate(staged),
        Err(RuntimeError::ForeignPackage)
    ));
    let handle = first.activate(
        first
            .stage(manifest(&bytes, Version::new(1, 0, 0))?, bytes)
            .await?,
    )?;
    assert!(matches!(
        second
            .resolve_handle(&handle, &lookup()?, CancellationToken::new())
            .await,
        Err(RuntimeError::ForeignPackage)
    ));
    Ok(())
}

#[tokio::test]
async fn staging_slots_are_bounded_even_for_shared_compiled_artifacts() -> TestResult {
    let config = RuntimeConfig {
        max_staged_packages: 1,
        ..RuntimeConfig::default()
    };
    let runtime = ProviderRuntime::new(Version::new(0, 1, 0), Arc::new(NoHttp), config)?;
    let bytes = component(EMPTY_RESULT, "", "")?;
    let first = runtime
        .stage(manifest(&bytes, Version::new(1, 0, 0))?, bytes.clone())
        .await?;
    assert!(matches!(
        runtime
            .stage(manifest(&bytes, Version::new(1, 1, 0))?, bytes.clone())
            .await,
        Err(RuntimeError::Capacity)
    ));
    drop(first);
    runtime
        .stage(manifest(&bytes, Version::new(1, 1, 0))?, bytes)
        .await?;
    Ok(())
}
