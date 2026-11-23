# MCP server

`divoom-ditoo-pro-controller mcp` runs a Model Context Protocol server on stdin and stdout. An AI agent can then
use the display and the speaker as tools.

## Start

```sh
divoom-ditoo-pro-controller mcp --pack /path/to/soundpack --art /path/to/art
```

Use absolute paths, because an MCP client starts the server in its own folder. `DITOO_SOUNDPACK` and `DITOO_ART`
work in place of the options. On macOS, add `--name PART` if the BLE name of your device does not contain
`DitooPro-Light`.

Example for Claude Code:

```sh
claude mcp add ditoo -- /path/to/divoom-ditoo-pro-controller mcp \
  --pack /path/to/soundpack --art /path/to/art
```

## Tools

| Tool | Arguments | Result |
| --- | --- | --- |
| `list_sounds` | `category` (optional) | Names, categories and texts of the soundpack |
| `play_sound` | `names` (list) | Plays the sounds in sequence, one time each |
| `loop_sound` | `name` | Starts a sound in repeat mode |
| `stop_sound` | none | Pauses playback |
| `sound_status` | none | Track, elapsed seconds, play flag, volume |
| `list_art` | none | Names in the art folder |
| `show_art` | `name` | Shows an animation or image by name |
| `show_image` | `path` | Shows an image or GIF file from disk |
| `show_card` | `top`, `bottom`, `color`, `bottom_color`, `background` | Two short lines of text |
| `show_gauge` | `label`, `value`, `max`, `color` | A label above a progress bar |
| `show_metric` | `label`, `value`, `color` | A small label above a value |
| `set_brightness` | `percent` | Display brightness, 0 to 100 |
| `get_volume` | none | Speaker volume, 0 to 15 |
| `set_volume` | `level` | Sets the speaker volume |

## Behavior

- Messages are JSON-RPC 2.0, one JSON object per line. Logs go to stderr.
- Each tool call opens its own device connection, as a command line call does. On macOS over BLE, that is about
  3 seconds for each call.
- The server handles one call at a time.
- A tool error comes back as a normal tool result with `isError` true and a message, for example an unknown
  sound name.

## Tested

On macOS with a Ditoo Pro on stock firmware: `initialize`, `tools/list`, `show_card`, `play_sound`,
`sound_status`, and an unknown sound name. The other tools use the same functions as the command line commands.
