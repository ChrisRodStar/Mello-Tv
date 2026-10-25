//! Native media transport, parsing and provider protocols.

pub use stream_provider_codecs as crypto;
pub mod component_http;
pub mod coordinator;
pub mod discovery;
pub mod hls;
pub mod network;
pub mod prober;
pub mod providers;
pub mod relay;
pub mod selection;

pub use coordinator::{
    ComponentProviders, DiscoveryCoordinator, DiscoveryError, DiscoveryLimits, DiscoveryProvider,
    MediaCoordinator, ProviderCatalog,
};
pub use network::HttpClient;
pub use prober::StreamProber;
pub use providers::NativeProviders;
pub use relay::{MediaRelay, RelayError, RelayResponse};
