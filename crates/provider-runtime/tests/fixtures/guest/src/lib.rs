//! The only unsafe-policy exception is generated component ABI glue. The
//! provider implementation is a separate crate with unsafe code forbidden.

struct Adapter;

#[allow(unsafe_code)]
mod bindings {
    use super::Adapter;

    wit_bindgen::generate!({ path: "../../../../provider-sdk/wit", world: "provider" });
    export!(Adapter);
}

impl bindings::Guest for Adapter {
    fn resolve(lookup_json: String) -> Result<String, String> {
        stream_provider_fixture_implementation::resolve(&lookup_json)
    }
}
