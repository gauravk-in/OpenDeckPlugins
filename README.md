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
- Optional **Caption**: the logo shrinks and the text goes underneath (handy for two links to the same site). Long captions wrap to two lines or get shortened.
- **Open with** a specific Chrome, Brave, Edge or Chromium profile (e.g. work vs personal). Profiles are read from the browser's `Local State` file; `opendeck-weblink --profiles` lists what it finds.
- **Switch to the tab if it's already open** (macOS): finds a Safari/Chrome/Brave/Edge tab showing that address (or a page under it) and brings it to the front instead of opening a new one. Uses AppleScript, so macOS asks once to let OpenDeck control the browser. Chrome's scripting can't tell profiles apart, so the first matching tab in any profile wins.
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

## Testing checklist

1. Install the zip (see above) and drag **Website** onto a key.
2. Type `github.com` and press Enter. Within a couple of seconds the key and the preview in the settings panel show the GitHub logo, with "Logo from …" naming where it came from.
3. Press the key: the site opens in your default browser.
4. Try a local device, e.g. `http://homeassistant.local:8123`. A self-signed certificate is fine.
5. Try something with no icon, e.g. `http://192.168.1.1` on a bare router page: you get a letter tile.
6. Change **Background** and **Logo padding** and watch the key update. **Fetch logo again** re-downloads it.
7. Restart OpenDeck: keys come back instantly from the cache.

Plugin logs: OpenDeck's log folder, `plugins/io.github.gauravk-in.weblink.sdPlugin.log`.
