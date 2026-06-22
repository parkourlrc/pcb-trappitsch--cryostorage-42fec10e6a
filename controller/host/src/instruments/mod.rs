//! Instruments module.
//!
//! This module handles communication with all peripherals. These are:
//! - Pfeiffer HiCube turbomolecular pump.
//! - Pfeiffer Vacuum gauge controller (two gauges, transfer and main chamber).
//! - Lakeshore temperature controller, to measure two different temperatures.
//! - Cryocooler
//!
//! This file contains two parts:
//! - A monitoring task that needs to run in its own thread to poll instruments periodically.
//! - An async executor to send commands to instruments and change their state.
//!
//! The monitoring tasks will need to run in its own thread as the instruments are polled
//! frequently and blocking calls are needed. However, we don't want to block the entire program
//! regularly.
//!
//! The command task on the other hand can run as an async task as commands will be sent very
//! infrequently, and we will simply accept the fact that it may take half a second to set
//! something. This is acceptable for our use case. Worse case scenario will be that the interface
//! freezes until a timeout is hit.

use anyhow::{Result, anyhow};

use std::{collections::HashMap, time::Duration};

use agilent_4uhv::HvState;
use measurements::{Power, Temperature};
use sunpower_cryotelgt::CoolerState;
use tokio::sync::{OnceCell, mpsc};

use crate::{
    app::get_ui_wait,
    dialog::error_dialog_helper,
    get_inst_status_wait, get_prg_conf_wait,
    instruments::{
        cryocooler::CryoCoolerInst,
        hi_cube::HiCubeCommands,
        ion_pump::IonPumpInst,
        lakeshore_temp::LakeshoreTempInst,
        omnicontrol::{Gauge, GaugeStatus, OmniControlInst},
        utils::ThermocoupleChannelName,
    },
    log,
    safety::sample_safety,
    status::PumpState,
};

pub mod cryocooler;
pub mod hi_cube;
pub mod ion_pump;
pub mod lakeshore_temp;
pub mod omnicontrol;
pub mod utils;

const POLLING_INTERVAL: Duration = Duration::from_secs(5);

pub static INSTRUMENT_COMMAND_SENDER: OnceCell<mpsc::Sender<InstrumentCommands>> =
    OnceCell::const_new();
pub static HICUBE_COMMAND_SENDER: OnceCell<mpsc::Sender<HiCubeCommands>> = OnceCell::const_new();

/// Commands that can be sent to instruments.
pub enum InstrumentCommands {
    /// Set temperature of the cryocooler.
    CryoCoolerSetpoint(Temperature),
    /// Turn the cryocooler on or off.
    CryoCoolerState(CoolerState),
    /// Set the state of the selected gauge to on or off.
    GaugeState((Gauge, GaugeStatus)),
    /// Turn the ion pump on or off.
    IonPumpState(HvState),
}

