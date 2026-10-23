use crate::error::{CONFIG, Error};
use std::{
    fs::OpenOptions,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};
use zeroize::Zeroizing;

/// Open first, then inspect the same descriptor; no pathname stat/read race.
/// O_NOFOLLOW prevents silently using a symlink as a credential file.
pub fn read_private(path: &Path, limit: usize) -> Result<Zeroizing<Vec<u8>>, Error> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| CONFIG)?;
    let meta = file.metadata().map_err(|_| CONFIG)?;

    if !meta.is_file() || meta.mode() & 0o777 != 0o600 || meta.len() > limit as u64 {
        return Err(CONFIG);
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| CONFIG)?;
    if bytes.is_empty() || bytes.len() > limit {
        return Err(CONFIG);
    }
    Ok(bytes)
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("relay foundation supports macOS and Linux only");

pub fn bearer(path: &Path) -> Result<Zeroizing<Vec<u8>>, Error> {
    let mut value = read_private(path, 4096)?;
    if value.last() == Some(&b'\n') {
        value.pop();
    }
    if value.len() < 32
        || !value
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~+/=".contains(b))
    {
        return Err(CONFIG);
    }
    Ok(value)
}
