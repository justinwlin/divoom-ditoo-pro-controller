use crate::{
  protocol::packet::{Packet, Response},
  ClassicConnection,
};
use crate::Address;
use std::{error::Error, future::Future, time::Duration};

#[cfg(target_os = "linux")]
mod bluez;
#[cfg(target_os = "linux")]
pub(crate) use bluez::{BleConnection, DeviceLease};
#[cfg(not(target_os = "linux"))]
pub(crate) mod portable;
#[cfg(not(target_os = "linux"))]
pub(crate) use portable::BleConnection;

#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
pub enum Transport {
  #[default]
  Auto,
  Rfcomm,
  Ble,
  /// USB control (custom firmware 306019+) and bootloader updates
  Usb,
}

tokio::task_local! { static TRANSPORT: Transport; }
tokio::task_local! { static USB_PORT: Option<String>; }
tokio::task_local! { static DEVICE_NAME: Option<String>; }

/// The name filter for BLE discovery on platforms that hide the device address (macOS).
pub fn selected_device_name() -> Option<String> {
  DEVICE_NAME.try_with(Clone::clone).unwrap_or_default()
}

/// Select the BLE device by a part of its advertised name within this future.
pub async fn with_device_name<F: Future>(name: Option<String>, future: F) -> F::Output {
  DEVICE_NAME.scope(name, future).await
}

pub fn selected_transport() -> Transport {
  TRANSPORT.try_with(|t| *t).unwrap_or_default()
}

/// Select the physical USB port without requiring a Bluetooth address.
pub async fn with_usb_port<F: Future>(port: Option<String>, future: F) -> F::Output {
  USB_PORT.scope(port, future).await
}

/// Select a transport for controller calls within this future.
pub async fn with_transport<F: Future>(transport: Transport, future: F) -> F::Output {
  TRANSPORT.scope(transport, future).await
}

pub(crate) enum DeviceConnection {
  Classic(ClassicConnection),
  Ble(BleConnection),
  Usb(crate::usb_control::UsbConnection),
}

impl DeviceConnection {
  pub async fn connect(address: Address) -> Result<Self, Box<dyn Error>> {
    let transport = selected_transport();
    if matches!(transport, Transport::Usb) {
      let port = USB_PORT.try_with(Clone::clone).unwrap_or_default();
      return Ok(Self::Usb(crate::usb_control::UsbConnection::connect(port.as_deref()).await?));
    }
    if !matches!(transport, Transport::Ble) {
      match ClassicConnection::connect(address).await {
        Ok(mut connection) => {
          if matches!(transport, Transport::Auto) {
            let probe = Packet {
              command: crate::protocol::command::Command::GetVolume,
              payload: vec![],
            };
            match connection.send_and_receive(&probe).await {
              Ok(_) => return Ok(Self::Classic(connection)),
              Err(error) => {
                log::info!("RFCOMM did not answer the read-only connection probe ({error}); falling back to BLE");
                connection.disconnect().await?;
              }
            }
          } else {
            return Ok(Self::Classic(connection));
          }
        }
        Err(error) if matches!(transport, Transport::Auto) => {
          log::info!("RFCOMM connection failed ({error}); falling back to BLE");
        }
        Err(error) => return Err(error),
      }
    }
    Ok(Self::Ble(
      tokio::time::timeout(Duration::from_secs(30), BleConnection::connect(address)).await??,
    ))
  }

  pub async fn fire_and_forget(&mut self, packet: &Packet) -> Result<(), Box<dyn Error>> {
    match self {
      Self::Classic(c) => c.fire_and_forget(packet).await,
      Self::Ble(c) => c.send(packet, None).await.map(|_| ()),
      Self::Usb(c) => c.send(packet, false).await,
    }
  }

  /// Send a stream packet without discarding asynchronous device update events.
  pub async fn stream_write(&mut self, packet: &Packet) -> Result<(), Box<dyn Error>> {
    match self {
      Self::Classic(c) => c.fire_and_forget(packet).await,
      Self::Ble(c) => c.send_inner(packet, None, true).await.map(|_| ()),
      Self::Usb(c) => c.send(packet, true).await,
    }
  }

  pub async fn send_and_receive(&mut self, packet: &Packet) -> Result<Response, Box<dyn Error>> {
    self.exchange(packet, packet.command.value(), &[]).await
  }

