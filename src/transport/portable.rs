//! Portable BLE backend (macOS, Windows) through `btleplug`.
//!
//! macOS hides the Bluetooth address of a peripheral, so the device is selected by a part of its advertised name.
//! The default filter is `DitooPro-Light`; `--name` changes it. An explicit address is used only where the platform
//! exposes one.

use std::{collections::VecDeque, error::Error, time::Duration};

use btleplug::api::{Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use tokio::sync::mpsc;

use super::{envelope, parse_frames, selected_device_name};
use crate::protocol::packet::{Packet, Response};
use crate::Address;

const SERVICE: u128 = 0x49535343_fe7d_4ae5_8fa9_9fafd205e455;
const WRITE: u128 = 0x49535343_8841_43f4_a8d4_ecbe34729bb3;
const NOTIFY: u128 = 0x49535343_1e4d_4bd9_ba61_23c647249616;
pub(crate) const DEFAULT_NAME: &str = "DitooPro-Light";

pub(crate) async fn adapter() -> Result<Adapter, Box<dyn Error>> {
  let manager = Manager::new().await?;
  manager
    .adapters()
    .await?
    .into_iter()
    .next()
    .ok_or_else(|| "No Bluetooth adapter found".into())
}

/// Scan until `accept` returns a peripheral or the time ends.
pub(crate) async fn discover(
  adapter: &Adapter,
  duration: Duration,
  mut visit: impl FnMut(&Peripheral, &str) -> bool,
) -> Result<Option<Peripheral>, Box<dyn Error>> {
  adapter.start_scan(ScanFilter::default()).await?;
  let deadline = tokio::time::Instant::now() + duration;
  let mut found = None;
  while found.is_none() && tokio::time::Instant::now() < deadline {
    tokio::time::sleep(Duration::from_millis(250)).await;
    for peripheral in adapter.peripherals().await? {
      let name = match peripheral.properties().await? {
        Some(properties) => properties.local_name.unwrap_or_default(),
        None => String::new(),
      };
      if visit(&peripheral, &name) {
        found = Some(peripheral);
        break;
      }
    }
  }
  adapter.stop_scan().await?;
  Ok(found)
}

/// Releases the BLE connection. Mirrors the role of the BlueZ `DeviceLease`.
pub(crate) struct DeviceLease {
  peripheral: Peripheral,
}

impl DeviceLease {
  pub async fn release(&mut self) -> Result<(), Box<dyn Error>> {
    if self.peripheral.is_connected().await.unwrap_or(false) {
      tokio::time::timeout(Duration::from_secs(12), self.peripheral.disconnect()).await??;
    }
    Ok(())
  }
}

pub(crate) struct BleConnection {
  write: Characteristic,
  pub(super) responses: mpsc::UnboundedReceiver<Response>,
  pub(super) pending: VecDeque<Response>,
  reader: tokio::task::JoinHandle<()>,
  sequence: u16,
  pub(super) device: DeviceLease,
}

impl Drop for BleConnection {
  fn drop(&mut self) {
    self.reader.abort();
  }
}

impl BleConnection {
  pub(super) async fn connect(address: Address) -> Result<Self, Box<dyn Error>> {
    let adapter = adapter().await?;
    let wanted_name = selected_device_name().unwrap_or_else(|| DEFAULT_NAME.to_string());
    let wanted_address = (address != Address::any()).then(|| address.to_string());
    let peripheral = discover(&adapter, Duration::from_secs(15), |peripheral, name| {
      match &wanted_address {
        // Platforms that expose the address (Windows) can match it. macOS reports a UUID here.
        Some(wanted) if peripheral.address().to_string().eq_ignore_ascii_case(wanted) => true,
        Some(_) if selected_device_name().is_none() => false,
        _ => name.to_lowercase().contains(&wanted_name.to_lowercase()),
      }
    })
    .await?
    .ok_or_else(|| format!("No BLE device with \"{wanted_name}\" in its name was found. Is the Ditoo on and near?"))?;

    if !peripheral.is_connected().await? {
      peripheral.connect().await?;
    }
    peripheral.discover_services().await?;
    let characteristics = peripheral.characteristics();
    let find = |uuid: u128| {
      characteristics
        .iter()
        .find(|c| c.uuid.as_u128() == uuid && c.service_uuid.as_u128() == SERVICE)
        .cloned()
    };
    let write = find(WRITE).ok_or("Divoom BLE write characteristic not found")?;
    let notify = find(NOTIFY).ok_or("Divoom BLE notification characteristic not found")?;
    peripheral.subscribe(&notify).await?;
    let mut notifications = peripheral.notifications().await?;
    let (tx, responses) = mpsc::unbounded_channel();
    let reader = tokio::spawn(async move {
      let mut buffer = Vec::new();
      while let Some(notification) = notifications.next().await {
        if notification.uuid.as_u128() != NOTIFY {
          continue;
        }
        log::trace!(
          "BLE notification ({} bytes): {}",
          notification.value.len(),
          hex::encode(&notification.value)
        );
        buffer.extend(notification.value);
        match parse_frames(&mut buffer) {
          Ok(replies) => {
            for reply in replies {
              if tx.send(reply).is_err() {
                return;
              }
            }
          }
          Err(error) => {
            log::warn!("BLE response error: {error}");
            return;
          }
        }
      }
    });
    let mut connection = Self {
      write,
      responses,
      pending: VecDeque::new(),
      reader,
      sequence: 0x0101,
      device: DeviceLease { peripheral },
    };
    let now = chrono::Local::now();
    let init = format!(
      r#"{{"Command":"Device\/SetUTC","Utc":{},"Time":"{}"}}"#,
      now.timestamp(),
      now.format("%Y-%m-%d %H:%M:%S")
    );
    connection.write_payload(init.as_bytes(), 1).await?;
    tokio::time::sleep(Duration::from_secs(1)).await;
    // Stock BLE remembers the last binary sequence across controller sessions. Prime the counter with a
    // read-only volume query, as the BlueZ backend does.
    connection
      .send(
        &Packet {
          command: crate::protocol::command::Command::GetVolume,
          payload: Vec::new(),
        },
        None,
      )
      .await?;
    log::info!("Connected via BLE GATT (device clock synchronized)");
    Ok(connection)
  }

  async fn write_payload(&mut self, payload: &[u8], sequence: u16) -> Result<(), Box<dyn Error>> {
    // 20-byte acknowledged writes work at the default ATT MTU. The platform does not report a larger one.
    for chunk in envelope(payload, sequence)?.chunks(20) {
      self
        .device
        .peripheral
        .write(&self.write, chunk, WriteType::WithResponse)
        .await?;
      tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
  }

  pub(super) async fn send(
    &mut self,
    packet: &Packet,
    expected: Option<(u8, &[u8])>,
  ) -> Result<Option<Response>, Box<dyn Error>> {
    self.send_inner(packet, expected, false).await
  }

  pub(super) async fn send_inner(
    &mut self,
    packet: &Packet,
    expected: Option<(u8, &[u8])>,
    preserve: bool,
  ) -> Result<Option<Response>, Box<dyn Error>> {
    if !preserve {
      self.pending.clear();
      while self.responses.try_recv().is_ok() {}
    }
    let sequence = self.sequence;
    self.sequence = if sequence == 0x01ff { 0x0101 } else { sequence + 1 };
    let mut payload = vec![packet.command.value()];
    payload.extend(&packet.payload);
    self.write_payload(&payload, sequence).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut acknowledged = false;
    let mut command_response = None;
    loop {
      let reply = tokio::time::timeout_at(deadline, self.responses.recv())
        .await
        .map_err(|_| "Timed out waiting for BLE acknowledgment/response")?
        .ok_or("BLE notifications stopped")?;
      if reply.original_command == 0x33 && reply.data == [sequence as u8, 0, 0, 0] {
        if !reply.ack {
          return Err("BLE transport rejected command".into());
        }
        acknowledged = true;
      } else if expected.is_some_and(|(code, prefix)| reply.original_command == code && reply.data.starts_with(prefix)) {
        if !reply.ack {
          return Err("Device rejected command".into());
        }
        command_response = Some(reply);
      } else if preserve {
        self.pending.push_back(reply);
      }
      if acknowledged && (expected.is_none() || command_response.is_some()) {
        return Ok(command_response);
      }
    }
  }
}
