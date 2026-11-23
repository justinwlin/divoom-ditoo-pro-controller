//! Backend for platforms without BlueZ (macOS, Windows): BLE discovery through `btleplug`.
//! RFCOMM is not available here, so `ClassicConnection` only reports that and `auto` falls back to BLE.

use std::error::Error;
use std::time::Duration;

use btleplug::api::Peripheral as _;
use log::info;
use tokio::sync::mpsc;

use crate::protocol::packet::{Packet, Response};
use crate::transport::portable::{adapter, discover, DEFAULT_NAME};
use crate::Address;

const UNSUPPORTED: &str = "RFCOMM is not supported on this platform; use --transport ble";

pub async fn scan_devices() -> Result<(), Box<dyn Error>> {
  let adapter = adapter().await?;
  let duration = Duration::from_secs(20);
  info!("Scanning BLE devices for {:?}", duration);
  let mut seen = std::collections::HashSet::new();
  discover(&adapter, duration, |peripheral, name| {
    if !name.is_empty() && seen.insert(peripheral.id()) {
      info!("Found device: {} ({})", name, peripheral.id());
    }
    false
  })
  .await?;
  info!("Scan complete");
  Ok(())
}

/// These platforms have no list of paired devices that the tool can read. The BLE backend selects the device by
/// name, so this returns the "any" address when a Ditoo is in range.
pub async fn find_paired_ditoo_pro_devices() -> Result<Vec<Address>, Box<dyn Error>> {
  Ok(vec![Address::any()])
}

pub async fn list_paired_devices() -> Result<(), Box<dyn Error>> {
  let adapter = adapter().await?;
  let mut seen = std::collections::HashSet::new();
  discover(&adapter, Duration::from_secs(8), |peripheral, name| {
    if (name.contains("DitooPro") || name == "Ditoo BLE Remote") && seen.insert(peripheral.id()) {
      info!("{} ({}) - select it with --name", name, peripheral.id());
    }
    false
  })
  .await?;
  if seen.is_empty() {
    info!("No Ditoo Pro found. The default name filter is \"{DEFAULT_NAME}\".");
  }
  Ok(())
}

pub(crate) struct ClassicConnection {
  pub(crate) response_rx: mpsc::UnboundedReceiver<Response>,
}

impl ClassicConnection {
  pub(crate) async fn connect(_mac_address: Address) -> Result<Self, Box<dyn Error>> {
    Err(UNSUPPORTED.into())
  }

  pub(crate) async fn send_and_receive(&mut self, _packet: &Packet) -> Result<Response, Box<dyn Error>> {
    Err(UNSUPPORTED.into())
  }

  pub(crate) async fn exchange(&mut self, _packet: &Packet, _expected: u8, _prefix: &[u8]) -> Result<Response, Box<dyn Error>> {
    Err(UNSUPPORTED.into())
  }

  pub(crate) async fn fire_and_forget(&mut self, _packet: &Packet) -> Result<(), Box<dyn Error>> {
    Err(UNSUPPORTED.into())
  }

  pub(crate) async fn disconnect(self) -> Result<(), Box<dyn Error>> {
    Ok(())
  }
}
