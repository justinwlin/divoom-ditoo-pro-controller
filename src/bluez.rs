//! Linux backend: BlueZ scan, paired-device lookup and the RFCOMM (classic) connection.

use std::error::Error;
use std::time::Duration;

use futures::StreamExt;
use log::{debug, info};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::protocol::packet::{Packet, Response};
use crate::{transport, Address};

pub async fn scan_devices() -> Result<(), Box<dyn Error>> {
  let session = bluer::Session::new().await?;
  let adapter = session.default_adapter().await?;
  adapter.set_powered(true).await?;

  let duration = Duration::from_secs(20);
  info!("Scanning bluetooth devices for {:?}", duration);

  let discover = adapter.discover_devices().await?;
  tokio::pin!(discover);

  let timeout = tokio::time::sleep(duration);
  tokio::pin!(timeout);

  loop {
    tokio::select! {
      Some(event) = discover.next() => {
        if let bluer::AdapterEvent::DeviceAdded(addr) = event {
          let device = adapter.device(addr)?;
          let name = device.name().await?.unwrap_or_default();
          info!("Found device: {} ({})", name, addr);
        }
      }
      _ = &mut timeout => {
        info!("Scan complete");
        break;
      }
    }
  }

  Ok(())
}

pub async fn find_paired_ditoo_pro_devices() -> Result<Vec<Address>, Box<dyn Error>> {
  let session = bluer::Session::new().await?;
  let adapter = session.default_adapter().await?;

  let addresses = adapter.device_addresses().await?;

  let mut result = Vec::new();
  for addr in addresses {
    let device = adapter.device(addr)?;
    let is_paired = device.is_paired().await?;
    if !is_paired {
      continue;
    }

    let name = device.name().await?.unwrap_or_default();
    if !name.contains("DitooPro") && name != "Ditoo BLE Remote" {
      continue;
    }

    result.push(addr);
  }

  Ok(result)
}

pub async fn list_paired_devices() -> Result<(), Box<dyn Error>> {
  let session = bluer::Session::new().await?;
  let adapter = session.default_adapter().await?;

  for addr in find_paired_ditoo_pro_devices().await? {
    let device = adapter.device(addr)?;
    let name = device.name().await?.unwrap_or_default();
    let is_connected = device.is_connected().await?;
    info!(
      "{} ({}) - connected: {}",
      name, addr, is_connected
    );
  }

  Ok(())
}

async fn read_response(reader: &mut bluer::rfcomm::stream::OwnedReadHalf) -> Result<Response, Box<dyn Error + Send + Sync>> {
  let start = reader.read_u8().await?;
  if start != 0x01 {
    return Err(format!("Expected start byte 0x01, got 0x{:02x}", start).into());
  }

  let mut len_bytes = [0u8; 2];
  reader.read_exact(&mut len_bytes).await?;
  let length = u16::from_le_bytes(len_bytes) as usize;

  // Read: payload (length - 2 for checksum) + checksum (2) + end byte (1) = length + 1
  let mut remaining = vec![0u8; length + 1];
  reader.read_exact(&mut remaining).await?;

  let mut frame = vec![0x01];
  frame.extend_from_slice(&len_bytes);
  frame.extend_from_slice(&remaining);

  debug!("Received response: {}", hex::encode(&frame));

  Response::deserialize(&frame)
}

const MAX_CONNECT_ATTEMPTS: u32 = 3;
const INTER_PACKET_DELAY: Duration = Duration::from_millis(40);

pub(crate) struct ClassicConnection {
  writer: bluer::rfcomm::stream::OwnedWriteHalf,
  _reader_handle: tokio::task::JoinHandle<()>,
  pub(crate) response_rx: mpsc::UnboundedReceiver<Response>,
  _session: bluer::Session,
  _profile_handle: bluer::rfcomm::ProfileHandle,
  device: transport::DeviceLease,
}

impl ClassicConnection {
  pub(crate) async fn connect(mac_address: Address) -> Result<Self, Box<dyn Error>> {
    info!("Connecting to device with MAC address {}", mac_address);

    let mut last_error = String::new();
    for attempt in 1..=MAX_CONNECT_ATTEMPTS {
      if attempt > 1 {
        info!("Retrying connection (attempt {}/{})..", attempt, MAX_CONNECT_ATTEMPTS);
        tokio::time::sleep(Duration::from_secs(1)).await;
      }

      match tokio::time::timeout(Duration::from_secs(8), Self::try_connect(mac_address)).await {
        Ok(Ok(conn)) => return Ok(conn),
        Ok(Err(error)) => last_error = error.to_string(),
        Err(error) => last_error = format!("RFCOMM connection timed out: {error}"),
      }
      debug!("Connection attempt {} failed: {}", attempt, last_error);
    }

    Err(format!("Failed to connect via RFCOMM: {last_error}").into())
  }

