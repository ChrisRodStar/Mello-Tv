# Health and lifecycle

Own private liveness/readiness, bounded low-cardinality metrics and drain state described in docs/operations.md. Readiness requires a usable store and guarded egress; provider resolver health does not gate existing relay sessions. Startup/shutdown owns and joins tasks. Never expose grants, source URLs or resource IDs through metrics or logs.