  pub async fn exchange(
    &mut self,
    packet: &Packet,
    expected: u8,
    prefix: &[u8],
  ) -> Result<Response, Box<dyn Error>> {
    match self {
      Self::Classic(c) => c.exchange(packet, expected, prefix).await,
      Self::Usb(c) => c.exchange(packet, expected, prefix).await,
      Self::Ble(c) => c
        .send(packet, Some((expected, prefix)))
        .await?
        .ok_or_else(|| "Missing command response".into()),
    }
  }

  pub fn transport_name(&self) -> &'static str {
    match self {
      Self::Classic(_) => "rfcomm",
      Self::Ble(_) => "ble",
      Self::Usb(_) => "usb",
    }
  }

  pub async fn receive(&mut self, timeout: Duration) -> Result<Option<Response>, Box<dyn Error>> {
    if let Self::Usb(c) = self { return c.receive(timeout).await; }
    if let Self::Ble(c) = self {
      if let Some(reply) = c.pending.pop_front() {
        return Ok(Some(reply));
      }
    }
    let rx = match self {
      Self::Classic(c) => &mut c.response_rx,
      Self::Ble(c) => &mut c.responses,
      Self::Usb(_) => unreachable!("USB response handled above"),
    };
    match tokio::time::timeout(timeout, rx.recv()).await {
      Ok(Some(reply)) => Ok(Some(reply)),
      Ok(None) => Err("Device response channel closed".into()),
      Err(_) => Ok(None),
    }
  }

  pub async fn disconnect(self) -> Result<(), Box<dyn Error>> {
    match self {
      Self::Classic(c) => c.disconnect().await,
      Self::Ble(mut c) => c.device.release().await,
      Self::Usb(c) => c.disconnect().await,
    }
  }

  pub async fn delay(&mut self, duration: Duration) -> Result<(), Box<dyn Error>> {
    if let Self::Usb(c) = self { c.delay(duration).await?; }
    else { tokio::time::sleep(duration).await; }
    Ok(())
  }
}

fn envelope(payload: &[u8], sequence: u16) -> Result<Vec<u8>, Box<dyn Error>> {
  let length = u16::try_from(payload.len() + 7)?;
  let mut wire = vec![0xfe, 0xef, 0xaa, 0x55];
  wire.extend(length.to_le_bytes());
  wire.extend(sequence.to_be_bytes());
  wire.extend([0, 0, 0]);
  wire.extend(payload);
  let checksum = wire[4..]
    .iter()
    .fold(0u16, |sum, b| sum.wrapping_add(*b as u16));
  wire.extend(checksum.to_le_bytes());
  Ok(wire)
}

fn parse_frames(buffer: &mut Vec<u8>) -> Result<Vec<Response>, Box<dyn Error + Send + Sync>> {
  let mut replies = Vec::new();
  while buffer.len() >= 3 {
    let size = u16::from_le_bytes([buffer[1], buffer[2]]) as usize + 4;
    if buffer[0] != 1 || size < 9 {
      return Err("Invalid BLE response framing".into());
    }
    if buffer.len() < size {
      break;
    }
    replies.push(Response::deserialize(&buffer[..size])?);
    buffer.drain(..size);
  }
  Ok(replies)
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn captured_brightness_packet() -> Result<(), Box<dyn Error>> {
    assert_eq!(
      hex::encode(envelope(&[0x74, 0], 0x0101)?),
      "feefaa550900010100000074007f00"
    );
    Ok(())
  }
  #[test]
  fn fragmented_and_concatenated_notifications() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut buffer = hex::decode("01090004335501")?;
    assert!(parse_frames(&mut buffer)?.is_empty());
    buffer.extend(hex::decode("000000960002010a000437550157ab0400a10102")?);
    let replies = parse_frames(&mut buffer)?;
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0].original_command, 0x33);
    assert_eq!(replies[1].data, [1, 0x57, 0xab, 4, 0]);
    assert!(buffer.is_empty());
    Ok(())
  }
  #[test]
  fn corrupt_notification_is_rejected() -> Result<(), Box<dyn Error>> {
    let mut buffer = hex::decode("01090004335501000000970002")?;
    assert!(parse_frames(&mut buffer).is_err());
    Ok(())
  }
}
