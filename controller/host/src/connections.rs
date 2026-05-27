//! This module holds connection adapters, i.e., how we connect to different instruments.
//!
//! All adapters must be ser/de compliant to be saved in the program config file.

use std::{net::Ipv4Addr, time::Duration};

use serde::{Deserialize, Serialize};

/// Timeout for TCP/IP connections
pub const TCP_IP_TIMEOUT: Duration = Duration::from_millis(500);

/// A TCP/IP adapter that connects to instruments via the Moxa serial device server.
///
/// Only IP and port are needed as the Moxa handles the serial connection internally.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TcpIpAdapter {
    pub ip: Ipv4Addr,
    pub port: u16,
}

impl TcpIpAdapter {
    /// Create a new TCP/IP adapter from IP and port given in one string.
    ///
    /// If invalid format is provided, the default adapter is returned.
    pub fn new_from_str(addr: &str) -> Self {
        let parts = addr.split(':').collect::<Vec<&str>>();
        if parts.len() != 2 {
            return Self::default();
        }

        let ip = parts[0]
            .parse::<Ipv4Addr>()
            .unwrap_or(Ipv4Addr::new(192, 168, 1, 2));
        let port = parts[1].parse::<u16>().unwrap_or(4001);

        Self { ip, port }
    }
    /// Get the simple address string to use with `InstrumentRs`.
    pub fn get_address(&self) -> String {
        format!("{}:{}", self.ip, self.port)
    }
}

impl Default for TcpIpAdapter {
    fn default() -> Self {
        Self {
            ip: Ipv4Addr::new(192, 168, 1, 2),
            port: 4001,
        }
    }
}

/// The adapter for connecting to the poststation server.
///
/// A combination of the TCP/IP adapter and a serial number of the device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoststationAdapter {
    pub product_name: String,
    pub tcp_ip: TcpIpAdapter,
}

impl Default for PoststationAdapter {
    fn default() -> Self {
        Self {
            product_name: String::from("Cryostorage Controller"),
            tcp_ip: TcpIpAdapter::new_from_str("127.0.0.1:51837"),
        }
    }
}

impl PoststationAdapter {
    /// Get the address string to connect to.
    pub fn get_address(&self) -> String {
        self.tcp_ip.get_address()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_tcp_ip_adapter_address() {
        let adapter = TcpIpAdapter {
            ip: Ipv4Addr::new(192, 168, 1, 100),
            port: 4001,
        };
        assert_eq!(adapter.get_address(), "192.168.1.100:4001");
    }

    #[test]
    fn test_tcp_ip_adapter_from_str() {
        let addr = "127.0.0.3:5555";
        let adapter = TcpIpAdapter::new_from_str(addr);
        assert_eq!(adapter.get_address(), addr);
    }
}
