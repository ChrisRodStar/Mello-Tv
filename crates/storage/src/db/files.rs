//! Creation permissions are confined to this platform filesystem adapter.
//! Existing files/directories are never chmodded or replaced on open.

use crate::StorageError;
use std::{fs, path::Path};

pub(super) fn prepare(path: &Path) -> Result<(), StorageError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(parent)?;
    }
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if !fs::symlink_metadata(path)?.is_file() {
                return Err(StorageError::InvalidInput(
                    "database path must be a regular file",
                ));
            }
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}
