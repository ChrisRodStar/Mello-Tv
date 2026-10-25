//! A real portable Rust guest with no WASI capabilities or handwritten ABI.

wit_bindgen::generate!({ path: "../../../wit", world: "provider" });

struct Fixture;

impl Guest for Fixture {
    fn resolve(lookup_json: String) -> Result<String, String> {
        let _lookup = stream_provider_sdk::decode_lookup(&lookup_json)
            .map_err(|_| "invalid_lookup".to_owned())?;
        Ok(r#"{"candidates":[],"resolver_duration":0}"#.to_owned())
    }
}

export!(Fixture);
