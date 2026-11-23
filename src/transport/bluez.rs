//! Linux BLE backend: BlueZ GATT through `bluer`, with per-bearer control over D-Bus.

use std::{collections::VecDeque, error::Error, time::Duration};

use bluer::gatt::{remote::Characteristic, remote::CharacteristicWriteRequest, WriteOp};
use futures::StreamExt;
use tokio::sync::mpsc;

use super::{envelope, parse_frames};
use crate::protocol::packet::{Packet, Response};
use crate::Address;

pub(crate) struct DeviceLease {
  device: bluer::Device,
  owned: bool,
  bearer: Bearer,
}

#[derive(Clone, Copy)]
enum Bearer {
  Classic,
  Le,
}

#[derive(Clone, Copy)]
enum BearerOperation {
  Connected,
  Connect,
  Disconnect,
}

fn missing_bearer(error: &dbus::Error) -> bool {
  matches!(error.name(), Some(
    "org.freedesktop.DBus.Error.UnknownMethod"
      | "org.freedesktop.DBus.Error.UnknownInterface"
      | "org.freedesktop.DBus.Error.UnknownProperty"
      | "org.freedesktop.DBus.Error.UnknownObject"
  )) || (error.name() == Some("org.freedesktop.DBus.Error.InvalidArgs")
    && error.message().is_some_and(|s| s.starts_with("No such interface 'org.bluez.Bearer.")))
}

// bluer does not yet expose BlueZ's per-bearer API. Keep blocking D-Bus work
// off the async executor; each call owns and closes its system-bus connection.
async fn bearer_operation(
  device: &bluer::Device,
  bearer: Bearer,
  operation: BearerOperation,
) -> Result<Option<bool>, Box<dyn Error>> {
  let path = format!(
    "/org/bluez/{}/dev_{}",
    device.adapter_name(),
    device.address().to_string().replace(':', "_")
  );
  let result = tokio::task::spawn_blocking(move || -> Result<bool, dbus::Error> {
    use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
    let connection = dbus::blocking::Connection::new_system()?;
    let proxy = connection.with_proxy("org.bluez", path, Duration::from_secs(10));
    let interface = match bearer {
      Bearer::Classic => "org.bluez.Bearer.BREDR1",
      Bearer::Le => "org.bluez.Bearer.LE1",
    };
    match operation {
      BearerOperation::Connected => proxy.get(interface, "Connected"),
      BearerOperation::Connect | BearerOperation::Disconnect => {
        let method = if matches!(operation, BearerOperation::Connect) {
          "Connect"
        } else {
          "Disconnect"
        };
        proxy.method_call::<(), _, _, _>(interface, method, ())?;
        Ok(true)
      }
    }
  })
  .await?;
  match result {
    Ok(value) => Ok(Some(value)),
    Err(error) if missing_bearer(&error) => Ok(None),
    Err(error) => Err(error.into()),
  }
}

async fn disconnect_lease(device: &bluer::Device, bearer: Bearer) -> Result<(), Box<dyn Error>> {
  if bearer_operation(device, bearer, BearerOperation::Disconnect).await?.is_some() {
    return Ok(());
  }
  if matches!(bearer, Bearer::Classic) || device.class().await?.is_some() {
    // Closing the RFCOMM stream releases our profile. Older BlueZ cannot
    // disconnect just its bearer without also terminating a live BLE link.
    log::debug!("Leaving Bluetooth ACL connected: BlueZ lacks per-bearer disconnect");
    return Ok(());
  }
  device.disconnect().await?;
  Ok(())
}

impl DeviceLease {
  pub async fn new_classic(device: bluer::Device) -> Result<Self, Box<dyn Error>> {
    let connected = match bearer_operation(&device, Bearer::Classic, BearerOperation::Connected).await? {
      Some(connected) => connected,
      None => match device.is_connected().await {
        Ok(connected) => connected,
        Err(error) if error.kind == bluer::ErrorKind::NotFound => false,
        Err(error) if matches!(&error.kind,
          bluer::ErrorKind::Internal(bluer::InternalErrorKind::DBus(name))
          if name == "org.freedesktop.DBus.Error.UnknownObject") => false,
        Err(error) => return Err(error.into()),
      },
    };
    Ok(Self {
      device,
      owned: !connected,
      bearer: Bearer::Classic,
    })
  }

  async fn new_ble(device: bluer::Device) -> Result<Self, Box<dyn Error>> {
    let connected = match bearer_operation(&device, Bearer::Le, BearerOperation::Connected).await? {
      Some(connected) => connected,
      None => device.is_connected().await?,
    };
    Ok(Self {
      device,
      owned: !connected,
      bearer: Bearer::Le,
    })
  }

