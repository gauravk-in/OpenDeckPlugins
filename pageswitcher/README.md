# Page Switcher for OpenDeck

Move between pages (OpenDeck profiles) in order, without wiring up a "switch profile" action on every page.

Actions:

- **Page dial** (knob): turn right for the next page, left for the previous one. Press to jump to the first page.
- **Next page** / **Previous page** (key or knob press). Keys show the name of the page they lead to.

Settings per action: wrap around at the ends (on by default), stay inside the current page's folder, reverse the dial, what pressing the dial does, and whether keys show the target page's name.

## Page order

Pages follow the order you set by dragging them in **Edit profiles…** (needs the OpenDeck build from `gauravk-in/OpenDeck`). Without a custom order, pages go alphabetically, like stock OpenDeck lists them. The plugin reads the profile list from OpenDeck's config folder each time, so new, renamed and reordered pages are picked up straight away.

## One knob for every page

In the `gauravk-in/OpenDeck` build, right-click the knob or key and choose **Show on all profiles**. The action is copied to the same spot on every page and kept in sync, so you set it up once.

## Requirements

Switching profiles from a plugin is a non-standard OpenDeck message. Stock OpenDeck only accepts it from its own starter pack, so this plugin needs the `gauravk-in/OpenDeck` build, which also allows `io.github.gauravk-in.pageswitcher`.

## Build

```
pip install ziglang && cargo install cargo-zigbuild
rustup target add aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin
scripts/package.sh   # -> dist/opendeck-pageswitcher.zip (macOS universal, Linux x86_64 and aarch64)
```

Install the zip from OpenDeck's Plugins window ("Install from file"). `opendeck-pageswitcher --pages <device id>` (with `OPENDECK_CONFIG_DIR` set) prints a device's pages in the order the plugin uses.

## License

MIT
