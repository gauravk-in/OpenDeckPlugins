# Web Link for OpenDeck

An [OpenDeck](https://github.com/nekename/OpenDeck) plugin with one action, **Website**: press the key to open a URL, and the key shows that site's own logo, fetched automatically.

The built-in *Open URL* action in OpenDeck's starter pack opens links but leaves the key blank. This plugin fills that gap.

## What it does

- Type `github.com`, `https://news.ycombinator.com` or `http://homeassistant.local:8123` and the key picks up the logo within a second or two.
- Looks for the best image the site publishes, in this order:
  1. `apple-touch-icon` (a square tile made for home screens, so it looks best on a key)
  2. Web app manifest icons
  3. `<link rel="icon">`, largest first; SVG icons are rendered sharply
  4. `/apple-touch-icon.png` and `/favicon.ico`
  5. Google's favicon service (can be turned off)
  6. If all else fails, a coloured tile with the site's first letter
- Square, opaque icons fill the whole key. Transparent logos are trimmed, padded and put on a background: **Automatic** picks white behind dark logos and black otherwise, or choose black, white or any colour.
- Logos are cached on disk (`~/Library/Caches/opendeck-weblink` on macOS, `~/.cache/opendeck-weblink` on Linux) for 30 days. **Fetch logo again** in the settings refreshes one.
- Self-signed HTTPS certificates are accepted when fetching logos, so local dashboards (Home Assistant, routers, NAS) work.
- Works on keys and on dials (press to open). Runs on macOS (Apple Silicon and Intel) and Linux x86_64 / aarch64 (Raspberry Pi 4/5 with a 64-bit OS).

## Install

1. Download `opendeck-weblink.zip` from the [latest release](../../releases) (or the build artifact of the latest workflow run).
2. In OpenDeck open **Plugins**, choose **Install from file**, and pick the zip.
3. Drag **Web Link → Website** onto a key and type an address.

## Build

```sh
pip install ziglang
cargo install cargo-zigbuild
rustup target add aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin
scripts/package.sh           # -> dist/opendeck-weblink.zip
```

Everything cross-compiles from Linux; no macOS machine is needed. GitHub Actions builds the zip on every push and attaches it to a release for `v*` tags.

Debug helper, to see which logo a site resolves to without OpenDeck:

```sh
cargo run --release -- --probe github.com /tmp/github.png
```

## Licence

MIT. Bundles the DejaVu Sans Bold font for letter tiles (see `fonts/DejaVu-LICENSE.txt`).
