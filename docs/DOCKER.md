# Run Mello TV with Docker

Docker runs the Rust backend and production web client together. No Mac app,
Rust, Bun, or separate database installation is needed on the host.

Install Docker Desktop on Windows (Linux containers / WSL 2) or macOS, or Docker
Engine and the Compose plugin on Linux. Start Docker and check `docker version`
and `docker compose version` before continuing.

## Start

From the repository root:

```sh
cp .env.example .env
```

On Windows PowerShell, use `Copy-Item .env.example .env` instead. Edit `.env` and
set your own `TMDB_API_KEY` or `TMDB_READ_ACCESS_TOKEN`. Keep this file private.
Without a credential, the server starts, but cannot populate a new catalog.

```sh
docker compose up -d --build --wait
```

Open <http://localhost:8080>, or `http://SERVER-IP:8080` on your home network.
The first build downloads dependencies and compiles Rust; later builds reuse
the build cache. The current Compose file builds locally. A published image and
registry-based updates are a separate distribution step.

Build stages pin their base images by digest. Dependabot checks these pins
weekly; rebuild after reviewing and merging a base-image update.

If port 8080 is in use, set `MELLO_PORT=8081` in `.env`. Set
`MELLO_BIND_ADDRESS=127.0.0.1` for access only from the hosting computer.

## Manage

```sh
docker compose ps
docker compose logs --tail=100 mello
docker compose stop
docker compose start
```

After updating the repository, rebuild and replace the container:

```sh
docker compose up -d --build --wait
```

Profiles, history, catalog cache, and the encryption key remain in the
`mello-data` named volume. `docker compose down` removes the container and
network but preserves that volume. Do not use `down -v` on your installation:
it deletes the persistent data.

The container runs as UID/GID 10001. Its first start generates a private,
32-byte `/data/api-encryption` key; subsequent starts retain it. Back up the
entire data volume, including this key, while the server is stopped. Do not
mount the running Mac app's data into Docker. Migrating an existing database
requires its matching encryption key; startup refuses an existing accounts
database with a missing or malformed key.

The container has a read-only root filesystem, a writable `/data` volume, and
a temporary `/tmp`. It drops Linux capabilities, prevents privilege escalation,
and uses an init process to reap child processes. Logs rotate at 10 MB with
three files retained. Memory and CPU limits should follow measured workload
requirements rather than arbitrary defaults.

## Networking and platform scope

Use a trusted home network or private remote access such as Tailscale. The app
does not provide an account login boundary; do not expose port 8080 directly to
the public internet. Docker does not add a VPN. The basic setup uses ordinary
container internet access; a VPN configuration needs separate verification.

Automatic iPhone discovery is not guaranteed through Docker bridge networking.
Use the host address for browser access. Discovery and device playback require
their own acceptance tests.

The Dockerfile uses architecture-native base images and contains no hard-coded
CPU target. Build separately on Linux ARM64 and AMD64 before claiming support
for both. Windows and macOS use a Linux container through Docker Desktop;
native Windows/macOS applications are not part of the container.

## Packaging verification

After `docker compose build`, run:

```sh
python3 docker/smoke.py
```

This uses a uniquely named Compose project, a separate test volume, and a
loopback port. It checks actual HTTP routes and bundled assets, creates a
profile, recreates the container, and verifies the profile and encryption key
survive. It also checks that missing or malformed encryption keys block unsafe
startup. It removes only its own test containers and volume afterward. It does
not require TMDB credentials or establish provider/device playback acceptance.
Set `MELLO_SMOKE_PORT` if its default port 18080 is occupied.
The Docker workflow is configured to build and run this check on native AMD64
and ARM64 Linux runners.

The packaging follows [Docker's build guidance](https://docs.docker.com/build/building/best-practices/)
and [Compose runtime configuration](https://docs.docker.com/reference/compose-file/services/).
