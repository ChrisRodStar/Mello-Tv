# VidSrc protocol fixtures

HTML files reproduce the observed minimal movie/TV iframe and literal JSON
configuration shapes, with synthetic gate/API tokens and no downloaded scripts.
`encrypted.json` contains two reserved `.example` destinations encrypted with a
zero key/nonce and the independent RFC 8439 Appendix A.1 zero-counter keystream.
It contains no live media URL or credential.

The three WASM files are **static data fixtures, never executed**. They retain the
statically traced ChaCha20 block/allocator/decrypt function bytes for fingerprint
regressions. Live key data, decoy bodies, globals and unrelated data were removed
or replaced. Keys XOR to zero. `rotating-layout.wasm` uses a different mask address
and decoy export names, with conflicting junk at the old mask address;
`shifted-functions.wasm` changes the decoy count and actual block-call index.
This tests layout recognition and native decryption without an interpreter.

Private optional capture replay uses `VIDSRC_CAPTURE` and is skipped by default.
All normal tests use synthetic fetchers and run offline. Raw research captures,
resolved URLs, generated audio and playback frames belong in ignored `.local/`
or private temporary directories, never these fixtures.
