//! Module to poll the Lakeshore temperature controller.
//!
//! This module provides the interface for all the things we want to do with the Lakeshore
//! temperature controller from the GUI. These tasks are:
//! - Autodetect the serial port the Lakeshore is connected to and use it.
//! - Reading the temperatures of two channels and returning the channel names and temperatures.
//! - Error handling in case the device is lost, etc.

use std::collections::HashMap;

use anyhow::{Result, bail};

use lakeshore336::{
    Input, Lakeshore336, Output,
    types::{
        HeaterMaxOutputCurrent, HeaterOutputDisplay, HeaterResistance, HeaterSetup, OnPowerup,
        OutputMode, OutputModeSetup, heater_setup::HeaterRange,
    },
};

use measurements::Temperature;
use serde::{Deserialize, Serialize};
use serialport::{SerialPort, SerialPortType};

use crate::instruments::utils::ThermocoupleChannelName;

/// Cryostorage chamber's way of looking at a Lakeshore temperature controller.
pub struct LakeshoreTempInst {
    config: LakeshoreTempConfig,
    instrument: Option<Lakeshore336<Box<dyn SerialPort>>>,
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
        let interface = lakeshore336::new_serialport(port).open()?;
        let instrument = Lakeshore336::new(interface);
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
                    let input = get_channel_input_idx(idx)?;
                    let temp_k = inst.channel(input).get_temperature()?;
                    ret_map.insert(ch_name.clone(), temp_k);
                }
            }

            return Ok(ret_map);
        }

        bail!("Lakeshore temperature controller not connected (should be unreachable)");
    }

    /// Set the heater to the predefined value and the given temperature.
    /// further.
    pub fn set_heater_on(&mut self) -> Result<()> {
        self.check_connection()?;

        if let Some(inst) = self.instrument.as_mut() {
            let outp = self.config.heater_output_channel;
            // Heater setup for output channel.
            inst.output(outp)
                .set_heater_setup(self.config.heater_setup)?;
            // set the output mode
            inst.output(outp)
                .set_output_mode(self.config.heater_output_mode)?;
            // Turn the heater on.
            inst.output(outp)
                .set_range(self.config.heater_output_range)?;
            Ok(())
        } else {
            bail!(
                "Could not set heater: Lakeshore temperature controller not connected (should be unreachable)"
            );
        }
    }

    /// Turn the heater off.
    pub fn set_heater_off(&mut self) -> Result<()> {
        self.check_connection()?;

        if let Some(inst) = self.instrument.as_mut() {
            inst.output(self.config.heater_output_channel)
                .set_range(HeaterRange::Off)?;
        }
        Ok(())
    }

    /// Get the heater setpoint temperature.
    pub fn get_heater_temperature(&mut self) -> Result<Temperature> {
        self.check_connection()?;
        if let Some(inst) = self.instrument.as_mut() {
            let temp = inst
                .output(self.config.heater_output_channel)
                .get_setpoint()?;
            Ok(temp)
        } else {
            bail!(
                "Could not get heater setpoint: Lakeshore temperature controller not connected (should be unreachable)"
            );
        }
    }

    /// Set the heater setpoint temperature.
    pub fn set_heater_temperature(&mut self, temperature: Temperature) -> Result<()> {
        self.check_connection()?;
        if let Some(inst) = self.instrument.as_mut() {
            inst.output(self.config.heater_output_channel)
                .set_setpoint(temperature)?;
            Ok(())
        } else {
            bail!(
                "Could not set heater setpoint: Lakeshore temperature controller not connected (should be unreachable)"
            );
        }
    }

    /// Reset the instrument to None, so that it reconnects on the next read.
    pub fn reset_instrument(&mut self) {
        self.instrument = None;
    }
}

/// Get the channel input based on an enumerated usize.
///
/// 0 -> Input::InA
/// 1 -> Input::InB
/// 2 -> Input::InC
/// 3 -> Input::InD
pub fn get_channel_input_idx(idx: usize) -> Result<Input> {
    match idx {
        0 => Ok(Input::InA),
        1 => Ok(Input::InB),
        2 => Ok(Input::InC),
        3 => Ok(Input::InD),
        _ => bail!("Channel input out of range."),
    }
}

/// Configuration to store for the Lakeshore temperature controller.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LakeshoreTempConfig {
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
    /// The output channel used for heater.
    pub heater_output_channel: Output,
    /// Setup of the heater (HTRSET)
    pub heater_setup: HeaterSetup,
    /// Heater output mode.
    pub heater_output_mode: OutputModeSetup,
    /// Heater output range.
    pub heater_output_range: HeaterRange,
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
            usb_prod_info: Some(String::from("Model 336 Temperature Controller")),
            channel_a_name: Some(ThermocoupleChannelName::Sample),
            channel_b_name: Some(ThermocoupleChannelName::Bridge),
            channel_c_name: None,
            channel_d_name: None,
            heater_output_channel: Output::Out1,
            heater_setup: HeaterSetup::try_new(
                HeaterResistance::R25Ohm,
                HeaterMaxOutputCurrent::C2A,
                HeaterOutputDisplay::Current,
            )
            .expect("Valid config"),
            heater_output_mode: OutputModeSetup::new(
                OutputMode::ClosedLoopPID,
                Some(Input::InA),
                OnPowerup::Disabled,
            ),
            heater_output_range: HeaterRange::Medium,
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
