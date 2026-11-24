# Media Knob

An [OpenDeck](https://github.com/gauravk-in/OpenDeck) plugin that turns a knob (dial) into a media control. Each knob has its own settings, so on a Streonor S6 the big knob can do volume while a small one skips tracks.

| Setting | Choices |
| --- | --- |
| Turning | Changes the volume · Seeks in the track · Goes to the next / previous track · Does nothing |
| Pressing | Plays / pauses · Mutes / unmutes · Next track · Previous track · Does nothing |
| Turning while held | Same as turning, or volume / seek / tracks (releasing after turning is not a press) |
| Volume step | 1 to 25 % per tick (default 5) |
| Seek step | 1 to 120 seconds per tick (default 10) |
| Reverse turning direction | Swap clockwise and counter-clockwise |
| Player | Whatever is playing · Spotify · Music (macOS) |
| Show the volume as the title | Shows "45%" or "Muted" on knobs that do volume or mute |

The action also works on a key, where pressing does the chosen press action. Quick turns are merged into one change, and track skipping takes at most one track per 350 ms so a spin does not jump through the playlist.

## How it controls things

**macOS**

- Volume and mute: AppleScript (`osascript`), on the current output device. Raising the volume unmutes.
- Play/pause, next, previous: sent as the keyboard's media keys, so they reach whatever app macOS shows in Now Playing (Music, Spotify, Safari, Chrome, ...). This needs OpenDeck to be allowed under **System Settings > Privacy & Security > Accessibility**. Until it is, or when you pick Spotify or Music as the player, the plugin tells Spotify or Music directly (macOS asks once whether OpenDeck may control them).
- Seek: macOS has no system-wide seek, so seeking works with Spotify and Music, whichever is playing.

**Linux** (including Raspberry Pi OS)

- Volume and mute: `wpctl` (PipeWire), else `pactl` (PulseAudio), else `amixer` (ALSA, `Master` or `PCM` control), whichever works first.
- Play/pause, tracks and seek: MPRIS over the session D-Bus, which most players support (Spotify, VLC, Firefox, Chromium, mpv with mpv-mpris, ...). It picks a playing player, else the one it controlled last, else a paused one.

## Install

Download `opendeck-mediaknob.zip` from the releases (or build it, below) and in OpenDeck choose **Plugins > Install from file**. The zip has binaries for macOS (Intel and Apple silicon), Linux x86_64 and Linux aarch64 (Raspberry Pi).

## Checking a machine

The binary runs single commands without OpenDeck, which helps find out why something does nothing:

```sh
opendeck-mediaknob --run level          # current volume
opendeck-mediaknob --run volume 5       # +5 %
opendeck-mediaknob --run mute
opendeck-mediaknob --run players        # MPRIS players (Linux) / open players and media key permission (macOS)
opendeck-mediaknob --run playpause [auto|spotify|music]
opendeck-mediaknob --run seek -10 [auto|spotify|music]
```

Set `MEDIAKNOB_DEBUG=1` for debug logging.

## Build

```sh
pip install ziglang && cargo install --locked cargo-zigbuild
rustup target add aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin
SDKROOT=/path/to/MacOSX13.3.sdk scripts/package.sh   # -> dist/opendeck-mediaknob.zip
```

On a Mac, `SDKROOT` is found with `xcrun`. Elsewhere use an SDK such as the one from [joseluisq/macosx-sdks](https://github.com/joseluisq/macosx-sdks); the mac binary links AppKit for the media keys.

## License

MIT
