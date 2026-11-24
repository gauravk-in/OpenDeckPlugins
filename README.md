# OpenDeck plugins

Plugins for [OpenDeck](https://github.com/gauravk-in/OpenDeck), one per folder. Each folder is a self-contained plugin with its own `README.md`, `Cargo.toml`, `assets/` (manifest, icons, property inspector) and `scripts/package.sh`, which builds an installable zip.

| Plugin | Folder | What it does |
| --- | --- | --- |
| Web Link | [`weblink/`](weblink) | Open a website with the site's logo on the key, optional caption, a chosen Chrome profile, and switching to an already-open tab on macOS |
| Page Switcher | [`pageswitcher/`](pageswitcher) | Move between pages (profiles) in order with a dial or Next/Previous keys |

Each plugin has a GitHub Actions workflow in `.github/workflows/<folder>.yml`. It runs when files in that folder change, and it publishes a release when you push a tag named `<folder>-v<version>` (for example `pageswitcher-v0.1.0`).
