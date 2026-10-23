# Security behavior cases

Use synthetic destinations and injected resolvers/connectors to cover public-to-
private DNS rebinding, IPv4-mapped IPv6, redirect-to-loopback, mixed address answers,
VPN epoch changes, stale pool reuse, scope/header leakage, duplicate JWT claims,
wrong audience/algorithm/owner/session/revision, revoked grants, unknown resources,
range widening and unsupported URI-bearing HLS tags. Assert no socket or body is
opened after rejection, and that logs/errors contain no sensitive source context.
Production source fetches must never use an injected-test bypass.
