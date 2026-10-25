<a id="readme-top"></a>

<div align="center">

[![Version](https://img.shields.io/badge/version-v0.5.0-e50914?style=for-the-badge)](https://github.com/ChrisRodStar/stream/releases)
[![Contributors](https://img.shields.io/github/contributors/ChrisRodStar/stream?style=for-the-badge&color=green)](#top-contributors)
[![Stars](https://img.shields.io/github/stars/ChrisRodStar/stream?style=for-the-badge&color=gold)](https://github.com/ChrisRodStar/stream/stargazers)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20iOS%20%7C%20Web-blue?style=for-the-badge)](#platforms)

# Mello TV 🎬

### _Support Mello TV’s development by starring this repository!_ ⭐

#### Self-hosted streaming server and native apps built for your home.
Powered by a single, lightweight Rust engine with zero-config network discovery across iPhone, Mac, and the web.

**[🌐 Mello Watch](https://mellowatch.com)** · **[📖 Documentation](https://mellowatch.com/docs)** · **[💬 Discussions](https://github.com/ChrisRodStar/stream/discussions)** · **[🐛 Issues](https://github.com/ChrisRodStar/stream/issues)**

</div>

---

**Mello TV** is a private, self-hosted streaming server and app suite for movies and TV shows. Run the server in your Mac menu bar (or on any home server), open the **Mello TV** app on your iPhone or browser, and start watching right away—no port forwarding, no manual IP setup, and no third-party ads or redirects.

It resolves streams across multiple providers in parallel and relays video cleanly to your devices while keeping your household profiles, watchlists, and playback progress in sync.

**Instant access to multiple high-quality sources for any movie or TV show!**

> [!CAUTION]
> Mello TV is designed for **personal and home use only.**  
> While we do not prevent remote hosting, it is intended to run on your local network or private Tailscale mesh.*
>
> Users are responsible for ensuring compliance with applicable laws and the terms of service of upstream providers.

<details><summary>*:</summary>

Mello TV automatically resolves direct media streams from third-party providers while stripping out ads, popups, and malicious redirects, then proxies the clean video stream to your devices.

Because resolving and relaying streams uses local compute and bandwidth, exposing an unprotected instance to the public internet can invite abuse. We strongly recommend running Mello TV on your home network and using [Tailscale](https://tailscale.com) when watching away from home.

</details>

## Quick Start

**Prerequisites:** Rust (stable), Node.js 20+, and a [TMDB API key](https://www.themoviedb.org/settings/api)

```bash
git clone https://github.com/ChrisRodStar/stream.git && cd stream
cp deploy/.env.example .env   # add your TMDB_API_KEY
cargo run -p stream-server    # http://localhost:8080
```

Want the native macOS menu bar app (`.app` & `.dmg`)?

```bash
./macos/package-macos.sh
```

For full setup guides, iOS companion installation, and remote Tailscale streaming → **[Documentation](https://mellowatch.com/docs)**

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Features

* 📡 **Zero-Config Discovery** – iOS and web clients automatically find your Mello TV server on Wi-Fi via UDP broadcast and Bonjour
* 🌍 **Seamless Tailscale Roaming** – learns your server’s Tailscale address automatically so you can keep watching away from home
* 🎬 **Multi-Source Streaming** – resolves and ranks multiple movie and TV sources with automatic backup failover
* 🛡️ **Clean Private Relay** – proxies playlists and segments locally so client devices never touch third-party ad networks or trackers
* 📱 **Native iOS & Web Apps** – fluid SwiftUI iPhone app and responsive web interface with a matching cinema aesthetic
* 👥 **Household Profiles** – personalized recommendations, watch history, Favorites, and Continue Watching shelves for everyone at home
* ⚡ **Single Lightweight Binary** – 100% Rust backend with embedded SQLite storage and zero headless browser bloat

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Platforms

Mello TV is built as an all-in-one ecosystem inside a single repository:

* **🖥️ Mello TV Server (`crates/` & `macos/`)** – Runs silently in the macOS menu bar or as a standalone service on Linux, Windows, and Docker.
* **📱 Mello TV for iOS (`ios/`)** – Native SwiftUI & AVKit player with instant local discovery, background stream pre-warming, and tactile controls.
* **🌐 Mello TV Web (`web/`)** – Built-in web client served directly by your server at `http://localhost:8080`.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Documentation

Full setup guides, configuration options, and architecture notes are available on **[Mello Watch](https://mellowatch.com/docs)**.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Contributing

Pull requests, bug reports, and provider improvements are welcome! Feel free to open an [issue](https://github.com/ChrisRodStar/stream/issues) or start a [discussion](https://github.com/ChrisRodStar/stream/discussions).

### Top Contributors

<a href="https://github.com/ChrisRodStar/stream/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=ChrisRodStar/stream" alt="contrib.rocks image" />
</a>

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## License

Designed for personal, non-commercial household use.

This software does not host, store, or distribute copyrighted media files. All metadata and artwork are provided by [TMDB](https://www.themoviedb.org).

<p align="right">(<a href="#readme-top">back to top</a>)</p>
