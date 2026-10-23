mod pool;
use super::{Guard, NetError, Result, context, destination, guard::Epoch};
use crate::{config::Network, contracts::providers::FetchContext};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::{
    body::{Bytes, Incoming},
    client::conn::http1,
};
use hyper_util::rt::TokioIo;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    collections::BTreeMap,
    future::Future,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    net::TcpStream,
    sync::{OwnedSemaphorePermit, Semaphore, mpsc},
    task::JoinSet,
    time::Instant,
};
use tokio_rustls::{
    TlsConnector,
    rustls::{ClientConfig, RootCertStore, pki_types::ServerName},
};
use tokio_util::sync::CancellationToken;
use url::Url;

type Driver = Pin<Box<dyn Future<Output = ()> + Send>>;
trait Io: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Io for T {}
struct Budget {
    counts: Arc<Mutex<BTreeMap<String, usize>>>,
    key: String,
}
impl Budget {
    fn acquire(
        counts: &Arc<Mutex<BTreeMap<String, usize>>>,
        key: String,
        max: usize,
    ) -> Result<Self> {
        let mut map = counts.lock().map_err(|_| NetError::Limit)?;
        let value = map.entry(key.clone()).or_default();
        if *value >= max {
            return Err(NetError::Limit);
        }
        *value += 1;
        Ok(Self {
            counts: counts.clone(),
            key,
        })
    }
}
impl Drop for Budget {
    fn drop(&mut self) {
        if let Ok(mut map) = self.counts.lock()
            && let Some(value) = map.get_mut(&self.key)
        {
            *value -= 1;
            if *value == 0 {
                map.remove(&self.key);
            }
        }
    }
}
pub struct Fetch<'a> {
    pub range: Option<&'a str>,
    pub if_range: Option<&'a axum::http::HeaderValue>,
    pub url: &'a str,
    pub method: Method,
    pub context: &'a FetchContext,
    pub session_id: &'a str,
    /// Caller clips this to grant, durable session, resource and source leases.
    pub deadline: Instant,
    pub cancel: CancellationToken,
}
pub struct Transport {
    acceptance: Option<super::acceptance::Gate>,
    config: Network,
    guard: Guard,
    tls: TlsConnector,
    jobs: mpsc::Sender<Driver>,
    slots: Arc<Semaphore>,
    dns_slots: Arc<Semaphore>,
    resolutions: super::resolution::Cache,
    pool: Arc<pool::Pool>,
    counts: Arc<Mutex<BTreeMap<String, usize>>>,
    dns_url: Url,
    dns_ip: IpAddr,
    #[cfg(test)]
    mock: Option<Mock>,
}
pub struct Worker {
    acceptance: Option<super::acceptance::Gate>,
    guard: Guard,
    jobs: mpsc::Receiver<Driver>,
    max: usize,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.guard.invalidate();
    }
}
impl Worker {
    /// Owned by daemon supervisor; every connection is cancelled and joined on shutdown.
    pub async fn run(self, shutdown: CancellationToken) {
        self.run_inner(shutdown, true).await;
    }
    async fn run_inner(self, shutdown: CancellationToken, attest: bool) {
        let guard = self.guard.clone();
        let stop = shutdown.child_token();
        let monitor = attest.then(|| Box::pin(async move { guard.monitor(stop).await }) as Driver);
        self.run_with_monitor(shutdown, monitor).await;
    }
    async fn run_with_monitor(mut self, shutdown: CancellationToken, monitor: Option<Driver>) {
        let mut tasks = JoinSet::new();
        let mut monitors = JoinSet::new();
        if let Some(monitor) = monitor {
            monitors.spawn(monitor);
        }
        loop {
            tokio::select! { biased;
                _ = shutdown.cancelled() => break,
                _ = async { match &self.acceptance { Some(gate) => gate.stopped().await, None => std::future::pending().await } } => break,
                _ = monitors.join_next(), if !monitors.is_empty() => break,
                result = tasks.join_next(), if !tasks.is_empty() => { if result.is_some_and(|r| r.is_err()) { break; } },
                job = self.jobs.recv(), if tasks.len() < self.max => match job { Some(job) => { tasks.spawn(job); }, None => break },
            }
        }
        self.guard.invalidate();
        self.jobs.close();
        tasks.abort_all();
        monitors.abort_all();
        while tasks.join_next().await.is_some() {}
        while monitors.join_next().await.is_some() {}
    }
}
impl Transport {
    pub fn new(config: Network) -> Result<(Arc<Self>, Worker)> {
        if !cfg!(any(target_os = "macos", target_os = "linux")) {
            return Err(NetError::Guard);
        }
        let dns_url =
            destination::destination(&config.dns_endpoint).map_err(|_| NetError::Destination)?;
        let dns_ip: IpAddr = config
            .dns_endpoint_ip
            .parse()
            .map_err(|_| NetError::Destination)?;
        if dns_url.scheme() != "https"
            || dns_url.query().is_some()
            || !destination::public_ip(dns_ip)
        {
            return Err(NetError::Destination);
        }
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let tls = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let max = config.max_streams_count as usize;
        let (jobs, receiver) = mpsc::channel(max * 2);
        let guard = Guard::default();
        let worker = Worker {
            acceptance: None,
            guard: guard.clone(),
            jobs: receiver,
            max: max * 2,
        };
        Ok((
            Arc::new(Self {
                acceptance: None,
                slots: Arc::new(Semaphore::new(max)),
                dns_slots: Arc::new(Semaphore::new(
                    config.max_streams_per_authority_count as usize,
                )),
                config,
                guard,
                tls: TlsConnector::from(Arc::new(tls)),
                jobs,
                counts: Arc::new(Mutex::new(BTreeMap::new())),
                dns_url,
                dns_ip,
                resolutions: super::resolution::Cache::default(),
                pool: Arc::new(pool::Pool::default()),
                #[cfg(test)]
                mock: None,
            }),
            worker,
        ))
    }
    pub fn with_acceptance(
        config: Network,
        gate: super::acceptance::Gate,
    ) -> Result<(Arc<Self>, Worker)> {
        let (mut transport, mut worker) = Self::new(config)?;
        Arc::get_mut(&mut transport)
            .ok_or(NetError::Limit)?
            .acceptance = Some(gate.clone());
        worker.acceptance = Some(gate);
        Ok((transport, worker))
    }
    pub fn available(&self) -> usize {
        self.slots.available_permits()
    }
    pub fn guard(&self) -> &Guard {
        &self.guard
    }
    pub async fn fetch(&self, request: Fetch<'_>) -> Result<Upstream> {
        let _time = crate::health::Timer::start(crate::health::Stage::Fetch);
        if request.method != Method::GET && request.method != Method::HEAD {
            return Err(NetError::Protocol);
        }
        if request.session_id.is_empty() || request.session_id.len() > 128 {
            return Err(NetError::Protocol);
        }
        let epoch = self.guard.lease()?;
        let deadline = request
            .deadline
            .min(Instant::now() + Duration::from_secs(self.config.mp4_max_transfer_s));
        let admission_time = crate::health::Timer::start(crate::health::Stage::Admission);
        let permit = wait(
            &epoch,
            &request.cancel,
            deadline.min(Instant::now() + Duration::from_secs(self.config.admission_wait_s)),
            self.slots.clone().acquire_owned(),
        )
        .await
        .map_err(|error| {
            if error == NetError::Timeout && Instant::now() < deadline {
                NetError::Limit
            } else {
                error
            }
        })?
        .map_err(|_| NetError::Limit)?;
        drop(admission_time);
        let session = Budget::acquire(
            &self.counts,
            format!("s:{}", request.session_id),
            self.config.max_streams_per_session_count as usize,
        )?;
        let mut url = destination::destination(request.url).map_err(|_| NetError::Destination)?;
        for hop in 0..=self.config.max_redirects_count {
            let authority = Budget::acquire(
                &self.counts,
                format!("a:{}", url.origin().ascii_serialization()),
                self.config.max_streams_per_authority_count as usize,
            )?;
            let mut headers =
                context::headers(request.context, &url).map_err(|_| NetError::Protocol)?;
            headers.insert(
                "accept-encoding",
                axum::http::HeaderValue::from_static("identity"),
            );
            if let Some(range) = request.range {
                headers.insert("range", range.parse().map_err(|_| NetError::Protocol)?);
            }
            if let Some(validator) = request.if_range {
                if request.range.is_none() {
                    return Err(NetError::Protocol);
                }
                headers.insert("if-range", validator.clone());
            }
            let dns_time = crate::health::Timer::start(crate::health::Stage::Dns);
            let ips = self
                .resolve(&url, &epoch, &request.cancel, deadline)
                .await?;
            drop(dns_time);
            destination::answers(&ips).map_err(|_| NetError::Destination)?;
            let exchange_time = crate::health::Timer::start(crate::health::Stage::Exchange);
            let mut wire = self
                .exchange(
                    &url,
                    &ips,
                    request.method.clone(),
                    headers,
                    &epoch,
                    &request.cancel,
                    deadline,
                )
                .await?;
            drop(exchange_time);
            if matches!(wire.status.as_u16(), 301 | 302 | 303 | 307 | 308) {
                if hop == self.config.max_redirects_count
                    || wire.headers.get_all("location").iter().count() != 1
                {
                    return Err(NetError::Protocol);
                }
                let location = wire
                    .headers
                    .get("location")
                    .and_then(|h| h.to_str().ok())
                    .ok_or(NetError::Protocol)?;
                url = destination::redirect(&url, location).map_err(|_| NetError::Destination)?;
                // Close rather than drain any redirect body; no body-based redirect work.
                wire.cancel.cancel();
                continue;
            }
            wire.url = url;
            wire._permit = Some(permit);
            wire._budgets = vec![session, authority];
            return Ok(wire);
        }
        Err(NetError::Protocol)
    }
    async fn resolve(
        &self,
        url: &Url,
        epoch: &Epoch,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<Vec<IpAddr>> {
        #[cfg(test)]
        if let Some(mock) = &self.mock
            && !mock.real_dns
        {
            if mock.pending_dns {
                return wait(epoch, cancel, deadline, std::future::pending()).await;
            }
            let round = mock
                .resolutions
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if mock.rebind && round > 0 {
                return Ok(vec![std::net::Ipv4Addr::LOCALHOST.into()]);
            }
            return Ok(mock.answers.clone());
        }
        if let Some(host) = url
            .host_str()
            .and_then(|h| h.trim_matches(['[', ']']).parse::<IpAddr>().ok())
        {
            return Ok(vec![host]);
        }
        let host = url.host_str().ok_or(NetError::Dns)?;
        loop {
            match self.resolutions.claim(host, epoch)? {
                super::resolution::Claim::Cached(addresses) => {
                    destination::answers(&addresses).map_err(|_| NetError::Destination)?;
                    crate::health::event(&crate::health::DNS_CACHE_HITS);
                    return Ok(addresses);
                }
                super::resolution::Claim::Wait(mut receiver) => {
                    let _ = wait(epoch, cancel, deadline, receiver.changed()).await?;
                }
                super::resolution::Claim::Owner(owner) => {
                    let deadline = deadline
                        .min(Instant::now() + Duration::from_secs(self.config.dns_timeout_s));
                    let _permit = wait(epoch, cancel, deadline, self.dns_slots.acquire())
                        .await?
                        .map_err(|_| NetError::Limit)?;
                    // These futures remain owned by this caller; cancellation drops
                    // both and Owner wakes waiters so one may retry within its bound.
                    let (ipv4, ipv6) = tokio::try_join!(
                        self.resolve_kind(url, 1, epoch, cancel, deadline),
                        self.resolve_kind(url, 28, epoch, cancel, deadline)
                    )?;
                    let mut addresses = ipv4.0;
                    addresses.extend(ipv6.0);
                    destination::answers(&addresses).map_err(|_| NetError::Destination)?;
                    owner.complete(addresses.clone(), ipv4.1.min(ipv6.1))?;
                    return Ok(addresses);
                }
            }
        }
    }
    async fn resolve_kind(
        &self,
        url: &Url,
        kind: u16,
        epoch: &Epoch,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<(Vec<IpAddr>, Duration)> {
        let mut query = self.dns_url.clone();
        query
            .query_pairs_mut()
            .append_pair("name", url.host_str().ok_or(NetError::Dns)?)
            .append_pair("type", &kind.to_string());
        let mut headers = HeaderMap::new();
        headers.insert(
            "accept",
            "application/dns-json"
                .parse()
                .map_err(|_| NetError::Protocol)?,
        );
        let mut wire = self
            .exchange(
                &query,
                &[self.dns_ip],
                Method::GET,
                headers,
                epoch,
                cancel,
                deadline,
            )
            .await?;
        if wire.status != StatusCode::OK {
            return Err(NetError::Dns);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = wire.chunk().await? {
            if bytes.len() + chunk.len() > 65536 {
                return Err(NetError::Dns);
            }
            bytes.extend_from_slice(&chunk);
        }
        parse_dns_with_ttl(&bytes, url.host_str().ok_or(NetError::Dns)?, kind)
    }
    // Keep the independently validated connection inputs explicit at this sole dialing boundary.
    #[allow(clippy::too_many_arguments)]
    async fn exchange(
        &self,
        url: &Url,
        ips: &[IpAddr],
        method: Method,
        headers: HeaderMap,
        epoch: &Epoch,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<Upstream> {
        destination::answers(ips).map_err(|_| NetError::Destination)?;
        if let Some(gate) = &self.acceptance {
            gate.request().map_err(|_| NetError::Limit)?;
        }
        let current = self.guard.lease()?;
        if current.index != epoch.index
            || current.identity != epoch.identity
            || epoch.cancel.is_cancelled()
        {
            return Err(NetError::Guard);
        }
        let authority = url.origin().ascii_serialization();
        let pooled = self.pool.take(&authority, ips, epoch, deadline)?;
        let mut from_pool = pooled.is_some();
        let mut connection = match pooled {
            Some(connection) => connection,
            None => self.connect(url, ips, epoch, cancel, deadline).await?,
        };
        let response = loop {
            let mut request = Request::builder()
                .method(method.clone())
                .uri(&url[url::Position::BeforePath..])
                .body(Full::new(Bytes::new()))
                .map_err(|_| NetError::Protocol)?;
            *request.headers_mut() = headers.clone();
            request.headers_mut().insert(
                "host",
                url[url::Position::BeforeHost..url::Position::AfterPort]
                    .parse()
                    .map_err(|_| NetError::Protocol)?,
            );
            let _header_time = crate::health::Timer::start(crate::health::Stage::Headers);
            let result = wait(
                epoch,
                cancel,
                deadline.min(
                    Instant::now() + Duration::from_secs(self.config.response_header_timeout_s),
                ),
                async {
                    connection.sender.ready().await?;
                    connection.sender.send_request(request).await
                },
            )
            .await?;
            match result {
                Ok(response) => {
                    if from_pool {
                        crate::health::event(&crate::health::CONNECTION_REUSES);
                    }
                    break response;
                }
                Err(_) if from_pool => {
                    // A stale idle connection gets one bounded fresh dial. Only
                    // GET/HEAD with empty bodies enter this transport.
                    drop(connection);
                    connection = self.connect(url, ips, epoch, cancel, deadline).await?;
                    from_pool = false;
                }
                Err(_) => return Err(NetError::Protocol),
            }
        };
        let local_cancel = cancel.child_token();
        let cancel_guard = local_cancel.clone().drop_guard();
        let (parts, body) = response.into_parts();
        if parts
            .headers
            .iter()
            .map(|(k, v)| k.as_str().len() + v.len())
            .sum::<usize>()
            > 16384
        {
            local_cancel.cancel();
            return Err(NetError::Limit);
        }
        let reusable = !parts.headers.get_all("connection").iter().any(|value| {
            value.to_str().map_or(true, |value| {
                value
                    .split(',')
                    .any(|token| token.trim().eq_ignore_ascii_case("close"))
            })
        });
        Ok(Upstream {
            first_byte: Some(Instant::now()),
            status: parts.status,
            headers: parts.headers,
            url: url.clone(),
            body,
            pending: None,
            epoch: epoch.clone(),
            cancel: local_cancel,
            deadline,
            idle: Duration::from_secs(self.config.read_idle_timeout_s),
            chunk_bytes: self.config.body_chunk_bytes as usize,
            reuse: pool::Lease {
                reusable,
                connection: Some(connection),
                pool: self.pool.clone(),
            },
            _cancel_guard: cancel_guard,
            _permit: None,
            _budgets: Vec::new(),
        })
    }
    async fn connect(
        &self,
        url: &Url,
        ips: &[IpAddr],
        epoch: &Epoch,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<pool::Connection> {
        let connect_time = crate::health::Timer::start(crate::health::Stage::Connect);
        let mut connected = None;
        let mut connect_timed_out = false;
        for ip in ips {
            // Every alternate connection is revalidated and bound before connect.
            if !destination::public_ip(*ip) {
                return Err(NetError::Destination);
            }
            let current = self.guard.lease()?;
            if current.identity != epoch.identity || current.index != epoch.index {
                return Err(NetError::Guard);
            }
            let attempt = async {
                if let Some(gate) = &self.acceptance {
                    gate.reserve_connection().map_err(|_| NetError::Limit)?;
                }
                #[cfg(test)]
                if let Some(mock) = &self.mock {
                    if mock.first_timeout
                        && mock.dials.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0
                    {
                        return std::future::pending().await;
                    }
                    if mock.pending_connect {
                        return std::future::pending().await;
                    }
                    mock.dials.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    return TcpStream::connect(mock.address)
                        .await
                        .map_err(|_| NetError::Connect);
                }
                bound_connect(
                    SocketAddr::new(
                        *ip,
                        url.port_or_known_default().ok_or(NetError::Destination)?,
                    ),
                    epoch,
                    self.acceptance.is_some(),
                )
                .await
            };
            match wait(
                epoch,
                cancel,
                deadline.min(Instant::now() + Duration::from_secs(self.config.connect_timeout_s)),
                attempt,
            )
            .await
            {
                Ok(Ok(stream)) => {
                    connected = Some((stream, *ip));
                    break;
                }
                // A failed address never relaxes policy for the next address.
                Ok(Err(NetError::Connect)) => continue,
                Err(NetError::Timeout) if Instant::now() < deadline => {
                    connect_timed_out = true;
                    continue;
                }
                Ok(Err(error)) | Err(error) => return Err(error),
            }
        }
        let (stream, peer) = connected.ok_or(if connect_timed_out {
            NetError::Timeout
        } else {
            NetError::Connect
        })?;
        drop(connect_time);
        let stream: Box<dyn Io> = match &self.acceptance {
            Some(gate) => Box::new(gate.wrap(stream)),
            None => Box::new(stream),
        };
        let io: Box<dyn Io> = if url.scheme() == "https" {
            let _tls_time = crate::health::Timer::start(crate::health::Stage::Tls);
            let server = ServerName::try_from(
                url.host_str()
                    .ok_or(NetError::Tls)?
                    .trim_matches(['[', ']'])
                    .to_owned(),
            )
            .map_err(|_| NetError::Tls)?;
            let tls = wait(
                epoch,
                cancel,
                deadline.min(Instant::now() + Duration::from_secs(self.config.tls_timeout_s)),
                self.tls.connect(server, stream),
            )
            .await?
            .map_err(|_| NetError::Tls)?;
            Box::new(tls)
        } else {
            Box::new(stream)
        };
        let mut builder = http1::Builder::new();
        builder.max_headers(64).max_buf_size(32768);
        let (sender, connection) =
            wait(epoch, cancel, deadline, builder.handshake(TokioIo::new(io)))
                .await?
                .map_err(|_| NetError::Protocol)?;

        let stop = CancellationToken::new();
        let (activity, receiver) = tokio::sync::watch::channel(deadline);
        let lease = pool::Connection {
            sender,
            authority: url.origin().ascii_serialization(),
            peer,
            epoch: epoch.clone(),
            stop: stop.clone(),
            activity,
        };
        self.jobs
            .try_send(Box::pin(pool::drive(
                connection,
                stop,
                epoch.cancel.clone(),
                receiver,
            )))
            .map_err(|_| NetError::Limit)?;
        Ok(lease)
    }
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
async fn bound_connect(address: SocketAddr, epoch: &Epoch, acceptance: bool) -> Result<TcpStream> {
    if !destination::public_ip(address.ip()) || epoch.cancel.is_cancelled() {
        return Err(NetError::Guard);
    }
    let socket = Socket::new(
        Domain::for_address(address),
        Type::STREAM,
        Some(Protocol::TCP),
    )
    .map_err(|_| NetError::Connect)?;
    if acceptance {
        socket
            .set_recv_buffer_size(super::acceptance::READ_RESERVATION)
            .map_err(|_| NetError::Limit)?;
        if socket.recv_buffer_size().map_err(|_| NetError::Limit)?
            > 2 * super::acceptance::READ_RESERVATION
        {
            return Err(NetError::Limit);
        }
    }
    socket
        .set_nonblocking(true)
        .map_err(|_| NetError::Connect)?;
    #[cfg(target_os = "macos")]
    if address.is_ipv4() {
        socket.bind_device_by_index_v4(Some(epoch.index))
    } else {
        socket.bind_device_by_index_v6(Some(epoch.index))
    }
    .map_err(|_| NetError::Guard)?;
    #[cfg(target_os = "linux")]
    socket
        .bind_device(Some(b"wg0"))
        .map_err(|_| NetError::Guard)?;
    let socket: std::net::TcpStream = socket.into();
    let socket = TcpStream::from_std(socket).map_err(|_| NetError::Connect)?;
    // Tokio TcpSocket owns safe nonblocking connect after socket2 binding.
    let socket =
        tokio::net::TcpSocket::from_std_stream(socket.into_std().map_err(|_| NetError::Connect)?);
    let stream = socket
        .connect(address)
        .await
        .map_err(|_| NetError::Connect)?;
    if stream.peer_addr().map_err(|_| NetError::Connect)? != address || epoch.cancel.is_cancelled()
    {
        return Err(NetError::Guard);
    }
    Ok(stream)
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
async fn bound_connect(_: SocketAddr, _: &Epoch, _: bool) -> Result<TcpStream> {
    Err(NetError::Guard)
}
pub(crate) async fn wait<T>(
    epoch: &Epoch,
    cancel: &CancellationToken,
    deadline: Instant,
    future: impl Future<Output = T>,
) -> Result<T> {
    tokio::select! { biased; _ = epoch.cancel.cancelled() => Err(NetError::Guard), _ = cancel.cancelled() => Err(NetError::Cancelled), _ = tokio::time::sleep_until(deadline) => Err(NetError::Timeout), value = future => Ok(value) }
}
pub struct Upstream {
    first_byte: Option<Instant>,
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub url: Url,
    body: Incoming,
    pending: Option<Bytes>,
    epoch: Epoch,
    cancel: CancellationToken,
    deadline: Instant,
    idle: Duration,
    chunk_bytes: usize,
    reuse: pool::Lease,
    _cancel_guard: tokio_util::sync::DropGuard,
    _permit: Option<OwnedSemaphorePermit>,
    _budgets: Vec<Budget>,
}
impl Upstream {
    /// Replay the unused suffix of one bounded transport frame after sniffing.
    pub(crate) fn replay(&mut self, bytes: Bytes) {
        self.pending = (!bytes.is_empty()).then_some(bytes);
    }
    pub async fn chunk(&mut self) -> Result<Option<Bytes>> {
        if let Some(bytes) = self.pending.take() {
            return wait(&self.epoch, &self.cancel, self.deadline, async {
                Some(bytes)
            })
            .await;
        }
        loop {
            let frame = wait(
                &self.epoch,
                &self.cancel,
                self.deadline.min(Instant::now() + self.idle),
                self.body.frame(),
            )
            .await?;
            match frame {
                None => {
                    self.reuse.complete();
                    return Ok(None);
                }
                Some(Err(_)) => return Err(NetError::Protocol),
                Some(Ok(frame)) => match frame.into_data() {
                    Ok(data) => {
                        if data.is_empty() {
                            continue;
                        }
                        if data.len() > self.chunk_bytes {
                            self.cancel.cancel();
                            return Err(NetError::Limit);
                        }
                        if let Some(start) = self.first_byte.take() {
                            crate::health::observe(
                                crate::health::Stage::FirstByte,
                                start.elapsed(),
                            );
                        }
                        crate::health::bytes(&crate::health::UPSTREAM_BYTES, data.len());
                        return Ok(Some(data));
                    }
                    Err(frame) => {
                        if let Ok(trailers) = frame.into_trailers() {
                            for name in [
                                "content-length",
                                "content-range",
                                "content-encoding",
                                "content-type",
                                "etag",
                                "last-modified",
                            ] {
                                if trailers.contains_key(name)
                                    && (!matches!(name, "etag" | "last-modified")
                                        || trailers.get_all(name).iter().count() != 1
                                        || trailers.get(name) != self.headers.get(name))
                                {
                                    return Err(NetError::Protocol);
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}
impl Drop for Upstream {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[cfg(test)]
#[derive(Clone)]
struct Mock {
    address: SocketAddr,
    answers: Vec<IpAddr>,
    dials: Arc<std::sync::atomic::AtomicUsize>,
    real_dns: bool,
    rebind: bool,
    resolutions: Arc<std::sync::atomic::AtomicUsize>,
    pending_dns: bool,
    pending_connect: bool,
    first_timeout: bool,
}
#[cfg(test)]
mod tests;

#[cfg(test)]
fn parse_dns(bytes: &[u8], host: &str, kind: u16) -> Result<Vec<IpAddr>> {
    parse_dns_with_ttl(bytes, host, kind).map(|answer| answer.0)
}
fn parse_dns_with_ttl(bytes: &[u8], host: &str, kind: u16) -> Result<(Vec<IpAddr>, Duration)> {
    let response: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| NetError::Dns)?;
    if response["Status"] != 0 || response["TC"] != false {
        return Err(NetError::Dns);
    }
    let questions = response["Question"].as_array().ok_or(NetError::Dns)?;
    if questions.len() != 1
        || questions[0]["type"] != kind
        || questions[0]["name"].as_str().is_none_or(|name| {
            !name
                .trim_end_matches('.')
                .eq_ignore_ascii_case(host.trim_end_matches('.'))
        })
    {
        return Err(NetError::Dns);
    }
    let mut ips = Vec::new();
    let mut ttl = 60;
    for answer in response["Answer"].as_array().into_iter().flatten() {
        ttl = ttl.min(answer["TTL"].as_u64().unwrap_or(0));
        if answer["type"] == 1 || answer["type"] == 28 {
            let ip = answer["data"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .ok_or(NetError::Dns)?;
            if !destination::public_ip(ip) || ips.len() >= 64 {
                return Err(NetError::Destination);
            }
            ips.push(ip);
        }
    }
    Ok((ips, Duration::from_secs(ttl)))
}
