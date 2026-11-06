//! Safe provider protocol orchestration. HTTP, time and entropy are explicit
//! host capabilities; this crate opens no sockets and executes no downloaded code.

pub mod anyembed;
pub mod cinesu;
mod common;
mod destination;
pub mod fmovies4u;
pub mod fshare;
pub mod icefy;
pub mod moviedownloader02;
pub mod peachify;
pub mod popr;
pub mod streammafia;
pub mod tulnex;
pub mod vidapi;
pub mod vidcore;
pub mod videasy;
pub mod vidfast;
pub mod vidlink;
pub mod vidnest;
pub mod vidrock;
pub mod vidsrc;
pub mod vidzee;
pub mod vixsrc;

pub use common::{AdapterError, ProviderHost};

use stream_core::providers::{AdapterResult, Lookup};

pub const PROVIDERS: [&str; 20] = [
    "vidcore",
    "vidlink",
    "vidsrc",
    "vidfast",
    "videasy",
    "vidrock",
    "anyembed",
    "cinesu",
    "fmovies4u",
    "fshare",
    "icefy",
    "moviedownloader02",
    "peachify",
    "popr",
    "streammafia",
    "tulnex",
    "vidapi",
    "vidnest",
    "vidzee",
    "vixsrc",
];

/// An individual component wrapper chooses its provider at compile time. A
/// lookup cannot switch the package's provider identity through JSON input.
pub fn resolve(
    provider: &str,
    lookup: &Lookup,
    host: &dyn ProviderHost,
) -> Result<AdapterResult, AdapterError> {
    lookup.validate().map_err(|_| AdapterError::Input)?;
    let start = host.now_unix_ms()?;
    let mut result = match provider {
        "vidcore" => vidcore::resolve(lookup, host)?,
        "videasy" => videasy::resolve(lookup, host)?,
        "vidfast" => vidfast::resolve(lookup, host)?,
        "vidlink" => vidlink::resolve(lookup, host)?,
        "vidsrc" => vidsrc::resolve(lookup, host)?,
        "vidrock" => vidrock::resolve(lookup, host)?,
        "anyembed" => anyembed::resolve(lookup, host)?,
        "cinesu" => cinesu::resolve(lookup, host)?,
        "fshare" => fshare::resolve(lookup, host)?,
        "icefy" => icefy::resolve(lookup, host)?,
        "popr" => popr::resolve(lookup, host)?,
        "vidapi" => vidapi::resolve(lookup, host)?,
        "moviedownloader02" => moviedownloader02::resolve(lookup, host)?,
        "peachify" => peachify::resolve(lookup, host)?,
        "vidnest" => vidnest::resolve(lookup, host)?,
        "vidzee" => vidzee::resolve(lookup, host)?,
        "vixsrc" => vixsrc::resolve(lookup, host)?,
        "fmovies4u" => fmovies4u::resolve(lookup, host)?,
        "streammafia" => streammafia::resolve(lookup, host)?,
        "tulnex" => tulnex::resolve(lookup, host)?,
        _ => return Err(AdapterError::Unsupported),
    };
    let end = host.now_unix_ms()?;
    result.resolver_duration = end.saturating_sub(start).max(0).saturating_mul(1_000_000);
    Ok(result)
}
