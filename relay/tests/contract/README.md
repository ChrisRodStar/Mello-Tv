# Contract regressions to implement

Generate redacted synthetic selections using the actual Go models pinned in
../../docs/provider-handoff.md, then deserialize them with production Rust types.
Cover optional/null fields, unknown metadata, nanosecond durations, zero-valued
booleans, canonical IDs and exact TV coordinates. Validate create/renew/source and
redacted responses against ../../contracts/relay.schema.json. Check missing
selected source, oversized imports, owner mismatch and stale revision rejection.
The existing JSON examples are design fixtures, not cross-language runtime proof.
