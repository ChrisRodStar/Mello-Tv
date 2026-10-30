# Independent provider health

The Mac Providers page calls `POST /api/v1/provider-health` when opened, and offers
an explicit recheck. While the page is visible it repeats checks every five minutes.
It does not interpret recent viewing requests as health.
Existing status and playback APIs are unchanged.

Each check calls the installed provider directly with a reference movie (Inception,
TMDB 27205 / IMDb tt1375666), falling back to Interstellar (157336 / tt0816692).
A result is working only if the adapter response validates and the normal media
prober records accessible media, completed samples, positive sampled bytes, no
probe failure and known protection. Device codec, resolution and delivery-speed
eligibility rules are deliberately not used to declare a provider unavailable.
This verifies resolution and media access, not decoded playback, content identity,
every title, or every device.
Neither playback sessions nor watch history are created. Normal provider request
observations and resolver-result caches are not changed by health checks.

A batch has at most five concurrent checks. Each provider has two attempts of up
to 18 seconds, including shared provider/probe admission waits. Each attempt probes
at most two candidates, one variant each, with one media sample per variant, a
2 MiB total sample budget and 512 KiB playlist budget. Probe admission is shared
with normal playback. Dropping the batch cancels its provider and probe work.

Concurrent callers share a cached batch for 60 seconds. The cache includes the
provider IDs, versions and generations. Responses contain only provider identity,
working/failed result, completion time, elapsed check duration, sample count and
a sanitized explanation; never media URLs or credentials. A failed reference
check is displayed as **Check failed**, rather than declaring a global outage.

No health checks run until a client requests them. Leaving the Mac page cancels
its pending request. Live provider and device acceptance is a separate manual
verification step.
