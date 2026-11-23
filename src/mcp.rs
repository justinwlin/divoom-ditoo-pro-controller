//! A Model Context Protocol server on stdin and stdout, so an AI agent can use the display and the speaker.
//!
//! Messages are JSON-RPC 2.0, one JSON object per line. Each tool call opens its own device connection, as a
//! command line call does. Logs go to stderr; stdout carries only protocol messages.

use std::error::Error;
use std::io::Write;
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::{art, canvas, sound, Address};

pub struct Config {
  pub address: Address,
  pub pack: PathBuf,
  pub art: PathBuf,
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
  json!({
    "name": name,
    "description": description,
    "inputSchema": { "type": "object", "properties": properties, "required": required },
  })
}

pub fn tools() -> Value {
  let color = json!({ "type": "string", "description": "CSS color name or hex, for example lime or #ff8800" });
  json!([
    tool("list_sounds", "List the sound names on the SD card soundpack. Use a category to keep the list short: jingle, event, personality, number, word, weekday, month, ordinal, weather, noun, connector, ambient, song.",
      json!({ "category": { "type": "string" } }), &[]),
    tool("play_sound", "Play one or more soundpack sounds in sequence through the speaker, one time each. Example: [\"j_done\", \"e01_build_finished\"] or a phrase from the word kit: [\"w_timer\", \"n25\", \"w_minutes\", \"w_left\"].",
      json!({ "names": { "type": "array", "items": { "type": "string" }, "minItems": 1 } }), &["names"]),
    tool("loop_sound", "Start one sound in repeat mode and leave it on, for example an ambient loop such as a_rain. Stop it with stop_sound.",
      json!({ "name": { "type": "string" } }), &["name"]),
    tool("stop_sound", "Pause the speaker playback.", json!({}), &[]),
    tool("sound_status", "Report what the SD card player does: track, elapsed seconds, play flag, volume.", json!({}), &[]),
    tool("list_art", "List the names of the animations and images that show_art can display.", json!({}), &[]),
    tool("show_art", "Show an animation or image from the art folder on the 16x16 display, by name. Example: fire, checkmark, pixel_idle.",
      json!({ "name": { "type": "string" } }), &["name"]),
    tool("show_image", "Show an image or GIF file from disk on the 16x16 display. The tool resizes it.",
      json!({ "path": { "type": "string", "description": "Absolute path of a PNG, JPEG, GIF, BMP or WebP file" } }), &["path"]),
    tool("show_card", "Show two short lines of text (up to 4 letters each) on the display, for example DONE / TEST.",
      json!({ "top": { "type": "string" }, "bottom": { "type": "string" }, "color": color, "bottom_color": color, "background": color }), &["top"]),
    tool("show_gauge", "Show a label (up to 4 letters) above a progress bar.",
      json!({ "label": { "type": "string" }, "value": { "type": "integer", "minimum": 0 }, "max": { "type": "integer", "minimum": 1 }, "color": color }), &["label", "value"]),
    tool("show_metric", "Show a small label above a value, for example TEMP / 72F. Up to 4 letters each.",
      json!({ "label": { "type": "string" }, "value": { "type": "string" }, "color": color }), &["label", "value"]),
    tool("set_brightness", "Set the display brightness, 0 to 100.",
      json!({ "percent": { "type": "integer", "minimum": 0, "maximum": 100 } }), &["percent"]),
    tool("get_volume", "Read the speaker volume, 0 to 15.", json!({}), &[]),
    tool("set_volume", "Set the speaker volume, 0 to 15.",
      json!({ "level": { "type": "integer", "minimum": 0, "maximum": 15 } }), &["level"]),
  ])
}

fn text_of<'a>(args: &'a Value, key: &str) -> Result<&'a str, Box<dyn Error>> {
  args.get(key).and_then(Value::as_str).ok_or_else(|| format!("Missing text argument \"{key}\"").into())
}

fn number_of(args: &Value, key: &str) -> Result<u64, Box<dyn Error>> {
  args.get(key).and_then(Value::as_u64).ok_or_else(|| format!("Missing number argument \"{key}\"").into())
}

