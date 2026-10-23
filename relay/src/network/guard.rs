use super::{NetError, Result};
#[cfg(target_os = "macos")]
use serde_json::Value;
use std::{
    num::NonZeroU32,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
#[cfg(target_os = "macos")]
use tokio::{io::AsyncReadExt, process::Command};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(crate) struct Epoch {
    pub index: NonZeroU32,
    pub identity: String,
    pub cancel: CancellationToken,
    checked: Instant,
}
#[derive(Clone, Default)]
pub struct Guard(Arc<Mutex<Option<Epoch>>>);
impl Guard {
    pub(crate) async fn monitor(&self, shutdown: CancellationToken) {
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! { biased;
                _ = shutdown.cancelled() => break,
                _ = tick.tick() => {
                    tokio::select! { biased;
                        _ = shutdown.cancelled() => break,
                        _ = self.refresh() => {},
                    }
                }
            }
        }
        self.invalidate();
    }
    pub(crate) fn lease(&self) -> Result<Epoch> {
        let state = self.0.lock().map_err(|_| NetError::Guard)?;
        state
            .as_ref()
            .filter(|e| !e.cancel.is_cancelled() && e.checked.elapsed() < Duration::from_secs(2))
            .cloned()
            .ok_or(NetError::Guard)
    }
    pub fn healthy(&self) -> bool {
        self.lease().is_ok()
    }
    pub(crate) fn invalidate(&self) {
        if let Ok(mut state) = self.0.lock()
            && let Some(epoch) = state.take()
        {
            crate::health::event(&crate::health::VPN_TRANSITIONS);
            epoch.cancel.cancel();
        }
    }
    pub(crate) async fn refresh(&self) -> Result<()> {
        let observed = attest().await;
        let mut state = self.0.lock().map_err(|_| NetError::Guard)?;
        match observed {
            Ok((index, identity)) => {
                if state.as_ref().is_some_and(|e| {
                    e.index == index && e.identity == identity && !e.cancel.is_cancelled()
                }) {
                    if let Some(epoch) = state.as_mut() {
                        epoch.checked = Instant::now();
                    }
                } else {
                    if let Some(old) = state.take() {
                        old.cancel.cancel();
                    }
                    crate::health::event(&crate::health::VPN_TRANSITIONS);
                    *state = Some(Epoch {
                        index,
                        identity,
                        cancel: CancellationToken::new(),
                        checked: Instant::now(),
                    });
                }
                Ok(())
            }
            Err(error) => {
                if let Some(old) = state.take() {
                    crate::health::event(&crate::health::VPN_TRANSITIONS);
                    old.cancel.cancel();
                }
                Err(error)
            }
        }
    }
}
#[cfg(target_os = "macos")]
async fn command(path: &str, args: &[&str]) -> Result<Vec<u8>> {
    tokio::time::timeout(Duration::from_secs(1), async {
        let mut child = Command::new(path)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| NetError::Guard)?;
        let mut output = Vec::new();
        child
            .stdout
            .take()
            .ok_or(NetError::Guard)?
            .take(16385)
            .read_to_end(&mut output)
            .await
            .map_err(|_| NetError::Guard)?;
        if output.len() > 16384 || !child.wait().await.map_err(|_| NetError::Guard)?.success() {
            return Err(NetError::Guard);
        }
        Ok(output)
    })
    .await
    .map_err(|_| NetError::Guard)?
}
#[cfg(target_os = "macos")]
async fn attest() -> Result<(NonZeroU32, String)> {
    let bytes = command("/usr/local/bin/mullvad", &["status", "--json"]).await?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| NetError::Guard)?;
    if value["state"] != "connected" {
        return Err(NetError::Guard);
    }
    let endpoint = &value["details"]["endpoint"];
    let interface = endpoint["tunnel_interface"]
        .as_str()
        .ok_or(NetError::Guard)?;
    if !interface.starts_with("utun") || !interface[4..].bytes().all(|b| b.is_ascii_digit()) {
        return Err(NetError::Guard);
    }
    let output = command("/sbin/ifconfig", &["-v", interface]).await?;
    let output = std::str::from_utf8(&output).map_err(|_| NetError::Guard)?;
    let first = output.lines().next().ok_or(NetError::Guard)?;
    if !first.contains("<UP,") || !first.contains("RUNNING") {
        return Err(NetError::Guard);
    }
    let index = first
        .split(" index ")
        .nth(1)
        .and_then(|s| s.trim().parse::<u32>().ok())
        .and_then(NonZeroU32::new)
        .ok_or(NetError::Guard)?;
    // Include relay endpoint and observed exit location: interface reuse is not continuity.
    let identity = format!("{}|{}|{}", endpoint, value["details"]["location"], first);
    Ok((index, identity))
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
async fn attest() -> Result<(NonZeroU32, String)> {
    Err(NetError::Guard)
}

#[cfg(test)]
impl Guard {
    pub(crate) fn synthetic(&self, identity: &str) {
        let Ok(mut state) = self.0.lock() else {
            return;
        };
        if let Some(old) = state.take() {
            old.cancel.cancel();
        }
        *state = NonZeroU32::new(1).map(|index| Epoch {
            index,
            identity: identity.into(),
            cancel: CancellationToken::new(),
            checked: Instant::now(),
        });
    }
}

#[cfg(target_os = "linux")]
async fn attest() -> Result<(NonZeroU32, String)> {
    use std::os::unix::fs::MetadataExt;
    use std::time::SystemTime;
    let path = "/run/family-stream-vpn/attestation.json";
    let metadata = std::fs::symlink_metadata(path).map_err(|_| NetError::Guard)?;
    if !metadata.is_file()
        || metadata.uid() != 0
        || metadata.mode() & 0o022 != 0
        || SystemTime::now()
            .duration_since(metadata.modified().map_err(|_| NetError::Guard)?)
            .map_err(|_| NetError::Guard)?
            > Duration::from_secs(2)
        || metadata.len() > 4096
    {
        return Err(NetError::Guard);
    }
    let bytes = std::fs::read(path).map_err(|_| NetError::Guard)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| NetError::Guard)?;
    if value["interface"] != "wg0" {
        return Err(NetError::Guard);
    }
    let index = value["index"]
        .as_u64()
        .and_then(|number| u32::try_from(number).ok())
        .and_then(NonZeroU32::new)
        .ok_or(NetError::Guard)?;
    let actual: u32 = std::fs::read_to_string("/sys/class/net/wg0/ifindex")
        .map_err(|_| NetError::Guard)?
        .trim()
        .parse()
        .map_err(|_| NetError::Guard)?;
    if actual != index.get() {
        return Err(NetError::Guard);
    }
    let identity = value["identity"]
        .as_str()
        .ok_or(NetError::Guard)?
        .to_owned();
    Ok((index, identity))
}
