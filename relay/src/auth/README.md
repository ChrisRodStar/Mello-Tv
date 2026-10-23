# Authorization

Implement private service-bearer checks and offline Ed25519 playback grant validation from docs/sessions.md. Keep control and playback credentials distinct. Reject duplicate query/claim inputs and validate ownership, auth revision and expiry on every resource request. Supply expiry cancellation to response task ownership. No upstream networking or API callback belongs here.
