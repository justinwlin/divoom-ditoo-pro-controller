#!/usr/bin/env python3
"""Build the SD card folder and manifest.json from the source sounds.

usage: build_pack.py SOURCE_DIR PACK_DIR
SOURCE_DIR has jessica/*.mp3 (from make_batch.py), batch1/*.wav (from make_jingles.py) and batch2/a*.mp3 (loops).
Card files get 8.3 names (0000.mp3, 0001.mp3, ...). The track ID on the Ditoo is the position in name order.
New sounds must go at the end, so that existing IDs do not change.
"""
import json, os, subprocess, sys

src, pack = sys.argv[1], sys.argv[2]
PAD = 0.6  # seconds of silence after each one-shot sound, so a pause command lands in silence
JINGLES = [("j_done", "j03_done_bell", "Task done"), ("j_message", "j05_message_ping", "New message"),
           ("j_error", "j07_error_bell", "Error"), ("j_goodnight", "j10_goodnight", "Good night")]
LOOPS = [("a_rain", "a01_rain_loop", "Rain loop"), ("a_cafe", "a02_cafe_ai", "Cafe loop")]
CATEGORY = {"e": "event", "p": "personality", "n": "number", "w": "word", "d": "weekday", "m": "month",
            "o": "ordinal", "wx": "weather", "c": "noun", "k": "connector"}

def seconds(path):
    out = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", path],
                         capture_output=True, text=True, check=True).stdout
    return round(float(out), 2)

items = [dict(name=n, category="jingle", text=t, source=f"{src}/batch1/{f}.wav", loop=False) for n, f, t in JINGLES]
for row in open(os.path.join(os.path.dirname(__file__), "jessica_lines.tsv")):
    name, text = row.rstrip("\n").split("\t")
    prefix = name.split("_")[0].rstrip("0123456789")
    items.append(dict(name=name, category=CATEGORY[prefix], text=text, source=f"{src}/jessica/{name}.mp3", loop=False))
items += [dict(name=n, category="ambient", text=t, source=f"{src}/batch2/{f}.mp3", loop=True) for n, f, t in LOOPS]

os.makedirs(f"{pack}/card", exist_ok=True)
manifest = []
for i, it in enumerate(items):
    dst = f"{pack}/card/{i:04d}.mp3"
    af = [] if it["loop"] else ["-af", f"apad=pad_dur={PAD}"]
    ch, rate = ("2", "128k") if it["loop"] else ("1", "96k")
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", it["source"], *af, "-ar", "44100", "-ac", ch, "-b:a", rate,
                    "-map_metadata", "-1", "-id3v2_version", "0", dst], check=True)
    manifest.append(dict(id=i, file=f"{i:04d}.mp3", name=it["name"], category=it["category"], text=it["text"],
                         seconds=seconds(it["source"]), loop=it["loop"]))
json.dump(dict(voice="ElevenLabs premade voice Jessica", pad_seconds=PAD, sounds=manifest),
          open(f"{pack}/manifest.json", "w"), indent=1)
print(len(manifest), "sounds")
