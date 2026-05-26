//! Workflow to start/stop baking

use std::sync::{Arc, Mutex};

use anyhow::{Result, bail};
use icd::{BakingState, ValveState};
use slint::ComponentHandle;

use crate::{
    app::{Logic, UI},
    controller::{ControllerCommands, send_cntrl_cmd_now},
    log,
    prg_config::Authorizations,
    status::{InstrumentStatus, PressureReading},
    workflows::valves::set_pump_valve,
};

/// Start baking permission check.
///
/// Returns error if:
/// - cryocooler is on
/// - chamber pressure is not baking max_pressure limit
/// - pump valve cannot be opened (if it is closed)
fn get_start_baking_permission(
    inst_status: Arc<Mutex<InstrumentStatus>>,
    auths: &Authorizations,
) -> Result<()> {
    // Is the cryocooler off?
    if !inst_status.lock().expect("Poisoned").is_cryo_cooler_off() {
        bail!("Please turn cryocooler off.");
    }

    // Is chamber pressure value valid?
    let p_curr_mbar = match inst_status.lock().expect("Poisoned").get_pressures().0 {
        PressureReading::Value(p) => p.as_millibars(),
        _ => bail!("Chamber pressure gauge error: check gauge and try again."),
    };

    // Is the chamber pressure low enough to start baking?
    if p_curr_mbar >= auths.baking.max_chamber_pressure_mbar {
        bail!(
            "Chamber pressure is too high to start baking.\n 
            Current value: {:.2E} mbar\n
            Maximum allowed: {:.2E} mbar",
            p_curr_mbar,
            auths.baking.max_chamber_pressure_mbar
        )
    };

    // Is the pump valve open?
    if !inst_status.lock().expect("Poisoned").is_pump_valve_open() {
        set_pump_valve(inst_status, auths, ValveState::Open)?;
        // if we opened successfully, we need to toggle the UI switch too
        if let Some(ui) = UI.get() {
            ui.upgrade_in_event_loop(move |ui| {
                ui.global::<Logic>().set_pump_valve_is_open(true);
            })
            .expect("UI must be alive");
        } else {
            log::err_now!("Cannot get a handle to the UI in get start baking permisison.")
        }
    }

    Ok(())
}

/// Function to st the baking
pub fn set_baking(
    inst_status: Arc<Mutex<InstrumentStatus>>,
    auths: &Authorizations,
    baking_state: BakingState,
) -> Result<()> {
    if baking_state != BakingState::Off {
        get_start_baking_permission(inst_status, auths)?;
    };
    send_cntrl_cmd_now(ControllerCommands::Baking(baking_state));
    Ok(())
}