  async fn try_connect(mac_address: Address) -> Result<Self, Box<dyn Error>> {
    let session = bluer::Session::new().await?;
    let adapter = session.default_adapter().await?;
    adapter.set_powered(true).await?;

    let spp_uuid = uuid::Uuid::from_u128(0x00001101_0000_1000_8000_00805f9b34fb);

    let profile = bluer::rfcomm::Profile {
      uuid: spp_uuid,
      name: Some("divoom-controller".to_string()),
      role: Some(bluer::rfcomm::Role::Client),
      require_authentication: Some(false),
      require_authorization: Some(false),
      auto_connect: Some(false),
      ..Default::default()
    };

    let mut profile_handle = session.register_profile(profile).await?;
    let device = adapter.device(mac_address)?;
    let lease = transport::DeviceLease::new_classic(device.clone()).await?;

    // ConnectProfile selects BR/EDR and resolves SDP even with only a cached
    // BLE device. Generic Connect may choose BLE or connect audio profiles.
    let connect = async {
      match device.connect_profile(&spp_uuid).await {
        Err(error) if error.kind == bluer::ErrorKind::NotFound => {
          // A non-discoverable peer may have expired from BlueZ's device cache.
          // Bootstrap that explicit Classic address, then request our profile.
          adapter.connect_device(mac_address, bluer::AddressType::BrEdr).await?;
          device.connect_profile(&spp_uuid).await
        }
        result => result,
      }
    };
    tokio::pin!(connect);
    let mut completed = false;

    // BlueZ waits for our NewConnection reply before completing ConnectProfile.
    // Accept the stream while that call is pending, not after it returns.
    let stream = loop {
      tokio::select! {
        result = &mut connect, if !completed => {
          completed = true;
          if let Err(error) = result {
            debug!("connect_profile: {error} (waiting for profile handle)");
          }
        }
        request = profile_handle.next() => {
          let req = request.ok_or("ProfileHandle stream closed")?;
          if req.device() == mac_address {
            break req.accept()?;
          }
        }
      }
    };
    if !completed {
      connect.await?;
    }

    info!("Connected via SDP profile");
    let (reader, writer) = stream.into_split();
    let (tx, rx) = mpsc::unbounded_channel();

    // Spawn background reader task
    let reader_handle = tokio::spawn(async move {
      let mut reader = reader;
      loop {
        match read_response(&mut reader).await {
          Ok(response) => {
            debug!("Background reader got response: {:?}", response);
            if tx.send(response).is_err() {
              debug!("Response channel closed, stopping reader");
              break;
            }
          }
          Err(e) => {
            debug!("Background reader error: {}", e);
            break;
          }
        }
      }
    });

    Ok(ClassicConnection {
      writer,
      _reader_handle: reader_handle,
      response_rx: rx,
      _session: session,
      _profile_handle: profile_handle,
      device: lease,
    })
  }

  pub(crate) async fn send_and_receive(&mut self, packet: &Packet) -> Result<Response, Box<dyn Error>> {
    self.exchange(packet, packet.command.value(), &[]).await
  }

  pub(crate) async fn exchange(&mut self, packet: &Packet, expected_command: u8, prefix: &[u8]) -> Result<Response, Box<dyn Error>> {
    while self.response_rx.try_recv().is_ok() {}
    let serialized = packet.serialize()?;
    debug!("send_and_receive 0x{:02x}: {}", expected_command, hex::encode(&serialized));
    self.writer.write_all(&serialized).await?;
    tokio::time::sleep(INTER_PACKET_DELAY).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
      let response = tokio::time::timeout_at(deadline, self.response_rx.recv()).await
        .map_err(|_| format!("Timed out waiting for response to command 0x{:02x}", expected_command))?
        .ok_or("Response channel closed")?;
      if response.original_command != expected_command || !response.data.starts_with(prefix) {
        debug!("Skipping unsolicited response for command 0x{:02x}", response.original_command);
        continue;
      }
      if !response.ack {
        return Err(format!("Device NAK'd command 0x{:02x}", expected_command).into());
      }
      return Ok(response);
    }
  }

  pub(crate) async fn fire_and_forget(&mut self, packet: &Packet) -> Result<(), Box<dyn Error>> {
    let serialized = packet.serialize()?;
    debug!("fire_and_forget 0x{:02x}: {}", packet.command.value(), hex::encode(&serialized));
    self.writer.write_all(&serialized).await?;
    tokio::time::sleep(INTER_PACKET_DELAY).await;
    Ok(())
  }

  pub(crate) async fn disconnect(mut self) -> Result<(), Box<dyn Error>> {
    info!("Disconnecting from device");
    self._reader_handle.abort();
    self.writer.shutdown().await?;
    self.device.release().await?;
    Ok(())
  }
}

impl Drop for ClassicConnection {
  fn drop(&mut self) { self._reader_handle.abort(); }
}