  pub async fn release(&mut self) -> Result<(), Box<dyn Error>> {
    if self.owned {
      self.owned = false;
      tokio::time::timeout(
        Duration::from_secs(12),
        disconnect_lease(&self.device, self.bearer),
      )
      .await??;
    }
    Ok(())
  }
}

impl Drop for DeviceLease {
  fn drop(&mut self) {
    if self.owned {
      let device = self.device.clone();
      let bearer = self.bearer;
      if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        runtime.spawn(async move {
          let _ = disconnect_lease(&device, bearer).await;
        });
      }
    }
  }
}

pub(crate) struct BleConnection {
  write: Characteristic,
  stream_write_size: usize,
  pub(super) responses: mpsc::UnboundedReceiver<Response>,
  pub(super) pending: VecDeque<Response>,
  reader: tokio::task::JoinHandle<()>,
  sequence: u16,
  _session: bluer::Session,
  pub(super) device: DeviceLease,
}

impl Drop for BleConnection {
  fn drop(&mut self) {
    self.reader.abort();
  }
}

async fn connect_ble_bearer(
  adapter: &bluer::Adapter,
  device: &bluer::Device,
) -> Result<(), Box<dyn Error>> {
  if let Some(connected) = bearer_operation(device, Bearer::Le, BearerOperation::Connected).await? {
    if !connected {
      bearer_operation(device, Bearer::Le, BearerOperation::Connect).await?;
    }
    while !device.is_services_resolved().await? {
      tokio::time::sleep(Duration::from_millis(50)).await;
    }
    return Ok(());
  }
  // Device1.Connect may choose BR/EDR for a dual-mode Ditoo and claim its
  // audio connection. ConnectDevice's explicit address type selects LE.
  let address_type = match device.address_type().await? {
    bluer::AddressType::LeRandom => bluer::AddressType::LeRandom,
    _ => bluer::AddressType::LePublic,
  };
  match adapter.connect_device(device.address(), address_type).await {
    Ok(_) => (),
    Err(error) if error.kind == bluer::ErrorKind::AlreadyConnected => (),
    Err(error)
      if matches!(&error.kind,
      bluer::ErrorKind::Internal(bluer::InternalErrorKind::DBus(name))
        if name == "org.freedesktop.DBus.Error.UnknownMethod") =>
    {
      // Keep older BlueZ installations usable for LE-only discovery, but do
      // not silently switch a known audio device to the classic bearer.
      if device.class().await?.is_some() {
        return Err("Explicit BLE selection requires BlueZ Experimental=true in /etc/bluetooth/main.conf; generic Connect could take the Ditoo audio connection".into());
      }
      if !device.is_connected().await? {
        device.connect().await?;
      }
    }
    Err(error) => return Err(error.into()),
  }
  while !device.is_services_resolved().await? {
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
  Ok(())
}

impl BleConnection {
  pub(super) async fn connect(address: Address) -> Result<Self, Box<dyn Error>> {
    let session = bluer::Session::new().await?;
    let adapter = session.default_adapter().await?;
    adapter.set_powered(true).await?;
    // Discover before reading Device1 properties: BlueZ can remove an
    // unpaired device object after the previous connection ends.
    adapter
      .set_discovery_filter(bluer::DiscoveryFilter {
        transport: bluer::DiscoveryTransport::Le,
        ..Default::default()
      })
      .await?;
    let discovery = adapter.discover_devices().await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let device = adapter.device(address)?;
    let lease = DeviceLease::new_ble(device.clone()).await?;
    connect_ble_bearer(&adapter, &device).await?;
    drop(discovery);
    let mut write = None;
    let mut notify = None;
    for service in device.services().await? {
      if service.uuid().await? != uuid::Uuid::from_u128(0x49535343_fe7d_4ae5_8fa9_9fafd205e455) {
        continue;
      }
      for characteristic in service.characteristics().await? {
        match characteristic.uuid().await?.as_u128() {
          0x49535343_8841_43f4_a8d4_ecbe34729bb3 => write = Some(characteristic),
          0x49535343_1e4d_4bd9_ba61_23c647249616 => notify = Some(characteristic),
          _ => (),
        }
      }
    }
    let write = write.ok_or("Divoom BLE write characteristic not found")?;
    let notify = notify.ok_or("Divoom BLE notification characteristic not found")?;
    let notifications = match notify.notify().await {
      Ok(notifications) => notifications,
      Err(error) if error.kind == bluer::ErrorKind::Failed && error.message == "Not connected" => {
        // BlueZ can report a connected classic bearer with cached GATT services.
        connect_ble_bearer(&adapter, &device).await?;
        notify.notify().await?
      }
      Err(error) => return Err(error.into()),
    };
    let (tx, responses) = mpsc::unbounded_channel();
    let reader = tokio::spawn(async move {
      tokio::pin!(notifications);
      let mut buffer = Vec::new();
      while let Some(bytes) = notifications.next().await {
        log::trace!(
          "BLE notification ({} bytes): {}",
          bytes.len(),
          hex::encode(&bytes)
        );
        buffer.extend(bytes);
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
      stream_write_size: 20,
      responses,
      pending: VecDeque::new(),
      reader,
      sequence: 0x0101,
      _session: session,
      device: lease,
    };
    let now = chrono::Local::now();
    let init = format!(
      r#"{{"Command":"Device\/SetUTC","Utc":{},"Time":"{}"}}"#,
      now.timestamp(),
      now.format("%Y-%m-%d %H:%M:%S")
    );
    if let Err(error) = connection.write_payload(init.as_bytes(), 1, None).await {
      let not_connected = error
        .downcast_ref::<bluer::Error>()
        .is_some_and(|e| e.kind == bluer::ErrorKind::Failed && e.message == "Not connected");
      if !not_connected {
        return Err(error);
      }
      // Cached notifications can also succeed with only the classic bearer active.
      connect_ble_bearer(&adapter, &connection.device.device).await?;
      connection.write_payload(init.as_bytes(), 1, None).await?;
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    connection.refresh_stream_capacity().await;
    // Stock BLE remembers the last binary sequence across controller sessions.
    // Reusing it gets an ACK without executing the command. Prime that counter
    // with a read-only volume query; the first user command then has a different
    // sequence whether this primer was executed or deduplicated. JSON clock
    // initialization uses a separate path and does not reset this counter.
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

  async fn refresh_stream_capacity(&mut self) {
    // bluer's mtu() already subtracts ATT overhead; do not subtract it twice.
    // Cached GATT services may expose MTU only after the bearer reconnects.
    if let Ok(size) = self.write.mtu().await {
      if size > 0 && size.min(512) != self.stream_write_size {
        self.stream_write_size = size.min(512);
        log::info!(
          "BLE streaming write capacity: {} bytes",
          self.stream_write_size
        );
      }
    }
  }

  async fn write_payload(
    &mut self,
    payload: &[u8],
    sequence: u16,
    stream_limit: Option<usize>,
  ) -> Result<(), Box<dyn Error>> {
    let streaming = stream_limit.is_some();
    if streaming && self.stream_write_size <= 20 {
      self.refresh_stream_capacity().await;
    }
    let capacity = if let Some(limit) = stream_limit {
      self.stream_write_size.min(limit)
    } else {
      20
    };
    for chunk in envelope(payload, sequence)?.chunks(capacity) {
      self
        .write
        .write_ext(
          chunk,
          &CharacteristicWriteRequest {
            op_type: WriteOp::Request,
            ..Default::default()
          },
        )
        .await?;
      // Acknowledged GATT writes provide backpressure for source/firmware data.
      if !streaming {
        tokio::time::sleep(Duration::from_millis(50)).await;
      }
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
    self.sequence = if sequence == 0x01ff {
      0x0101
    } else {
      sequence + 1
    };
    let mut payload = vec![packet.command.value()];
    payload.extend(&packet.payload);
    // Source uploads can span hundreds of fragments. Pace them with ATT write
    // responses, like firmware data, instead of a fixed delay per 20 bytes.
    // Bound each source-data write to 256 bytes even when the peer offers more.
    let stream_limit = if preserve {
      Some(512)
    } else if packet.command.value() == 0x37 && packet.payload.starts_with(b"\x7fDLUA\x04") {
      Some(256)
    } else {
      None
    };
    self.write_payload(&payload, sequence, stream_limit).await?;
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
      } else if expected.is_some_and(|(code, prefix)| {
        reply.original_command == code && reply.data.starts_with(prefix)
      }) {
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

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn absent_classic_bearer_on_ble_only_cached_device() {
    let absent = dbus::Error::new_custom(
      "org.freedesktop.DBus.Error.InvalidArgs",
      "No such interface 'org.bluez.Bearer.BREDR1'",
    );
    assert!(missing_bearer(&absent));
    for (name, message) in [
      ("org.freedesktop.DBus.Error.InvalidArgs", "Invalid connection parameters"),
      ("org.freedesktop.DBus.Error.AccessDenied", "Permission denied"),
      ("org.freedesktop.DBus.Error.NoReply", "Timed out"),
    ] {
      assert!(!missing_bearer(&dbus::Error::new_custom(name, message)));
    }
  }
}
