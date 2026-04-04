<p align="center">
  <img src="assets/coconut.svg" alt="Coconut" width="128" height="128">
  <br>
  <br>
  <h1>Coconut</h1>
  <br>
  <i>Archive your entire GOG library locally.</i>
  <br>
  <br>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/rust-1.85%2B-orange?style=flat-square&logo=rust" alt="Rust"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue?style=flat-square" alt="MIT License"></a>
  <a href="https://github.com/<owner>/coconut/actions"><img src="https://img.shields.io/badge/build-passing-brightgreen?style=flat-square" alt="Build"></a>
</p>

## Installation

Requires [Rust](https://rustup.rs/) (1.85+).

```sh
git clone https://github.com/<owner>/coconut.git
cd coconut
cargo build --release
```

The binary will be at `./target/release/coconut`.

## Authentication

Coconut uses GOG's Galaxy OAuth2 flow. You authenticate once via your browser:

```sh
coconut auth login
```

This opens GOG's login page. After signing in, paste the redirect URL back into the terminal. Your token is stored at `~/.config/coconut/token.json` and refreshes automatically.

```sh
coconut auth status    # check login state
coconut auth logout    # remove stored token
```

## Library

### List owned games

```sh
coconut library list
coconut library list --platform linux
```

### Show game details

```sh
coconut library info "baldurs gate"
```

Displays available downloads per platform, extras (manuals, soundtracks), and game features.

### Sync game files

```sh
coconut library sync
```

Downloads all installers and extras for every game in your library. Files are organized as:

```
~/coconut/library/
  game_slug/
    windows/
      setup_game_1.0.exe
    linux/
      game_installer.sh
    mac/
      game_installer.pkg
    extras/
      manual.pdf
      soundtrack.zip
  .coconut-manifest.json
```

On first run, you'll be prompted to confirm the sync directory (default: `~/coconut/library`).

### Filtering

```sh
coconut library sync --game "witcher"        # sync a specific game
coconut library sync --platform linux        # only Linux installers
coconut library sync --game "doom" --force   # re-download even if present
```

### Overflow directory

When your primary disk runs out of space, provide a secondary location:

```sh
coconut library sync --sync-dir ~/coconut/library --overflow-dir /mnt/external/coconut
```

Coconut monitors free space and automatically switches to the overflow directory when the primary drops below 1 GB. Each directory maintains its own manifest.

### Download history

Coconut keeps a global download history at `~/.config/coconut/download_history.json` that records every successful download. This persists even if you move or delete the downloaded files, so re-running sync won't re-download files you've already pulled.

To import entries from existing per-directory manifests (e.g., from downloads made before the history feature was added):

```sh
coconut library sync --backfill-history
```

### Resilience

- **Resume**: Interrupted downloads resume from where they left off via `.part` files and HTTP Range headers.
- **Retries**: Failed downloads are retried up to 10 times automatically.
- **Stall detection**: If no data arrives for 60 seconds, the download is restarted.
- **Token refresh**: Expired tokens are refreshed transparently on 401 responses.

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for details.

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE) for details.