/// Monitoring task that polls the instruments periodically.
pub async fn instruments_task(mut rx_instr: mpsc::Receiver<InstrumentCommands>) {
    let prg_conf = get_prg_conf_wait().await;

    let inst_status = get_inst_status_wait().await;

    // Get all the instruments
    let mut lakeshore_temp_inst = {
        let conf = prg_conf
            .lock()
            .expect("PrgConfig lock poisoned")
            .get_lakeshore_temp_config();
        LakeshoreTempInst::new(conf)
    };

    let mut cryocooler_inst = {
        let conf = prg_conf
            .lock()
            .expect("PrgConfig lock poisoned")
            .get_cryocooler_config();
        CryoCoolerInst::new(conf)
    };

    let mut ion_pump_inst = {
        let conf = prg_conf
            .lock()
            .expect("PrgConfig lock poisoned")
            .get_ion_pump_config();
        IonPumpInst::new(conf)
    };
    let mut omnicontrol_inst = {
        let conf = prg_conf
            .lock()
            .expect("PrgConfig lock poisoned")
            .get_omnicontrol_config();
        OmniControlInst::new(conf)
    };
    // Get shutdown receiver
    let mut rx_shutdown = crate::HALT_SENDER.get().unwrap().subscribe();

    // Wait for the UI to be globally available.
    let _ = get_ui_wait().await;

    // Set the instrument initial states
    // cryocooler
    let cooler_setpoint_temperature = match cryocooler_inst.get_setpoint_temperature() {
        Ok(t) => t,
        Err(e) => {
            log::err!(
                "Failed to set initial cryocooler setpoint temperature: {}",
                e,
            )
            .await;
            cryocooler_inst.reset_instrument();
            Temperature::default()
        }
    };

    let cooler_state = match cryocooler_inst.get_state() {
        Ok(s) => s,
        Err(e) => {
            log::err!("Failed to set initial cryocooler state: {}", e,).await;
            cryocooler_inst.reset_instrument();
            CoolerState::Disabled
        }
    };

    let ion_pump_state = match ion_pump_inst.get_high_voltage() {
        Ok(HvState::On) => PumpState::On,
        Ok(HvState::Off) => PumpState::Off,
        Err(_) => {
            ion_pump_inst.reset_instrument();
            PumpState::Error
        }
    };

    // set all initial statuses of various instruments
    {
        let mut inst_status = inst_status.lock().expect("InstrumentStatus lock poisoned");

        if inst_status
            .set_temperature_setpoint_and_ui(cooler_setpoint_temperature)
            .is_err()
        {
            log::err_now!("Failed to set initial cryocooler setpoint temperature in UI");
        }

        if inst_status.set_cooler_state_and_ui(cooler_state).is_err() {
            log::err_now!("Failed to set initial cryocooler state in UI");
        }

        if inst_status
            .set_ion_pump_state_and_ui(ion_pump_state)
            .is_err()
        {
            log::err_now!("Failed to set initial ion pump state in UI");
        }
    } // drop lock

    // Get the max safe sample temperature from the program configuration (only needed once).
    let (min_safe_sample_temp, max_safe_sample_temp) = prg_conf
        .lock()
        .expect("ProgramConfig lock poisoned")
        .get_safe_sample_temperatures();

    // set later to a reasonable value
    let mut polling_interval = Duration::from_millis(1);

    loop {
        tokio::select! {
            // Loop that polls all instruments
            _ = tokio::time::sleep(polling_interval) => {
                // Temperatures
                let mut temperatures = match lakeshore_temp_inst.get_status_measurements() {
                    Ok(temps) => temps,
                    Err(e) => {
                        log::err!("Failed to read temperatures from Lakeshore336: {}", e).await;
                        lakeshore_temp_inst.reset_instrument();
                        HashMap::new()
                    }
                };

                let temperature_cooler = match cryocooler_inst.get_status_measurement() {
                    Ok(temps) => temps,
                    Err(e) => {
                        log::err!("Failed to read temperature from Cryocooler: {}", e).await;
                        cryocooler_inst.reset_instrument();
                        HashMap::new()
                    }
                };

                temperatures.extend(temperature_cooler);

                inst_status
                    .lock()
                    .expect("InstrumentStatus lock poisoned")
                    .set_temperatures(
                        *temperatures.get(&ThermocoupleChannelName::Bridge).unwrap_or(&Temperature::default()),
                        *temperatures.get(&ThermocoupleChannelName::Cooler).unwrap_or(&Temperature::default()),
                        *temperatures.get(&ThermocoupleChannelName::Sample).unwrap_or(&Temperature::default())
                    );

                // Cryocooler current power
                let current_power = match cryocooler_inst.get_current_power() {
                    Ok(p) => p,
                    Err(e) => {
                        log::err!("Failed to read current power from Cryocooler: {}", e).await;
                        cryocooler_inst.reset_instrument();
                        Power::default()
                    }
                };
                inst_status.lock().expect("InstrumentStatus lock poisoned")
                    .set_power_cooler_current(current_power);

                // Pressures read from Omnicontrol
                let mut pressures = HashMap::new();
                match omnicontrol_inst.get_pressures() {
                    Ok(phm) => {
                        pressures.extend(phm);
                    }
                    Err(e) => {
                        log::err!("Failed to read pressures from Omnicontrol: {}", e).await;
                        omnicontrol_inst.reset_instrument();
                    }
                };

                inst_status.lock().expect("InstrumentStatus lock poisoned")
                    .set_pressures(pressures);

                // Get the status of the ion pump. Poll regularly in case it turned off.
                let ion_pump_state = match ion_pump_inst.get_high_voltage() {
                    Ok(HvState::On) => PumpState::On,
                    Ok(HvState::Off) => PumpState::Off,
                    Err(_) => {
                        ion_pump_inst.reset_instrument();
                        PumpState::Error
                    }
                };

                if inst_status.lock().expect("InstrumentStatus lock poisoned")
                    .set_ion_pump_state_and_ui(ion_pump_state).is_err()
                {
                    log::err_now!("Failed to set ion pump state in UI");
                }

                // Update UI
                inst_status.lock().expect("InstrumentStatus lock poisoned")
                    .update_ui_instrument_status();

                // Check for sample safety if any samples are loaded.
                let smpls_empty = prg_conf.lock().expect("ProgramConfig lock poisoned").is_samples_empty();
                if !smpls_empty {
                    let is = inst_status.lock().expect("InstrumentStatus lock poisoned");
                    sample_safety(&is, (&min_safe_sample_temp, &max_safe_sample_temp));
                }

                // after init, set polling interval to normal value
                if polling_interval != POLLING_INTERVAL {
                    polling_interval = POLLING_INTERVAL;
                }
            }
            // Instrument command received
            Some(cmd) = rx_instr.recv() => {
                match cmd {
                    InstrumentCommands::CryoCoolerSetpoint(temperature) => {
                        match cryocooler_inst.set_setpoint_temperature(temperature) {
                            Ok(_) => {
                                inst_status.lock().expect("InstrumentStatus lock poisoned")
                                    .set_temperature_setpoint_and_ui(temperature)
                                    .expect("UI set before this loop started.");
                            },
                            Err(e) => {
                                log::err!("Failed to set cryocooler setpoint temperature: {}", e).await;
                            }
                        }
                    }
                    InstrumentCommands::CryoCoolerState(state) => {
                        match cryocooler_inst.set_state(state.clone()) {
                            Ok(_) => {
                                inst_status.lock().expect("InstrumentStatus lock poisoned")
                                    .set_cooler_state_and_ui(state)
                                    .expect("UI set before this loop started.");
                            },
                            Err(e) => {
                                log::err!("Failed to set cryocooler state: {}", e).await;
                                error_dialog_helper(
                                    anyhow!("Setting cryocooler state failed. Please check connections and try again!")
                                );
                            }
                        }
                    }
                    InstrumentCommands::GaugeState((gauge, state)) => {
                        if omnicontrol_inst.set_status(gauge, state).is_err() {
                            log::err!("Failed to set {} gauge to state: {}", gauge, state).await;
                        }
                    }
                    InstrumentCommands::IonPumpState(state) => {
                        let st = match ion_pump_inst.set_high_voltage(state.clone()) {
                            Ok(_) => {
                                match state {
                                    HvState::On => PumpState::On,
                                    HvState::Off => PumpState::Off,
                                }
                            },
                            Err(e) => {
                                ion_pump_inst.reset_instrument();
                                log::err!("Failed to set ion pump state: {}", e).await;
                                PumpState::Error
                            }
                        };
                        inst_status.lock().expect("InstrumentStatus lock poisoned")
                            .set_ion_pump_state_and_ui(st)
                            .expect("UI set before this loop started.");
                    }
                }
            }
            // Shutdown signal received
            _ = rx_shutdown.recv() => {
                break;
            }
        }
    }
}

/// Get a clone of the instrument command sender.
fn get_instr_cmd_sender() -> mpsc::Sender<InstrumentCommands> {
    INSTRUMENT_COMMAND_SENDER
        .get()
        .expect("Uninitialized")
        .clone()
}

/// Convenience function to send an instrument command without awaiting.
///
/// If an error occurs, this error is logged. Otherwise, the program will continue
/// as normal.
pub fn send_instr_cmd_now(cmd: InstrumentCommands) {
    let sender = get_instr_cmd_sender();
    if let Err(e) = sender.try_send(cmd) {
        log::err_now!("Failed to send instrument command now: {}", e);
    }
}

/// Convenience function to send an instrument command and awaiting it.
///
/// If an error occurs, the error is logged and passed on!
pub async fn send_instr_cmd_w_err(cmd: InstrumentCommands) -> Result<()> {
    let sender = get_instr_cmd_sender();
    if let Err(e) = sender.send(cmd).await {
        log::err!("Failed to send instrument command now: {}", e).await;
        return Err(e.into());
    }
    Ok(())
}
