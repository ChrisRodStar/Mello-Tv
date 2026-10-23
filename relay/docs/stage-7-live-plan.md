# Stage 7: approved local live plan and execution boundary

The user approved this plan during stage 7. Executed evidence and the byte-budget
overrun are recorded in [stage 7](stage-7.md); this plan alone is not evidence.
The authorized run has ended. Do not reuse its approval for further media checks. Changes stay in the
relay repository. Private config, keys, grants, selections and receipts live in a
mode-0700 temporary operator directory outside Git; no signed URL or raw capture
is printed. No VPN/firewall change, device operation, packaging, Server, Serve or
deployment is included.

1. Provision ephemeral local relay secrets/config and an acceptance-only Ed25519
   signer (the API signer boundary remains synthetic). Start the actual daemon
   on loopback, using its encrypted SQLite store and production transport. Run
   `relay-netcheck CONFIG`: require both IPv4 and IPv6 Mullvad exit attestations;
   stop if either fails. Do not repair or disconnect the system VPN incidentally.
2. Use the existing Go `cmd/vidfast-check` diagnostic with all four adapters,
   no `-vidfast-only` and no fixed priority. Request three exact titles: movie
   TMDB 550; TV TMDB 60625 season 1 episode 1; anime TV TMDB 1429 season 1 episode 1.
   Each invocation has its existing 45-second total bound and writes its native
   `providers.v1` selection privately. Build the diagnostic to the temporary
   directory without editing provider files. Resolver/probe bytes are distinct
   from Rust relay bytes. Missing HLS/MP4/rendition/protection cases remain open;
   never substitute another unrequested endpoint or execute downloaded code.
3. Import each available selection through `relayctl import-selection` and its
   optional private receipt. Issue a short, locally signed grant from the returned
   SID/owner/auth_revision; never substitute control revision. Run
   `tools/acceptance_probe.py --opt-in` through the actual production listener.
   Each case is capped at 120 seconds, 64 requests and 8 MiB received; manifests
   cap at 1 MiB, media at 64 KiB per prefix. MP4 probes first/middle/last byte
   ranges; these are not temporal seeks. Missing byte-range playlist support in
   the probe is a tooling limitation, not a source rejection.
4. If FFmpeg/FFprobe become available, prepare a separate bounded decoder run
   using only authenticated loopback relay resources: 30 seconds video plus
   independent PCM audio/silence analysis at startup and two selected late times;
   then at most five minutes sustained delivery per available format. Cap the
   whole live session at 30 minutes and received media at 512 MiB. Kill/join the
   decoder at the bound; suppress raw stderr containing URLs/grants. No direct
   source decode fallback. Record decoded frames/audio separately from player
   behavior; non-silent audio does not establish language. FFmpeg/FFprobe are
   initially absent from PATH; a local ignored FFmpeg installation was used as
   recorded in the evidence. Byte probing alone cannot satisfy this step.
5. With a usable backup, explicitly call the documented private failover route
   using owner/expected_revision; require new generation and resume_required.
   Reopen the new entry and explicitly restore playback time when a player is
   available. Test a graceful relay SIGTERM and restart of this test-owned process
   with the same store/keys; retry saved URLs and document broken old connections.
   Stop after one failover and one restart per format. Do not restart unrelated
   provider/API/catalog processes. Their absence in this isolated run proves only
   lack of per-resource calls, not integration or actual replacement acceptance.
6. Local Safari startup/seek/subtitle/sustained player testing needs the approved
   live sources and an interactive browser session. Record measured events and
   visible/audible results; no device claims. API/catalog adoption, a running
   replaceable provider service, unavailable media cases and device/deployment/
   tunnel-disruption checks remain blockers rather than inferred successes.

Approval was granted for steps 1–3 and the local source/media checks in 4–6 within
these bounds, including a test-owned daemon restart and local Safari playback.
The experimental decoder exceeded the byte cap; remaining checks were stopped.
A future run needs a receive-boundary aggregate byte gate and fresh approval.
System VPN disruption, devices and remote/deployment work remain separate.
