#!/usr/bin/env python3
"""
media_bridge.py - Python prototype for media-control-bridge.

Implemented:
- TCP JSONL protocol server/client.
- Linux controlled endpoint via playerctl/MPRIS.
- Linux controller endpoint exposing MPRIS via dbus-next.
- Windows controlled endpoint via GSMTC using winsdk package.

Not implemented in Python:
- Windows controller endpoint that creates a real SMTC provider entry. Use the Rust
  smtc-client for that mode; WinRT SMTC provider creation from a console process
  is much more reliable in Rust/C++/C# than Python.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import os
import platform
import shlex
import subprocess
import sys
from dataclasses import dataclass, asdict
from typing import Any, Dict, Optional

PROTOCOL_VERSION = 1


@dataclass
class PlaybackState:
    playback: Optional[str] = None
    title: Optional[str] = None
    artist: Optional[str] = None
    album: Optional[str] = None


def normalize_command(command: str) -> str:
    command = command.lower().replace("-", "_")
    aliases = {
        "playpause": "play_pause",
        "toggle": "play_pause",
        "prev": "previous",
    }
    command = aliases.get(command, command)
    allowed = {"play", "pause", "play_pause", "stop", "next", "previous", "status"}
    if command not in allowed:
        raise ValueError(f"unknown command: {command}")
    return command


def make_command(command: str, token: Optional[str]) -> Dict[str, Any]:
    return {"v": PROTOCOL_VERSION, "kind": "command", "token": token, "command": normalize_command(command)}


def make_ack(ok: bool, message: str, state: Optional[PlaybackState] = None) -> Dict[str, Any]:
    return {
        "v": PROTOCOL_VERSION,
        "kind": "ack",
        "ok": ok,
        "message": message,
        "state": asdict(state) if state else None,
    }


async def send_command(connect: str, token: Optional[str], command: str) -> Optional[Dict[str, Any]]:
    host, port_s = connect.rsplit(":", 1)
    reader, writer = await asyncio.open_connection(host, int(port_s))
    writer.write((json.dumps(make_command(command, token), ensure_ascii=False) + "\n").encode())
    await writer.drain()
    line = await reader.readline()
    writer.close()
    await writer.wait_closed()
    if not line:
        raise RuntimeError("server closed connection without reply")
    reply = json.loads(line.decode())
    if not reply.get("ok"):
        raise RuntimeError(reply.get("message", "server returned error"))
    return reply.get("state")


class Target:
    async def handle(self, command: str) -> Optional[PlaybackState]:
        raise NotImplementedError


class CmdTarget(Target):
    def __init__(self, mapping: Dict[str, Optional[str]]) -> None:
        self.mapping = mapping

    async def handle(self, command: str) -> Optional[PlaybackState]:
        script = self.mapping.get(command)
        if not script:
            raise RuntimeError(f"no command mapping configured for {command}")
        proc = await asyncio.create_subprocess_shell(
            script,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        out, err = await proc.communicate()
        if proc.returncode != 0:
            raise RuntimeError(f"command failed: {script}\nstdout={out.decode()}\nstderr={err.decode()}")
        if command == "status":
            text = out.decode(errors="replace").strip()
            return PlaybackState(playback=text or None)
        return None


class LinuxMprisTarget(Target):
    def __init__(self, player: Optional[str]) -> None:
        self.player = player

    async def _playerctl(self, *args: str, check: bool = True) -> str:
        cmd = ["playerctl"]
        if self.player:
            cmd += ["-p", self.player]
        cmd += list(args)
        proc = await asyncio.create_subprocess_exec(
            *cmd,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        out, err = await proc.communicate()
        if check and proc.returncode != 0:
            raise RuntimeError(f"playerctl failed: {' '.join(shlex.quote(x) for x in cmd)}\n{err.decode()}")
        return out.decode(errors="replace").strip()

    async def handle(self, command: str) -> Optional[PlaybackState]:
        command = normalize_command(command)
        if command == "play":
            await self._playerctl("play")
        elif command == "pause":
            await self._playerctl("pause")
        elif command == "play_pause":
            await self._playerctl("play-pause")
        elif command == "stop":
            await self._playerctl("stop")
        elif command == "next":
            await self._playerctl("next")
        elif command == "previous":
            await self._playerctl("previous")
        elif command == "status":
            playback = await self._playerctl("status", check=False)
            title = await self._playerctl("metadata", "xesam:title", check=False)
            artist = await self._playerctl("metadata", "xesam:artist", check=False)
            album = await self._playerctl("metadata", "xesam:album", check=False)
            return PlaybackState(playback or None, title or None, artist or None, album or None)
        return None


class WindowsSmtcTarget(Target):
    async def handle(self, command: str) -> Optional[PlaybackState]:
        if platform.system() != "Windows":
            raise RuntimeError("Windows SMTC target is only available on Windows")
        from winsdk.windows.media.control import GlobalSystemMediaTransportControlsSessionManager

        manager = await GlobalSystemMediaTransportControlsSessionManager.request_async()
        session = manager.get_current_session()
        if session is None:
            raise RuntimeError("no current Windows media session")
        command = normalize_command(command)
        if command == "play":
            await session.try_play_async()
        elif command == "pause":
            await session.try_pause_async()
        elif command == "play_pause":
            await session.try_toggle_play_pause_async()
        elif command == "stop":
            await session.try_stop_async()
        elif command == "next":
            await session.try_skip_next_async()
        elif command == "previous":
            await session.try_skip_previous_async()
        elif command == "status":
            info = session.get_playback_info()
            status = str(info.playback_status).split(".")[-1]
            props = await session.try_get_media_properties_async()
            return PlaybackState(
                playback=status,
                title=getattr(props, "title", None),
                artist=getattr(props, "artist", None),
                album=getattr(props, "album_title", None),
            )
        return None


async def run_server(args: argparse.Namespace) -> None:
    if args.target == "mpris":
        target: Target = LinuxMprisTarget(args.player)
    elif args.target == "smtc":
        target = WindowsSmtcTarget()
    elif args.target == "cmd":
        mapping = {
            "play": args.cmd_play,
            "pause": args.cmd_pause,
            "play_pause": args.cmd_play_pause,
            "stop": args.cmd_stop,
            "next": args.cmd_next,
            "previous": args.cmd_previous,
            "status": args.cmd_status,
        }
        if not any(mapping.values()):
            raise SystemExit("--target cmd requires at least one --cmd-* mapping")
        target = CmdTarget(mapping)
    else:
        raise SystemExit(f"unknown target: {args.target}")

    host, port_s = args.listen.rsplit(":", 1)

    async def handle_client(reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        try:
            while True:
                line = await reader.readline()
                if not line:
                    break
                try:
                    msg = json.loads(line.decode())
                    if args.token and msg.get("token") != args.token:
                        reply = make_ack(False, "unauthorized")
                    elif msg.get("kind") != "command":
                        reply = make_ack(False, "expected command")
                    else:
                        state = await target.handle(normalize_command(msg["command"]))
                        reply = make_ack(True, "ok", state)
                except Exception as exc:  # noqa: BLE001 - this is a CLI bridge, report errors to client
                    reply = make_ack(False, str(exc))
                writer.write((json.dumps(reply, ensure_ascii=False) + "\n").encode())
                await writer.drain()
        finally:
            writer.close()
            await writer.wait_closed()

    server = await asyncio.start_server(handle_client, host, int(port_s))
    print(f"listening on {args.listen}", file=sys.stderr)
    async with server:
        await server.serve_forever()


async def run_mpris_client(args: argparse.Namespace) -> None:
    if platform.system() != "Linux":
        raise SystemExit("mpris-client is only available on Linux")
    try:
        from dbus_next.aio import MessageBus
        from dbus_next.service import ServiceInterface, method, dbus_property
        from dbus_next import Variant
    except ImportError as exc:
        raise SystemExit("Install dependency first: python -m pip install dbus-next") from exc

    class RootInterface(ServiceInterface):
        def __init__(self, identity: str) -> None:
            super().__init__("org.mpris.MediaPlayer2")
            self.identity = identity

        @method()
        def Raise(self) -> None:  # noqa: N802 - D-Bus method name
            return None

        @method()
        def Quit(self) -> None:  # noqa: N802
            return None

        @dbus_property()
        def CanQuit(self) -> "b":  # type: ignore[name-defined]  # noqa: F821, N802
            return False

        @dbus_property()
        def Fullscreen(self) -> "b":  # type: ignore[name-defined]  # noqa: F821, N802
            return False

        @dbus_property()
        def CanSetFullscreen(self) -> "b":  # type: ignore[name-defined]  # noqa: F821, N802
            return False

        @dbus_property()
        def CanRaise(self) -> "b":  # type: ignore[name-defined]  # noqa: F821, N802
            return False

        @dbus_property()
        def HasTrackList(self) -> "b":  # type: ignore[name-defined]  # noqa: F821, N802
            return False

        @dbus_property()
        def Identity(self) -> "s":  # type: ignore[name-defined]  # noqa: F821, N802
            return self.identity

        @dbus_property()
        def DesktopEntry(self) -> "s":  # type: ignore[name-defined]  # noqa: F821, N802
            return "media-control-bridge"

        @dbus_property()
        def SupportedUriSchemes(self) -> "as":  # type: ignore[name-defined]  # noqa: F821, N802
            return []

        @dbus_property()
        def SupportedMimeTypes(self) -> "as":  # type: ignore[name-defined]  # noqa: F821, N802
            return []

    class PlayerInterface(ServiceInterface):
        def __init__(self) -> None:
            super().__init__("org.mpris.MediaPlayer2.Player")
            self.playback_status = "Playing"

        async def _send(self, cmd: str) -> None:
            try:
                await send_command(args.connect, args.token, cmd)
            except Exception as exc:  # noqa: BLE001
                print(f"send failed: {exc}", file=sys.stderr)

        @method()
        async def Next(self) -> None:  # noqa: N802
            await self._send("next")

        @method()
        async def Previous(self) -> None:  # noqa: N802
            await self._send("previous")

        @method()
        async def Pause(self) -> None:  # noqa: N802
            await self._send("pause")

        @method()
        async def PlayPause(self) -> None:  # noqa: N802
            await self._send("play_pause")

        @method()
        async def Stop(self) -> None:  # noqa: N802
            await self._send("stop")

        @method()
        async def Play(self) -> None:  # noqa: N802
            await self._send("play")

        @method()
        def Seek(self, Offset: "x") -> None:  # type: ignore[name-defined]  # noqa: F821, N803
            return None

        @method()
        def SetPosition(self, TrackId: "o", Position: "x") -> None:  # type: ignore[name-defined]  # noqa: F821, N803
            return None

        @method()
        def OpenUri(self, Uri: "s") -> None:  # type: ignore[name-defined]  # noqa: F821, N803
            return None

        @dbus_property()
        def PlaybackStatus(self) -> "s":  # type: ignore[name-defined]  # noqa: F821, N802
            return self.playback_status

        @dbus_property()
        def LoopStatus(self) -> "s":  # type: ignore[name-defined]  # noqa: F821, N802
            return "None"

        @dbus_property()
        def Rate(self) -> "d":  # type: ignore[name-defined]  # noqa: F821, N802
            return 1.0

        @dbus_property()
        def Shuffle(self) -> "b":  # type: ignore[name-defined]  # noqa: F821, N802
            return False

        @dbus_property()
        def Metadata(self) -> "a{sv}":  # type: ignore[name-defined]  # noqa: F821, N802
            return {
                "mpris:trackid": Variant("o", "/org/mpris/MediaPlayer2/Track/0"),
                "xesam:title": Variant("s", args.name),
                "xesam:artist": Variant("as", ["media-control-bridge"]),
            }

        @dbus_property()
        def Volume(self) -> "d":  # type: ignore[name-defined]  # noqa: F821, N802
            return 1.0

        @dbus_property()
        def Position(self) -> "x":  # type: ignore[name-defined]  # noqa: F821, N802
            return 0

        @dbus_property()
        def MinimumRate(self) -> "d":  # type: ignore[name-defined]  # noqa: F821, N802
            return 1.0

        @dbus_property()
        def MaximumRate(self) -> "d":  # type: ignore[name-defined]  # noqa: F821, N802
            return 1.0

        @dbus_property()
        def CanGoNext(self) -> "b":  # type: ignore[name-defined]  # noqa: F821, N802
            return True

        @dbus_property()
        def CanGoPrevious(self) -> "b":  # type: ignore[name-defined]  # noqa: F821, N802
            return True

        @dbus_property()
        def CanPlay(self) -> "b":  # type: ignore[name-defined]  # noqa: F821, N802
            return True

        @dbus_property()
        def CanPause(self) -> "b":  # type: ignore[name-defined]  # noqa: F821, N802
            return True

        @dbus_property()
        def CanSeek(self) -> "b":  # type: ignore[name-defined]  # noqa: F821, N802
            return False

        @dbus_property()
        def CanControl(self) -> "b":  # type: ignore[name-defined]  # noqa: F821, N802
            return True

    safe_name = "".join(ch if ch.isalnum() or ch == "_" else "_" for ch in args.name)
    if not safe_name or safe_name[0].isdigit():
        safe_name = "_" + safe_name
    bus_name = f"org.mpris.MediaPlayer2.py_mediabridge_{safe_name}"
    bus = await MessageBus().connect()
    bus.export("/org/mpris/MediaPlayer2", RootInterface(args.name))
    bus.export("/org/mpris/MediaPlayer2", PlayerInterface())
    await bus.request_name(bus_name)
    print(f"MPRIS client registered as {bus_name}", file=sys.stderr)
    await asyncio.Event().wait()


async def main_async() -> None:
    parser = argparse.ArgumentParser(description="Python prototype for media-control-bridge")
    sub = parser.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("serve")
    p.add_argument("--listen", default="127.0.0.1:17777")
    p.add_argument("--token")
    p.add_argument("--target", choices=["mpris", "smtc", "cmd"], required=True)
    p.add_argument("--player")
    p.add_argument("--cmd-play")
    p.add_argument("--cmd-pause")
    p.add_argument("--cmd-play-pause")
    p.add_argument("--cmd-stop")
    p.add_argument("--cmd-next")
    p.add_argument("--cmd-previous")
    p.add_argument("--cmd-status")

    p = sub.add_parser("mpris-client")
    p.add_argument("--connect", required=True)
    p.add_argument("--token")
    p.add_argument("--name", default="RemoteMusic")

    p = sub.add_parser("smtc-client")
    p.add_argument("--connect", required=True)
    p.add_argument("--token")
    p.add_argument("--name", default="RemoteMusic")

    p = sub.add_parser("send")
    p.add_argument("--connect", required=True)
    p.add_argument("--token")
    p.add_argument("--command", required=True)

    if len(sys.argv) == 1:
        print("warning: no subcommand was supplied; printing help. Use serve, mpris-client, smtc-client, or send.\n", file=sys.stderr)
        parser.print_help()
        raise SystemExit(2)

    args = parser.parse_args()
    if args.cmd == "serve":
        await run_server(args)
    elif args.cmd == "mpris-client":
        await run_mpris_client(args)
    elif args.cmd == "smtc-client":
        raise SystemExit("Python smtc-client is not implemented; use Rust smtc-client on Windows.")
    elif args.cmd == "send":
        state = await send_command(args.connect, args.token, args.command)
        if state:
            print(json.dumps(state, indent=2, ensure_ascii=False))
        else:
            print("ok")


def main() -> None:
    try:
        asyncio.run(main_async())
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
