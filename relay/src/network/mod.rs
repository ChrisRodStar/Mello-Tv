//! The only source/media egress boundary. No system resolver or proxy; epoch-bound idle reuse only.
pub mod acceptance;
pub mod context;
pub mod destination;
mod guard;
mod resolution;
mod transport;
pub use guard::Guard;
pub use transport::{Fetch, Transport, Upstream, Worker};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetError {
    Guard,
    Destination,
    Dns,
    Connect,
    Tls,
    Timeout,
    Protocol,
    Limit,
    Cancelled,
}
pub type Result<T> = std::result::Result<T, NetError>;
impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "upstream_{self:?}")
    }
}
impl std::error::Error for NetError {}
