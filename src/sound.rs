//! Play sounds from an SD card soundpack by name. See `docs/sd-card-sound.md` and `soundpack/README.md`.
//!
//! The firmware has no "play one track and stop" mode. A one-shot sound uses repeat-one mode, a track selection,
//! a wait for the length of the sound, and a pause.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::protocol::command::Command;
use crate::protocol::packet::Packet;
use crate::transport::DeviceConnection;
use crate::Address;

const SELECT_TRACK: u8 = 0x11;
const PLAY_MODE: u8 = 0xb9;
const CARD_STATUS: u8 = 0xb4;
const REPEAT_ONE: u8 = 2;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Sound {
  /// Track ID on the card: the position of the file in name order, from 0.
  pub id: u16,
  pub name: String,
  #[serde(default)]
  pub category: String,
  #[serde(default)]
  pub text: String,
  /// Length without the silence at the end.
  pub seconds: f32,
  #[serde(default, rename = "loop")]
  pub looping: bool,
}

#[derive(Debug, Deserialize)]
struct Manifest {
  sounds: Vec<Sound>,
}

#[derive(Debug)]
pub struct Pack {
  pub sounds: Vec<Sound>,
}

impl Pack {
  /// Read `manifest.json` and, if present, `manifest.local.json` (your own tracks) from the pack folder.
  pub fn load(dir: &Path) -> Result<Self, Box<dyn Error>> {
    let read = |name: &str| -> Result<Option<Manifest>, Box<dyn Error>> {
      let path = dir.join(name);
      if !path.exists() {
        return Ok(None);
      }
      let text = std::fs::read_to_string(&path)?;
      Ok(Some(serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?))
    };
    let mut sounds = read("manifest.json")?
      .ok_or_else(|| format!("No manifest.json in {}. Use --pack DIR or set DITOO_SOUNDPACK.", dir.display()))?
      .sounds;
    if let Some(local) = read("manifest.local.json")? {
      sounds.extend(local.sounds);
    }
    Ok(Self { sounds })
  }

  pub fn find(&self, name: &str) -> Result<&Sound, Box<dyn Error>> {
    self
      .sounds
      .iter()
      .find(|s| s.name == name)
      .ok_or_else(|| format!("Unknown sound \"{name}\". Use `sound list`.").into())
  }
}

/// The pack folder: `--pack`, then `DITOO_SOUNDPACK`, then `./soundpack`.
pub fn pack_dir(explicit: Option<PathBuf>) -> PathBuf {
  explicit
    .or_else(|| std::env::var_os("DITOO_SOUNDPACK").map(PathBuf::from))
    .unwrap_or_else(|| PathBuf::from("soundpack"))
}

#[derive(Debug, Serialize)]
pub struct CardStatus {
  pub elapsed_seconds: u16,
  pub length_seconds: u16,
  pub track: u16,
  pub playing: bool,
  pub volume: u8,
  pub play_mode: u8,
}

fn raw(opcode: u8, payload: Vec<u8>) -> Packet {
  Packet { command: Command::Raw(opcode), payload }
}

/// Play the sounds in sequence, one time each, then pause.
pub async fn play(address: Address, sounds: &[Sound]) -> Result<(), Box<dyn Error>> {
  let mut conn = DeviceConnection::connect(address).await?;
  conn.fire_and_forget(&raw(PLAY_MODE, vec![REPEAT_ONE])).await?;
  for sound in sounds {
    let started = tokio::time::Instant::now();
    conn.fire_and_forget(&raw(SELECT_TRACK, sound.id.to_le_bytes().to_vec())).await?;
    // The acknowledgment already used part of the sound length.
    let length = Duration::from_secs_f32(sound.seconds.max(0.0) + 0.25);
    tokio::time::sleep_until(started + length).await;
  }
  conn.fire_and_forget(&raw(Command::SetPlayStatus.value(), vec![0])).await?;
  conn.disconnect().await
}

/// Start a sound in repeat-one mode and leave it on.
pub async fn start_loop(address: Address, sound: &Sound) -> Result<(), Box<dyn Error>> {
  let mut conn = DeviceConnection::connect(address).await?;
  conn.fire_and_forget(&raw(PLAY_MODE, vec![REPEAT_ONE])).await?;
  conn.fire_and_forget(&raw(SELECT_TRACK, sound.id.to_le_bytes().to_vec())).await?;
  conn.disconnect().await
}

pub async fn stop(address: Address) -> Result<(), Box<dyn Error>> {
  let mut conn = DeviceConnection::connect(address).await?;
  conn.fire_and_forget(&raw(Command::SetPlayStatus.value(), vec![0])).await?;
  conn.disconnect().await
}

pub fn parse_status(data: &[u8]) -> Result<CardStatus, Box<dyn Error>> {
  if data.len() < 9 {
    return Err(format!("Card status reply has {} bytes, expected 9", data.len()).into());
  }
  let u16_at = |i: usize| u16::from_le_bytes([data[i], data[i + 1]]);
  Ok(CardStatus {
    elapsed_seconds: u16_at(0),
    length_seconds: u16_at(2),
    track: u16_at(4),
    playing: data[6] != 0,
    volume: data[7],
    play_mode: data[8],
  })
}

pub async fn status(address: Address) -> Result<CardStatus, Box<dyn Error>> {
  let mut conn = DeviceConnection::connect(address).await?;
  let reply = conn.exchange(&raw(CARD_STATUS, vec![]), CARD_STATUS, &[]).await?;
  conn.disconnect().await?;
  parse_status(&reply.data)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn status_reply_from_the_device() -> Result<(), Box<dyn Error>> {
    // Captured after a pause on track 260: 10 of 11 seconds, volume 4, repeat-one mode.
    let status = parse_status(&[0x0a, 0x00, 0x0b, 0x00, 0x04, 0x01, 0x00, 0x04, 0x02])?;
    assert_eq!((status.elapsed_seconds, status.length_seconds, status.track), (10, 11, 260));
    assert!(!status.playing);
    assert_eq!((status.volume, status.play_mode), (4, 2));
    assert!(parse_status(&[0; 8]).is_err());
    Ok(())
  }

  #[test]
  fn manifest_with_loop_field() -> Result<(), Box<dyn Error>> {
    let manifest: Manifest = serde_json::from_str(
      r#"{"voice":"x","sounds":[{"id":255,"file":"0255.mp3","name":"a_rain","category":"ambient","text":"Rain","seconds":24.0,"loop":true}]}"#,
    )?;
    let pack = Pack { sounds: manifest.sounds };
    assert!(pack.find("a_rain")?.looping);
    assert_eq!(pack.find("a_rain")?.id, 255);
    assert!(pack.find("missing").is_err());
    Ok(())
  }
}
