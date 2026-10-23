Synthetic offline TLS fixtures. `leaf-key.der` is a public test-only PKCS#8 key,
never a production credential. The test CA cert signs a leaf for
`source.example.com`, valid for ten years from 2026-10-15. The CA private key was
discarded. Only cfg(test) code trusts this CA. Production always uses WebPKI roots.
Tests verify original SNI/Host, trusted TLS, wrong-host and untrusted rejection.
