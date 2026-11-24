# tado° Thermostats for OpenDeck

Shows your tado° rooms on keys and lets a dial change their temperature. It is made for the "pick a device on a key, adjust it with a knob" layout, for example on a Streonor S6.

- **Room** (key): shows the room's name, its current temperature and the target temperature. Press it to select the room; the selected key gets an orange frame. Pressing the selected room again asks tado° for fresh values.
- **Temperature dial** (knob): turn it to change the selected room's target temperature. The key shows the new target in yellow while you turn, and the plugin sends it to tado° 1.5 seconds after you stop, so one turn costs one request. Turning below 5 °C turns the heating off. Pressing the dial puts the room back on its schedule (or turns the heating off, if you choose that).

On the key, an orange target means a manual change is active, blue "Off" means the heating is off, and the red bar along the bottom shows how hard the room is heating.

Works with classic tado° (V3+) homes and tado° X homes. tado° X support follows the API used by the Python `python-tado` library and has not been tried on a real tado° X system yet.

## Signing in

Open the settings of any Room key or Temperature dial and click **Sign in with tado°**. The plugin opens tado°'s sign-in page in your browser and shows a code and a QR code; confirm the code on that page (the QR code lets you do it from your phone, which is handy on a Raspberry Pi). This is tado°'s OAuth device code flow, the only sign-in tado° allows for third-party tools since 2025. Your password never passes through the plugin.

The plugin keeps only the refresh token tado° hands out, in `auth.json` in its own folder, readable only by your user account:

- macOS: `~/Library/Application Support/opendeck-tado/`
- Linux: `~/.config/opendeck-tado/`

tado° replaces the refresh token every time it is used and it expires after 30 days without use, so if OpenDeck is off for a month you will need to sign in again. **Sign out** in the settings deletes the token.

## tado°'s request limit

Without an Auto-Assist subscription tado° allows **100 API requests a day** (20,000 with one). The plugin is careful with them:

- One refresh is one request per home, however many rooms you show. The default is every 30 minutes (48 a day); change it under **Refresh every**. Only homes with a Room key on screen are refreshed.
- A temperature change is one request. The plugin does not re-read the room afterwards.
- Signing in and loading your rooms costs 1 + 2 per home. Rooms are cached, so restarting OpenDeck does not reload them; use **Reload rooms** after adding a room in the tado° app.
- The settings show how many requests tado° says are left today. When they run out, the keys keep the last values until the allowance refills.

## Building

```sh
pip install ziglang && cargo install --locked cargo-zigbuild
rustup target add aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin
scripts/package.sh          # -> dist/opendeck-tado.zip (macOS universal, Linux x86_64 and aarch64)
cargo test
```

`opendeck-tado --render keys.png` draws sample keys, to check the look without a device.

For testing against a fake server, `TADO_LOGIN_BASE`, `TADO_API_BASE` and `TADO_HOPS_BASE` replace tado°'s URLs and `OPENDECK_TADO_DIR` replaces the plugin's folder.

## Licence

MIT. Bundles DejaVu Sans Bold (see `fonts/DejaVu-LICENSE.txt`). Not affiliated with tado° GmbH.
