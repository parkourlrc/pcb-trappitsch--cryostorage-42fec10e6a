//! Module to set/poll the Agilent 4UHV Ion Pump Controller.
//!
//! This module provides the interface for the Agilent 4UHV Ion Pump Controller in order to control
//! this pump from the GUI.
//!
//! The tasks are:
//! - Turn ion pump on/off
//! - Poll its status

use std::net::{SocketAddr, TcpStream};

use anyhow::{Result, bail};

use agilent_4uhv::{Agilent4Uhv, Channel, HvState};
use instrumentrs::{Instrument, TcpIpInterface};
use serde::{Deserialize, Serialize};

use crate::connections::{TCP_IP_TIMEOUT, TcpIpAdapter};

/// Agilent 4UHV Ion Pump Controller instrument.
pub struct IonPumpInst {
    /// Ion pump configuration
    pub config: IonPumpConfig,
    /// Instrument connection, if connected.
    instrument: Option<Agilent4Uhv<Instrument<TcpStream>>>,
    /// Channel connection, if connected.
    channel: Option<Channel<Instrument<TcpStream>>>,
}

impl IonPumpInst {
    /// Create a new IonPump instance.
    pub fn new(config: IonPumpConfig) -> Self {
        Self {
            config,
            instrument: None,
            channel: None,
        }
    }

    /// Check the connection and if it's none, connect.
    fn check_connection(&mut self) -> Result<()> {
        if self.instrument.is_none() || self.channel.is_none() {
            self.connect()
        } else {
            Ok(())
        }
    }

    /// Connect to the ion pump controller and store instrument in self.
    fn connect(&mut self) -> Result<()> {
        let addr = self.config.tcp_ip_adapter.get_address();
        let socket_addr: SocketAddr = addr.parse()?;

        let stream = TcpStream::connect_timeout(&socket_addr, TCP_IP_TIMEOUT)?;
        stream.set_write_timeout(Some(TCP_IP_TIMEOUT))?;
        stream.set_read_timeout(Some(TCP_IP_TIMEOUT))?;

        let interface = TcpIpInterface::full(stream)?;
        let mut instrument = Agilent4Uhv::try_new(interface)?;
        let channel = instrument.get_channel(self.config.channel.into())?;

        self.instrument = Some(instrument);
        self.channel = Some(channel);

        Ok(())
    }

    /// Get the High Voltage state of the configured channel.
    pub fn get_high_voltage(&mut self) -> Result<HvState> {
        self.check_connection()?;
        if let Some(channel) = &mut self.channel {
            let state = channel.get_hv_state()?;
            return Ok(state);
        }

        bail!("Channel not connected (should be unreachable)");
    }

    /// Set the High Voltage state of the configured channel.
    ///
    /// As the argument we use the `HvState` enum.
    pub fn set_high_voltage(&mut self, state: HvState) -> Result<()> {
        self.check_connection()?;
        if let Some(channel) = &mut self.channel {
            channel.set_hv_state(state)?;
        }
        Ok(())
    }

    /// Reset the instrument to None, so that it reconnects on next use.
    pub fn reset_instrument(&mut self) {
        self.instrument = None;
        self.channel = None;
    }
}

/// Configuration for the Agilent 4UHV Ion Pump Controller.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IonPumpConfig {
    pub tcp_ip_adapter: TcpIpAdapter,
    pub channel: IonPumpChannel,
}

impl Default for IonPumpConfig {
    fn default() -> Self {
        IonPumpConfig {
            tcp_ip_adapter: TcpIpAdapter::new_from_str("192.168.1.2:4001"),
            channel: IonPumpChannel::Channel1,
        }
    }
}

/// This ion pump has four channels.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum IonPumpChannel {
    Channel1,
    Channel2,
    Channel3,
    Channel4,
}

/// Convert IonPumpChannel to usize for indexing in driver.
impl From<IonPumpChannel> for usize {
    fn from(channel: IonPumpChannel) -> Self {
        match channel {
            IonPumpChannel::Channel1 => 0,
            IonPumpChannel::Channel2 => 1,
            IonPumpChannel::Channel3 => 2,
            IonPumpChannel::Channel4 => 3,
        }
    }
}