fn color_of(args: &Value, key: &str, default: canvas::Color) -> Result<canvas::Color, Box<dyn Error>> {
  match args.get(key).and_then(Value::as_str) {
    None => Ok(default),
    Some(text) => {
      let c = csscolorparser::parse(text).map_err(|e| format!("Invalid color \"{text}\": {e}"))?.to_rgba8();
      Ok([c[0], c[1], c[2]])
    }
  }
}

/// Run one tool and return the text for the caller.
pub async fn call(config: &Config, name: &str, args: &Value) -> Result<String, Box<dyn Error>> {
  let address = config.address;
  match name {
    "list_sounds" => {
      let category = args.get("category").and_then(Value::as_str);
      let pack = sound::Pack::load(&config.pack)?;
      let lines = pack
        .sounds
        .iter()
        .filter(|s| category.is_none_or(|c| s.category == c))
        .map(|s| format!("{}\t{}\t{}", s.name, s.category, s.text))
        .collect::<Vec<_>>();
      Ok(if lines.is_empty() { "No sounds in that category.".into() } else { lines.join("\n") })
    }
    "play_sound" => {
      let names = args.get("names").and_then(Value::as_array).ok_or("Missing array argument \"names\"")?;
      let pack = sound::Pack::load(&config.pack)?;
      let sounds = names
        .iter()
        .map(|n| pack.find(n.as_str().ok_or("Each name must be text")?).cloned())
        .collect::<Result<Vec<_>, _>>()?;
      sound::play(address, &sounds).await?;
      Ok(format!("Played: {}", sounds.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ")))
    }
    "loop_sound" => {
      let pack = sound::Pack::load(&config.pack)?;
      let sound = pack.find(text_of(args, "name")?)?.clone();
      sound::start_loop(address, &sound).await?;
      Ok(format!("Loop started: {}", sound.name))
    }
    "stop_sound" => sound::stop(address).await.map(|_| "Playback paused.".into()),
    "sound_status" => Ok(serde_json::to_string(&sound::status(address).await?)?),
    "list_art" => Ok(art::names(&config.art)?.join("\n")),
    "show_art" => {
      let name = text_of(args, "name")?;
      art::show(address, &config.art, name).await?;
      Ok(format!("Showing {name}."))
    }
    "show_image" => {
      let path = text_of(args, "path")?;
      crate::send_image(address, path).await?;
      Ok(format!("Showing {path}."))
    }
    "show_card" => {
      let top_color = color_of(args, "color", canvas::WHITE)?;
      let card = canvas::card(
        text_of(args, "top")?,
        args.get("bottom").and_then(Value::as_str).unwrap_or(""),
        top_color,
        color_of(args, "bottom_color", top_color)?,
        color_of(args, "background", [0, 0, 0])?,
      );
      crate::send_canvas(address, &card).await.map(|_| "Card shown.".into())
    }
    "show_gauge" => {
      let maximum = args.get("max").and_then(Value::as_u64).unwrap_or(100);
      let gauge = canvas::gauge(text_of(args, "label")?, number_of(args, "value")? as u32, maximum as u32, color_of(args, "color", canvas::GREEN)?);
      crate::send_canvas(address, &gauge).await.map(|_| "Gauge shown.".into())
    }
    "show_metric" => {
      let metric = canvas::metric(text_of(args, "label")?, text_of(args, "value")?, color_of(args, "color", canvas::CYAN)?);
      crate::send_canvas(address, &metric).await.map(|_| "Metric shown.".into())
    }
    "set_brightness" => {
      let percent = number_of(args, "percent")?.min(100) as u8;
      crate::send_set_brightness(address, percent).await.map(|_| format!("Brightness {percent}."))
    }
    "get_volume" => Ok(crate::send_get_volume(address).await?.to_string()),
    "set_volume" => {
      let level = number_of(args, "level")?.min(15) as u8;
      crate::send_set_volume(address, level).await.map(|_| format!("Volume {level}."))
    }
    other => Err(format!("Unknown tool \"{other}\"").into()),
  }
}

/// Answer one JSON-RPC message. A notification gets no answer.
pub async fn handle(config: &Config, message: &Value) -> Option<Value> {
  let id = message.get("id").cloned()?;
  let method = message.get("method").and_then(Value::as_str).unwrap_or("");
  let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
  let result = match method {
    "initialize" => Ok(json!({
      // Use the version of the client, so that both sides agree.
      "protocolVersion": params.get("protocolVersion").cloned().unwrap_or_else(|| json!("2024-11-05")),
      "capabilities": { "tools": {} },
      "serverInfo": { "name": "divoom-ditoo-pro-controller", "version": env!("CARGO_PKG_VERSION") },
      "instructions": "Controls a Divoom Ditoo Pro: a 16x16 pixel display and a speaker that plays named sounds from an SD card. Each call needs about 3 seconds to connect over Bluetooth.",
    })),
    "ping" => Ok(json!({})),
    "tools/list" => Ok(json!({ "tools": tools() })),
    "tools/call" => {
      let name = params.get("name").and_then(Value::as_str).unwrap_or("");
      let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
      let (text, is_error) = match call(config, name, &args).await {
        Ok(text) => (text, false),
        Err(error) => (error.to_string(), true),
      };
      Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
    }
    other => Err(json!({ "code": -32601, "message": format!("Method not found: {other}") })),
  };
  Some(match result {
    Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
    Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
  })
}

/// Serve until stdin closes.
pub async fn serve(config: Config) -> Result<(), Box<dyn Error>> {
  loop {
    let line = tokio::task::spawn_blocking(|| {
      let mut line = String::new();
      std::io::stdin().read_line(&mut line).map(|n| (n, line))
    })
    .await??;
    if line.0 == 0 {
      return Ok(());
    }
    let text = line.1.trim();
    if text.is_empty() {
      continue;
    }
    let reply = match serde_json::from_str::<Value>(text) {
      Ok(message) => handle(&config, &message).await,
      Err(error) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("Parse error: {error}") } })),
    };
    if let Some(reply) = reply {
      let mut out = std::io::stdout().lock();
      writeln!(out, "{reply}")?;
      out.flush()?;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn config() -> Config {
    Config { address: Address::new([0; 6]), pack: PathBuf::from("soundpack"), art: PathBuf::from("art") }
  }

  #[tokio::test]
  async fn initialize_echoes_the_client_protocol_version() {
    let reply = handle(&config(), &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } })).await.unwrap();
    assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
    assert!(reply["result"]["capabilities"]["tools"].is_object());
  }

  #[tokio::test]
  async fn notifications_get_no_reply_and_unknown_methods_get_an_error() {
    assert!(handle(&config(), &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await.is_none());
    let reply = handle(&config(), &json!({ "jsonrpc": "2.0", "id": 7, "method": "nope" })).await.unwrap();
    assert_eq!(reply["error"]["code"], -32601);
    assert_eq!(reply["id"], 7);
  }

  #[tokio::test]
  async fn every_tool_has_a_schema_and_a_handler() {
    let reply = handle(&config(), &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).await.unwrap();
    let tools = reply["result"]["tools"].as_array().unwrap().clone();
    assert!(tools.len() >= 14);
    for tool in &tools {
      assert_eq!(tool["inputSchema"]["type"], "object");
      assert!(!tool["description"].as_str().unwrap().is_empty());
    }
    // A handler exists for each listed name: a call with no arguments must not answer "Unknown tool".
    // Only tools that fail on their arguments before any device access are called here.
    for name in ["play_sound", "loop_sound", "show_art", "show_image", "show_card", "show_gauge", "show_metric", "set_brightness", "set_volume"] {
      assert!(tools.iter().any(|t| t["name"] == name));
      let error = call(&config(), name, &json!({})).await.unwrap_err().to_string();
      assert!(error.starts_with("Missing") || error.contains("manifest"), "{name}: {error}");
    }
    let reply = handle(&config(), &json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "nope" } })).await.unwrap();
    assert_eq!(reply["result"]["isError"], true);
  }

  #[tokio::test]
  async fn list_tools_read_the_repository_pack_and_art() {
    let sounds = call(&config(), "list_sounds", &json!({ "category": "jingle" })).await.unwrap();
    assert!(sounds.contains("j_done\tjingle"));
    assert!(!sounds.contains("e01_build_finished"));
    assert!(call(&config(), "list_art", &json!({})).await.unwrap().lines().any(|l| l == "fire"));
  }
}
