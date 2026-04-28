# media-control-bridge

A small protocol-translation bridge for remote media controls.

Current scope:

- Music is played on the **controlled endpoint**.
- A controlled endpoint runs `serve` and listens on TCP.
- One or more controller endpoints connect to it.
- Linux controller: exposes an MPRIS player, so KDE Plasma / GNOME / `playerctl` can control the remote player.
- Windows controller: exposes a Windows SMTC entry, so Windows quick settings / media keys can control the remote player.
- Linux controlled endpoint: controls a local MPRIS player through `playerctl`, or arbitrary commands through `cmd` target.
- Windows controlled endpoint: controls the current Windows SMTC session through GSMTC.

This repo contains:

- Rust implementation: `rust/media-control-bridge`
- Python prototype: `python/media_bridge.py`
- x86_64 build scripts and a GitHub Actions workflow.

## Why this exists

Linux desktops usually expose media control through MPRIS over D-Bus. Windows exposes media sessions through System Media Transport Controls (SMTC) / GlobalSystemMediaTransportControlsSessionManager (GSMTC). Wine/Proton/umu generally does not automatically translate a Windows app's SMTC session into Linux MPRIS, so this bridge translates control commands over a simple TCP protocol.

## Protocol

JSON Lines over TCP. One message per line.

```json
{"v":1,"kind":"command","token":"optional-shared-secret","command":"play_pause"}
```

Supported commands:

- `play`
- `pause`
- `play_pause`
- `stop`
- `next`
- `previous`
- `status`

Server replies:

```json
{"v":1,"kind":"ack","ok":true,"message":"ok","state":{"playback":"Playing","title":"Song","artist":"Artist","album":"Album"}}
```

## Rust usage

### Linux controlled endpoint: control an MPRIS player

Install `playerctl` first.

```bash
media-control-bridge serve \
  --listen 0.0.0.0:17777 \
  --token CHANGE_ME \
  --target mpris \
  --player amberol
```

If you want to target the active/default MPRIS player, omit `--player`.

### Linux controlled endpoint: control a Wine app by shell commands

For Wine apps that do not expose MPRIS, use `cmd` target. Example using media keys:

```bash
media-control-bridge serve \
  --listen 0.0.0.0:17777 \
  --token CHANGE_ME \
  --target cmd \
  --cmd-play-pause 'ydotool key 164:1 164:0' \
  --cmd-next 'ydotool key 163:1 163:0' \
  --cmd-previous 'ydotool key 165:1 165:0'
```

On X11 you can also use `xdotool key XF86AudioPlay` / `XF86AudioNext` / `XF86AudioPrev`.

### Windows controlled endpoint: control current Windows media session

```powershell
media-control-bridge.exe serve `
  --listen 0.0.0.0:17777 `
  --token CHANGE_ME `
  --target smtc
```

This uses GSMTC to control the current Windows media session.

### Linux controller endpoint: expose remote as KDE/GNOME MPRIS player

```bash
media-control-bridge mpris-client \
  --connect 192.168.1.20:17777 \
  --token CHANGE_ME \
  --name WineCloudMusic
```

Then KDE's media widget, GNOME shell, or `playerctl -p WineCloudMusic play-pause` will send commands to the controlled endpoint.

### Windows controller endpoint: expose remote as Windows SMTC player

```powershell
media-control-bridge.exe smtc-client `
  --connect 192.168.1.20:17777 `
  --token CHANGE_ME `
  --name RemoteMusic
```

Windows media overlay / media keys should send commands through the bridge.

### Direct CLI test

```bash
media-control-bridge send --connect 127.0.0.1:17777 --token CHANGE_ME --command play_pause
media-control-bridge send --connect 127.0.0.1:17777 --token CHANGE_ME --command status
```

## Security notes

- Do not expose this to the public Internet.
- Use `--listen 127.0.0.1:17777` for local-only use.
- If you bind to `0.0.0.0`, use `--token` and a firewall.
- The `cmd` target executes shell commands; only run it on trusted machines.

## Building x86_64

### Linux x86_64

```bash
cd rust/media-control-bridge
cargo build --release
```

Output:

```text
target/release/media-control-bridge
```

### Windows x86_64

On Windows with Rust and the Windows SDK installed:

```powershell
cd rust\media-control-bridge
cargo build --release --target x86_64-pc-windows-msvc
```

Output:

```text
target\x86_64-pc-windows-msvc\release\media-control-bridge.exe
```

There is also a GitHub Actions workflow in `.github/workflows/build.yml` that builds both Linux x86_64 and Windows x86_64 artifacts.

## Python prototype

The Python implementation is useful for quick testing and Linux MPRIS bridging. The Rust version is the intended implementation for Windows SMTC provider mode.

Linux dependencies:

```bash
python -m pip install dbus-next
sudo pacman -S playerctl
```

Windows target-control dependencies:

```powershell
py -m pip install winsdk
```

Example:

```bash
python python/media_bridge.py serve --listen 127.0.0.1:17777 --target mpris
python python/media_bridge.py mpris-client --connect 127.0.0.1:17777 --name RemoteMusic
```

## No-argument behavior

Running `media-control-bridge` with no arguments intentionally does not start a server or connect to anything. It prints a warning plus the same help text as `--help`, then exits. Running `media-control-bridge --help` prints the help text without the warning.

The Python version follows the same rule:

```bash
python python/media_bridge.py          # warning + help
python python/media_bridge.py --help   # help only
```
