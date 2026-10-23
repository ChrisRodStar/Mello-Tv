# Controlled upstream transport

Own every upstream DNS lookup, socket, TLS connection, redirect and pooled connection. Implement docs/networking.md before enabling real source fetches. Inject transport dependencies for offline fixtures; production has no unsafe bypass. Bind DNS and sockets to the attested VPN interface, validate literal addresses at dialing, and invalidate active/pool state when the guard epoch changes.
