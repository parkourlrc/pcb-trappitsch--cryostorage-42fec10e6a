//! Module to poll the Lakeshore temperature controller.
//!
//! This module provides the interface for all the things we want to do with the Lakeshore
//! temperature controller from the GUI. These tasks are:
//! - Autodetect the serial port the Lakeshore is connected to and use it.
//! - Reading the temperatures of two channels and returning the channel names and temperatures.
//! - Error handling in case the device is lost, etc.

use std::collections::HashMap;

use anyhow::{Result, bail};

use instrumentrs::Instrument;
use lakeshore_336::{Lakeshore336, SerialInterfaceLakeshore};

use measurements::Temperature;
use serde::{Deserialize, Serialize};
use serialport::{SerialPort, SerialPortType};

use crate::instruments::utils::ThermocoupleChannelName;

/// Cryostorage chamber's way of looking at a Lakeshore temperature controller.
pub struct LakeshoreTempInst {
    config: LakeshoreTempConfig,
    instrument: Option<Lakeshore336<Instrument<Box<dyn SerialPort>>>>,
}

impl LakeshoreTempInst {
    /// Create a new Lakeshore temperature controller instance.
    pub fn new(config: LakeshoreTempConfig) -> Self {
        Self {
            config,
            instrument: None,
        }
    }

    // Check the connection and if it's none, connect.
    //
    // If the connection fails, the stored connection simply remains None.
    fn check_connection(&mut self) -> Result<()> {
        if self.instrument.is_none() {
            self.connect()
        } else {
            Ok(())
        }
    }

    // Connect to the Lakeshore temperature controller and store the interface in self.
    fn connect(&mut self) -> Result<()> {
        let product_expected = match &self.config.usb_prod_info {
            Some(s) => s.clone(),
            None => bail!("No USB product info string provided for Lakeshore temp controller"),
        };
        let port = find_port_by_product(&product_expected)?;
        let interface = SerialInterfaceLakeshore::simple(&port)?;
        let instrument = Lakeshore336::try_new(interface)?;
        self.instrument = Some(instrument);
        Ok(())
    }

    /// Read the temperatures and return them.
    ///
    /// We return a HashMap with the name of the channel as the key and the temperature as the
    /// value.
    /// An error is returned if the we cannot read the temperatures for any reason.
    pub fn get_status_measurements(
        &mut self,
    ) -> Result<HashMap<ThermocoupleChannelName, Temperature>> {
        // Do we need to connect again?
        self.check_connection()?;

        if let Some(inst) = self.instrument.as_mut() {
            let mut ret_map = HashMap::new();

            for (idx, name) in self.config.channel_iter().enumerate() {
                // for each populated channel, get the temperature
                if let Some(ch_name) = name {
                    let temp_k = inst.get_channel(idx)?.get_temperature()?;
                    ret_map.insert(ch_name.clone(), temp_k);
                }
            }

            return Ok(ret_map);
        }

        bail!("Lakeshore temperature controller not connected (should be unreachable)");
    }

    /// Set the heater to the predefined value and the given temperature.
    ///
    /// FIXME: This is a temporary workaround as I don't have time right now to develop the driver
    /// further.
    pub fn set_heater_on(&mut self) -> Result<()> {
        self.check_connection()?;

        if let Some(inst) = self.instrument.as_mut() {
            // Set the heater setup for output 1.
            inst.sendcmd("HTRSET 1,1,4,0,2")?;
            // set the output mode
            inst.sendcmd("OUTMODE 1,1,1,0")?;
            // Turn the heater on.
            inst.sendcmd("RANGE 1,2")?;
        }
        Ok(())
    }

    /// Turn the heater off.
    ///
    /// FIXME: This is a temporary workaround as I don't have time right now to develop the driver
    /// further.
    pub fn set_heater_off(&mut self) -> Result<()> {
        self.check_connection()?;

        if let Some(inst) = self.instrument.as_mut() {
            inst.sendcmd("RANGE 1,0")?;
        }
        Ok(())
    }

    /// Set the heater temperature.
    ///
    /// Sets the temperature that is stored in the program config to the instrument.
    ///
    /// FIXME: This is a temporary workaround as I don't have time right now to develop the driver
    /// further.
    pub fn set_heater_temperature(&mut self, temperature: Temperature) -> Result<()> {
        self.check_connection()?;
        if let Some(inst) = self.instrument.as_mut() {
            let cmd = format!("SETP 1,{:.1}", temperature);
            inst.sendcmd(&cmd)?;
        }
        Ok(())
    }

    /// Reset the instrument to None, so that it reconnects on the next read.
    pub fn reset_instrument(&mut self) {
        self.instrument = None;
    }
}

/// Configuration to store for the Lakeshore temperature controller.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LakeshoreTempConfig {
    /// Target temperature for the heater.
    pub sample_temperature_heater: Temperature,
    /// USB product info string to identify the device for automatic port detection.
    pub usb_prod_info: Option<String>,
    /// Channel 1/A name, or None if not present.
    pub channel_a_name: Option<ThermocoupleChannelName>,
    /// Channel 2/B name, or None if not present.
    pub channel_b_name: Option<ThermocoupleChannelName>,
    /// Channel 3/C name, or None if not present.
    pub channel_c_name: Option<ThermocoupleChannelName>,
    /// Channel 4/D name, or None if not present.
    pub channel_d_name: Option<ThermocoupleChannelName>,
}

impl LakeshoreTempConfig {
    /// Iterator over the four channels, yielding `&Option<String>`.
    pub fn channel_iter(&self) -> impl Iterator<Item = &Option<ThermocoupleChannelName>> {
        // Build a temporary slice of tuples; the slice lives only for the
        // duration of the call, so returning an `impl Iterator` is safe.
        [
            &self.channel_a_name,
            &self.channel_b_name,
            &self.channel_c_name,
            &self.channel_d_name,
        ]
        .into_iter()
    }
}

/// Some default values, according to the initial setup.
///
/// These can be changed in the configuration file, only used for writing the file the first time.
impl Default for LakeshoreTempConfig {
    fn default() -> Self {
        Self {
            sample_temperature_heater: Temperature::from_kelvin(120.0),
            usb_prod_info: Some(String::from("Model 336 Temperature Controller")),
            channel_a_name: Some(ThermocoupleChannelName::Sample),
            channel_b_name: Some(ThermocoupleChannelName::Bridge),
            channel_c_name: None,
            channel_d_name: None,
        }
    }
}

/// Find the first serial port whose USB product name contains `needle`.
///
/// # Errors
///
/// Returns an error if no matching Lakeshore 336 controller is detected.
pub fn find_port_by_product(needle: &str) -> Result<String> {
    let ports = serialport::available_ports()?;

    let port_name = ports.iter().find_map(|info| {
        if let SerialPortType::UsbPort(usb) = &info.port_type {
            // `product` is an `Option<String>`.
            usb.product
                .as_ref()
                .filter(|prod| prod.contains(needle))
                .map(|_| info.port_name.clone())
        } else {
            None
        }
    });

    match port_name {
        Some(name) => Ok(name),
        None => bail!("Lakeshore 336 not found. Is it connected?"),
    }
}
