//! Safe fixture provider. This crate contains no generated ABI or network I/O.

pub fn resolve(lookup_json: &str) -> Result<String, String> {
    let _lookup = stream_provider_sdk::decode_lookup(lookup_json)
        .map_err(|_| "invalid_lookup".to_owned())?;
    Ok(r#"{"candidates":[],"resolver_duration":789}"#.to_owned())
}
