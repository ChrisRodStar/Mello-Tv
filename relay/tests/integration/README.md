# Production-path integration cases to implement

Use real routers, encrypted temporary state and injected bounded transport peers.
Cover create -> authorized entry -> master -> audio/subtitle/video -> range/key/init;
then cancellation, revoke, grant expiry, restart/restore, source generation change,
idempotent retries and store failure. Old resource IDs must never resolve to a new
source. Slow readers and upstream stalls must stay within configured bounds.
No test-only production network bypass or second preview server is allowed.
Live decoder/browser/device and tunnel-loss checks require separate authorization.
