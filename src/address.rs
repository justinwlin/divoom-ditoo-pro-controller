//! Bluetooth device address. On Linux this is `bluer::Address`. Other platforms get an equivalent type, because
//! their Bluetooth stacks do not expose `bluer`.

#[cfg(target_os = "linux")]
pub use bluer::Address;

#[cfg(not(target_os = "linux"))]
mod portable {
  use std::{fmt, str::FromStr};

  /// A 6-byte Bluetooth address, written as `11:22:33:44:55:66`.
  #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
  pub struct Address(pub [u8; 6]);

  impl Address {
    pub const fn new(bytes: [u8; 6]) -> Self {
      Self(bytes)
    }

    /// The all-zero address, used when the platform selects the device by name.
    pub const fn any() -> Self {
      Self([0; 6])
    }
  }

  impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      let b = self.0;
      write!(f, "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", b[0], b[1], b[2], b[3], b[4], b[5])
    }
  }

  #[derive(Debug)]
  pub struct InvalidAddress(String);

  impl fmt::Display for InvalidAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      write!(f, "invalid Bluetooth address: {}", self.0)
    }
  }

  impl std::error::Error for InvalidAddress {}

  impl FromStr for Address {
    type Err = InvalidAddress;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
      let parts: Vec<&str> = s.split(':').collect();
      if parts.len() != 6 {
        return Err(InvalidAddress(s.to_string()));
      }
      let mut bytes = [0u8; 6];
      for (byte, part) in bytes.iter_mut().zip(parts) {
        if part.len() != 2 {
          return Err(InvalidAddress(s.to_string()));
        }
        *byte = u8::from_str_radix(part, 16).map_err(|_| InvalidAddress(s.to_string()))?;
      }
      Ok(Self(bytes))
    }
  }
}

#[cfg(not(target_os = "linux"))]
pub use portable::Address;
