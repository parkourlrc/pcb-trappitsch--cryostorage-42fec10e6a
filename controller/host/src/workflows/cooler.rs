//! Workflows for starting and stopping the cryocooler.

use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow, bail};

use sunpower_cryotelgt::CoolerState;

use crate::{
    PRG_CONF,
    instruments::{InstrumentCommands, send_instr_cmd_now},
    prg_config::Authorizations,
    status::{InstrumentStatus, PressureReading},
};

/// Get permission to start the cryocooler.
///
/// Checks for active watercooling and safe pressure levels.
fn get_start_cooler_permission(
    inst_status: Arc<Mutex<InstrumentStatus>>,
    auths: &Authorizations,
) -> Result<()> {
    let p_chamber = inst_status.lock().expect("Poisoned").get_pressures().0;

    let p_chamber_mbar = match p_chamber {
        PressureReading::Value(val) => val.as_millibars(),
        _ => bail!("Chamber pressure gauge error: Check gauge and try again."),
    };

    if !inst_status.lock().expect("Poisoned").is_baking_off() {
        bail!("Baking is on: cannot start cooler.")
    };

    // FIXME: This needs to be reactivated when water flow read is good.
    // if !inst_status.lock().expect("Poisoned").is_water_flow_ok() {
    //     bail!("Water flow error: cannot start cooler.")
    // };

    if p_chamber_mbar >= auths.cryo_cooler.max_pressure_on_mbar {
        bail!("Chamber pressure is too high: cannot start cooler.")
    }

    if !inst_status.lock().expect("Poisoned").is_ion_pump_on() {
        bail!("Ion pump must be on to start the cryocooler.")
    }

    if inst_status.lock().expect("Poisoned").is_pump_valve_open() {
        bail!("Pump valve must be closed to start the cryocooler.")
    }

    Ok(())
}

/// Get permission to stop the cryocooler.
///
/// Checks if any samples are currently loaded in the UI. This is not perfect, however should
/// prevent turning off the cryocooler by accident.
fn get_stop_cooler_permission() -> Result<()> {
    match PRG_CONF.get() {
        Some(pc) => {
            if !pc
                .lock()
                .expect("ProgramConfig lock is poisoned")
                .is_samples_empty()
            {
                bail!("The cryocooler cannot be stopped if samples are currently loaded.");
            }
        }
        _ => bail!("Cannot obtain the program config. This should not be possible. Report as bug."),
    }
    Ok(())
}

/// Function to set the cryocooler.
pub fn set_cooler(
    inst_status: Arc<Mutex<InstrumentStatus>>,
    auths: &Authorizations,
    cooler_state: CoolerState,
) -> Result<()> {
    if cooler_state == CoolerState::Enabled {
        get_start_cooler_permission(inst_status, auths)?
    } else {
        get_stop_cooler_permission()?
    }
    send_instr_cmd_now(InstrumentCommands::CryoCoolerState(cooler_state));
    Ok(())
}
