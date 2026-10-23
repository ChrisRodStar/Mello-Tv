# Private control API

Implement contracts/openapi.json on the private listener, including strict import validation, idempotency, owner matching, CAS mutations, redacted reads and revocation. The relayctl importer authenticates to these same production routes. Delegate durable changes to session and source preparation to the guarded transport; never expose arbitrary URL fetching.
