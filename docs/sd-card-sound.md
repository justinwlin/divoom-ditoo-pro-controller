# SD card sound: how to make the Ditoo Pro speak without audio pairing

The Ditoo Pro plays audio files from a microSD card. The stock firmware lets a host select a track over the
control link. With one short file per sound, a program can make the speaker say "Your build is finished" or play
a jingle, with no Bluetooth audio pairing and no custom firmware.

This page records what we tested, how we built the [soundpack](../soundpack/README.md), and how to add your own
music. All results are from one Ditoo Pro on stock firmware, controlled from macOS over BLE.

## What we found

| # | Question | Result |
| --- | --- | --- |
| 1 | Can the control link stream audio? | No. No command sends audio data. Live audio needs Bluetooth audio or USB audio. |
| 2 | Can the control link upload a file to the card? | No. Copy the files with a card reader. |
| 3 | Which card format works? | FAT32 with an MBR partition table. A 4 GB card worked with no format step. |
| 4 | Which file types play? | MP3 and WAV. |
| 5 | How are track IDs assigned? | From 0, in file name order (`0000.mp3` is ID 0). Checked with 263 files. |
| 6 | Does the Ditoo report what it plays? | Yes. Status `0xb4` gives elapsed seconds, length, track ID, play flag, volume, play mode. |
| 7 | Is there a "play one track and stop" mode? | No. Modes are 1 = all in order, 2 = repeat one, 3 = random. |
| 8 | How fast is a command? | About 0.1 s for the acknowledgment. Sound starts about 0.2 to 0.5 s after the select command. |
| 9 | What happens when the card goes in? | The Ditoo starts playback by itself. Send pause. |

Opcodes:

| Opcode | Payload | Use |
| --- | --- | --- |
| `0x7d` | none | Track count, u16 LE. |
| `0x07` | first ID u16 LE, last ID u16 LE | Track list. Each reply: ID u16, name length u16, name in UTF-16. |
| `0x11` | track ID u16 LE | Select a track. Playback starts immediately. |
| `0x0a` | `00` | Pause. |
| `0xb9` | mode | Play mode. |
| `0xb4` | none | Status (9 bytes). |

Values 0 and 4 for the play mode changed to 1 and 3 when playback started.

## How to play one sound one time

1. Set play mode 2 (repeat one track). A late pause then never reaches a different sound.
2. Select the track.
3. Wait for the length of the sound.
4. Send pause.

Each file in the soundpack has 0.6 seconds of silence at the end, so the pause lands in silence. For a sequence
of words, select the next track when the current one ends, and send pause only after the last one.

## How we made the sounds

- **Voice.** A text-to-speech service made one MP3 for each line in `soundpack/tools/jessica_lines.tsv`. We trimmed
  the silence at both ends and set an equal loudness (`loudnorm=I=-16`).
- **Check.** We joined the shortest words into one file and ran speech recognition on it. Two words were cut by
  the silence trim (threshold -45 dB). A gentler trim (-60 dB) corrected them.
- **Jingles.** `soundpack/tools/make_jingles.py` is a small synthesizer. It needs no service.
- **Loops.** The rain loop is filtered noise from ffmpeg. Play a loop with mode 2 and no pause.
- **Order.** `soundpack/tools/build_pack.py` gives each file an 8.3 name (`0000.mp3`) and writes `manifest.json`.

A word kit (numbers, units, weekdays, nouns, connectors) lets a program build phrases such as "Timer, 25 minutes
left" from separate tracks. The gap between the words is audible. A full sentence in one file sounds better.

## Add your own music

The soundpack has no music. To add short pieces of songs that you own:

1. Cut the piece with a fade in and a fade out, and add the 0.6 seconds of silence:

   ```sh
   ffmpeg -ss 48.0 -t 13.5 -i song.mp3 \
     -af "afade=t=in:d=0.12,afade=t=out:st=12.8:d=0.7,loudnorm=I=-16:TP=-1.5,apad=pad_dur=0.6" \
     -ar 44100 -b:a 160k 0257.mp3
   ```

2. Name the files after the last pack file: `0257.mp3`, `0258.mp3`, and so on.
3. Copy them to the card after the pack files.
4. List them in `soundpack/manifest.local.json`, in the same format as `manifest.json`. Git ignores that file.

   ```json
   {"sounds": [{"id": 257, "file": "0257.mp3", "name": "my_song", "category": "song", "text": "My song",
                "seconds": 13.5, "loop": false}]}
   ```

5. Play it: `ditoo_play.py my_song`.

To find a good part of a song, a speech recognition tool with timestamps gives the start time of each line. Cut at
a quiet moment between two phrases.

## Limits

- One track plays at a time. Sounds cannot mix.
- New files must go after the existing ones, or the IDs in the manifest change.
- Other audio files on the card take track IDs. Keep the card for this use only.
- Results are from one device. Other firmware versions can differ.
