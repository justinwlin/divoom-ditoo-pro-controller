#!/usr/bin/env python3
"""Play sounds from the soundpack SD card on a Divoom Ditoo Pro over BLE (works on macOS, stock firmware).

usage:
  uv run --with bleak ditoo_play.py NAME [NAME ...]     play the sounds in sequence, then pause
  uv run --with bleak ditoo_play.py --loop a_rain       start an ambient loop and leave it on
  uv run --with bleak ditoo_play.py --stop              pause playback
  uv run --with bleak ditoo_play.py --list              print the sound names

Example: ditoo_play.py j_done e01_build_finished
         ditoo_play.py w_timer n25 w_minutes w_left
"""
import asyncio, datetime, json, os, sys, time
from bleak import BleakClient, BleakScanner

WRITE = "49535343-8841-43f4-a8d4-ecbe34729bb3"
NOTIFY = "49535343-1e4d-4bd9-ba61-23c647249616"
SET_VOLUME_QUERY, PLAY_PAUSE, SELECT_TRACK, PLAY_MODE = 0x09, 0x0A, 0x11, 0xB9
REPEAT_ONE = 2  # a late pause then never reaches a different sound


def envelope(payload, seq):
    wire = bytearray([0xFE, 0xEF, 0xAA, 0x55])
    wire += (len(payload) + 7).to_bytes(2, "little") + seq.to_bytes(2, "big") + b"\x00\x00\x00" + payload
    return bytes(wire + (sum(wire[4:]) & 0xFFFF).to_bytes(2, "little"))


class Ditoo:
    def __init__(self, client):
        self.client, self.acks = client, asyncio.Queue()
        self.buffer, self.seq = bytearray(), 0x0101 + int(time.time()) % 200

    def on_notify(self, _, data):
        self.buffer.extend(data)
        while len(self.buffer) >= 3:
            size = int.from_bytes(self.buffer[1:3], "little") + 4
            if self.buffer[0] != 1 or len(self.buffer) < size:
                break
            frame = bytes(self.buffer[:size])
            del self.buffer[:size]
            if frame[4] == 0x33:
                self.acks.put_nowait(frame[6])

    async def write(self, payload, seq):
        wire = envelope(payload, seq)
        for i in range(0, len(wire), 20):
            await self.client.write_gatt_char(WRITE, wire[i:i + 20], response=True)
            await asyncio.sleep(0.05)

    async def command(self, opcode, payload=b""):
        seq, self.seq = self.seq, 0x0101 if self.seq >= 0x01FF else self.seq + 1
        await self.write(bytes([opcode]) + payload, seq)
        try:
            while await asyncio.wait_for(self.acks.get(), 2) != seq & 0xFF:
                pass
        except asyncio.TimeoutError:
            sys.exit(f"The Ditoo did not acknowledge command 0x{opcode:02x}")

    async def start(self):
        await self.client.start_notify(NOTIFY, self.on_notify)
        now = datetime.datetime.now()
        clock = '{"Command":"Device\\/SetUTC","Utc":%d,"Time":"%s"}' % (time.time(), now.strftime("%Y-%m-%d %H:%M:%S"))
        await self.write(clock.encode(), 1)  # the Ditoo expects this at the start of each BLE session
        await asyncio.sleep(1.2)
        await self.command(SET_VOLUME_QUERY)  # read-only primer, see src/transport.rs


async def main():
    args = sys.argv[1:]
    here = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
    pack = json.load(open(os.path.join(here, "manifest.json")))
    local = os.path.join(here, "manifest.local.json")  # your own extra tracks, not in git
    if os.path.exists(local):
        pack["sounds"] += json.load(open(local))["sounds"]
    sounds = {s["name"]: s for s in pack["sounds"]}
    if not args or args[0] == "--list":
        print("\n".join(f'{s["id"]:4d}  {s["name"]:22s} {s["category"]:12s} {s["text"]}' for s in pack["sounds"]))
        return
    loop = args[0] == "--loop"
    names = [a for a in args if not a.startswith("--")]
    missing = [n for n in names if n not in sounds]
    if missing:
        sys.exit("Unknown sound: " + ", ".join(missing))
    device = await BleakScanner.find_device_by_filter(
        lambda d, adv: "ditoo" in (d.name or adv.local_name or "").lower(), timeout=15)
    if not device:
        sys.exit("No Ditoo found. Is it on and near?")
    async with BleakClient(device, timeout=20) as client:
        ditoo = Ditoo(client)
        await ditoo.start()
        if args[0] == "--stop":
            await ditoo.command(PLAY_PAUSE, b"\x00")
            return
        await ditoo.command(PLAY_MODE, bytes([REPEAT_ONE]))
        for name in names:
            started = time.time()
            await ditoo.command(SELECT_TRACK, sounds[name]["id"].to_bytes(2, "little"))
            if loop:
                return
            await asyncio.sleep(max(0, sounds[name]["seconds"] + 0.25 - (time.time() - started)))
        await ditoo.command(PLAY_PAUSE, b"\x00")


asyncio.run(main())
