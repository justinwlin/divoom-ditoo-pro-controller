//! The art folder: `.divoom16`, `.gif` and `.png` files that a caller shows by name.

use std::error::Error;
use std::fs::File;
use std::path::{Path, PathBuf};

use crate::Address;

/// The art folder: the explicit one, then `DITOO_ART`, then `./art`.
pub fn art_dir(explicit: Option<PathBuf>) -> PathBuf {
  explicit
    .or_else(|| std::env::var_os("DITOO_ART").map(PathBuf::from))
    .unwrap_or_else(|| PathBuf::from("art"))
}

/// The files in the folder, in name order.
pub fn files(dir: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
  let mut files = std::fs::read_dir(dir)
    .map_err(|e| format!("Art folder {}: {e}. Use --art DIR or set DITOO_ART.", dir.display()))?
    .filter_map(|entry| entry.ok().map(|e| e.path()))
    .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("divoom16" | "gif" | "png")))
    .collect::<Vec<_>>();
  files.sort();
  Ok(files)
}

pub fn names(dir: &Path) -> Result<Vec<String>, Box<dyn Error>> {
  Ok(
    files(dir)?
      .iter()
      .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
      .collect(),
  )
}

pub fn find(dir: &Path, name: &str) -> Result<PathBuf, Box<dyn Error>> {
  files(dir)?
    .into_iter()
    .find(|p| p.file_stem().and_then(|s| s.to_str()) == Some(name))
    .ok_or_else(|| format!("No art named \"{name}\" in {}. Use `art list`.", dir.display()).into())
}

/// Send the named file to the display.
pub async fn show(address: Address, dir: &Path, name: &str) -> Result<(), Box<dyn Error>> {
  let path = find(dir, name)?;
  if path.extension().and_then(|e| e.to_str()) == Some("divoom16") {
    crate::send_divoom_animation(address, &mut File::open(path)?).await
  } else {
    crate::send_image(address, &path.to_string_lossy()).await
  }
}
