//! Sandboxed, independently versioned Stream TV provider components.

mod bindings {
    wasmtime::component::bindgen!({
        path: "../provider-sdk/wit",
        world: "provider",
        imports: { default: async },
        exports: { default: async },
    });
}

pub mod http;
mod runtime;

pub use runtime::{ProviderHandle, ProviderRuntime, RuntimeConfig, RuntimeError, StagedPackage};
