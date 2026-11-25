# Keyboard Shortcut

An OpenDeck plugin that presses a keyboard shortcut (any key combo with Cmd, Ctrl, Alt or Shift) from a key or a knob. Works on macOS and Linux, including Raspberry Pi (ARM64), under X11 and Wayland.

- **Record it**: click *Record* and press the shortcut, or type it like `cmd+shift+4`.
- **The key shows the combo** on a keycap (`⇧⌘4` on a Mac, `Ctrl+Alt+T` on Linux). Turn this off to use your own icon or title.
- **Sequences**: `ctrl+k ctrl+s` presses one combo after the other (*+ Then…* records the next one).
- **Hold**: keeps the combo down for as long as you hold the key, for push-to-talk and the like. Modifier-only combos like `ctrl` work too.
- **Repeat**: tap the shortcut several times per press.
- **Knobs**: one shortcut for turning left, one for turning right (pressed once per step), one for pressing.
- **Test** sends the shortcut after 3 seconds, so you can click into the app it's meant for first.

## Install

Download `opendeck-shortcut.zip` and install it in OpenDeck with *Plugins > Install from file*. Then drag **Shortcut** from the *Keyboard Shortcut* category onto a key or knob.

### macOS: allow Accessibility

macOS only delivers key presses from apps you allow. The first time a shortcut runs, macOS asks whether OpenDeck may control the computer. Allow it under **System Settings > Privacy & Security > Accessibility**, then restart OpenDeck. The plugin's settings show a warning and an *Open Accessibility settings* button while this is missing.

The permission belongs to the OpenDeck app, not to the plugin, and macOS ties it to the app's exact build. If you run your own OpenDeck build, or update it, an older entry in the list (even switched on) no longer counts: select it, remove it with the minus button, and add the OpenDeck you are running now (or press a shortcut key again to get the prompt).

### Linux and Raspberry Pi: allow /dev/uinput

On Linux the plugin types through a virtual keyboard (`/dev/uinput`), so it works on X11, Wayland (including Raspberry Pi OS's labwc) and without picking a display server. By default only root may create one. The zip contains `70-opendeck-shortcut.rules`; install it once:

```sh
cd ~/.config/opendeck/plugins/io.github.gauravk-in.shortcut.sdPlugin   # or wherever OpenDeck put it
sudo cp 70-opendeck-shortcut.rules /etc/udev/rules.d/
echo uinput | sudo tee /etc/modules-load.d/uinput.conf
sudo modprobe uinput && sudo udevadm control --reload-rules && sudo udevadm trigger --name-match=uinput
sudo usermod -aG input "$USER"
```

Log out and in again (or reboot). The plugin's settings say *Ready (uinput)* once it works.

## Writing shortcuts

- Join keys with `+`; separate combos pressed one after another with a space.
- Modifiers: `ctrl`, `alt` (also `option`), `shift`, `cmd` (also `command`, `super`, `win`; the Super/Windows key on Linux).
- Keys: `a`-`z`, `0`-`9`, `f1`-`f20`, `enter`, `tab`, `space`, `backspace`, `delete` (forward delete), `escape`, `up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`, `insert`, `capslock`, `minus`, `equal`, `leftbracket`, `rightbracket`, `backslash`, `semicolon`, `quote`, `comma`, `period`, `slash`, `grave`, `section` (the extra key on ISO keyboards), `kp0`-`kp9`, `kpplus`, `kpminus`, `kpmultiply`, `kpdivide`, `kpdecimal`, `kpenter`, `kpequal`. Most punctuation can also be typed as itself (`cmd+[`), and `cmd++` means `shift+cmd+equal`.
- Keys are physical positions named after the US layout, like the browser's `KeyboardEvent.code`. On another layout, record the shortcut rather than typing it, and the right key is pressed.
- Some shortcuts never reach the settings page because macOS or OpenDeck handles them first (Cmd+Tab, Cmd+Space, Cmd+Q, Cmd+W). Type those.

## Command line

```sh
opendeck-shortcut-mac --check                      # can it send keys?
opendeck-shortcut-mac --send 'cmd+shift+4' 3       # send a shortcut after 3 seconds
opendeck-shortcut-mac --image 'cmd+shift+4' k.png  # draw the key image
```

`--check` and `--send` from a terminal test the terminal app's permission on macOS, not OpenDeck's.

## Build

```sh
pip install ziglang && cargo install --locked cargo-zigbuild
rustup target add aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin
SDKROOT=/path/to/MacOSX13.3.sdk scripts/package.sh   # -> dist/opendeck-shortcut.zip
```

`cargo test` runs the parser tests. Setting `OPENDECK_SHORTCUT_DRY_RUN=/tmp/keys.log` makes the plugin write key events to that file instead of sending them.

## How it works

- **macOS**: `CGEventCreateKeyboardEvent` + `CGEventPost` to the HID event tap, modifier keys pressed and released around the key with matching flags. Permission is checked with `CGPreflightPostEventAccess` and requested with `CGRequestPostEventAccess`.
- **Linux**: a uinput device named "OpenDeck Keyboard Shortcut", created when the first Shortcut key appears and kept open, with evdev key codes.
