//! Workflows for opening and closing valves.

use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use icd::ValveState;
use thiserror::Error;

use crate::{
    controller::{ControllerCommands, send_cntrl_cmd_now},
    prg_config::Authorizations,
    status::{InstrumentStatus, PressureReading},
};

/// Get permission to close the transfer valve based on the VCT state.
///
/// If the VCT dock gate valve is currently open, permission to close the transfer valve is
/// denied as the arm could be in. This check allows raising an appropriate error message
/// to the user.
fn get_close_transfer_valve_permission(inst_status: Arc<Mutex<InstrumentStatus>>) -> Result<()> {
    if inst_status.lock().expect("Poisoned").is_vct_gate_open() {
        Err(anyhow!("Unsafe to close transfer valve: VCT is connected."))
    } else {
        Ok(())
    }
}

/// Get permission to open a valve based on the pressure gauges.
///
/// We do the following checks:
/// - If any of the gauges show 0 mbar (error state), we do not give permission.
/// - If the ratio of the pressures is within a specified safe range, we give permission.
/// - If both pressures are below a specified low pressure limit, regardless of the ratio, we give
///   permission (as we are in a safe low pressure regime).
fn get_open_valve_permission(
    inst_status: Arc<Mutex<InstrumentStatus>>,
    auths: &Authorizations,
) -> Result<(), OpenValvePermissionError> {
    let (p_chamber, p_transfer) = inst_status.lock().expect("Poisoned").get_pressures();
    let auth = &auths.open_valve;

    let p_chamber_mbar = match p_chamber {
        PressureReading::Value(val) => val.as_millibars(),
        _ => return Err(OpenValvePermissionError::GaugeError),
    };
    let p_transfer_mbar = match p_transfer {
        PressureReading::Value(val) => val.as_millibars(),
        _ => return Err(OpenValvePermissionError::GaugeError),
    };

    let p_ratio = p_chamber_mbar / p_transfer_mbar;

    #[allow(clippy::if_same_then_else)] // We want to be explicit about both conditions for
    // readability and comparison with the workflow
    if p_ratio > auth.valve_ratio_range.lower_limit && p_ratio < auth.valve_ratio_range.upper_limit
    {
        Ok(())
    } else if p_chamber_mbar < auth.low_pressure_limit_mbar
        && p_transfer_mbar < auth.low_pressure_limit_mbar
    {
        Ok(())
    } else if p_ratio <= auth.valve_ratio_range.lower_limit {
        Err(OpenValvePermissionError::ChamberPressureTooLow)
    } else if p_ratio >= auth.valve_ratio_range.upper_limit {
        Err(OpenValvePermissionError::ChamberPressureTooHigh)
    } else {
        unreachable!(
            "This branch should be unreachable, unless there is a logic error in above comparisons. Please report!"
        );
    }
}

/// Error possibilities when asking for permission to open valves.
///
/// These errors are useful and can be used when trying to pump/vent the chamber to have the correct
/// check already there.
#[derive(Error, Debug)]
pub enum OpenValvePermissionError {
    #[error("Cannot open valve. One or more gauges are unreachable")]
    GaugeError,
    #[error(
        "Cannot open valve. Pressure in chamber is much smaller than pressure in transfer system."
    )]
    ChamberPressureTooLow,
    #[error(
        "Cannot open valve. Pressure in chamber is much larger than pressure in transfer system."
    )]
    ChamberPressureTooHigh,
}

/// Function to set the transfer valve.
pub fn set_transfer_valve(
    inst_status: Arc<Mutex<InstrumentStatus>>,
    auths: &Authorizations,
    valve_state: ValveState,
) -> Result<()> {
    match valve_state {
        ValveState::Open => get_open_valve_permission(inst_status, auths)?,
        ValveState::Closed => get_close_transfer_valve_permission(inst_status)?,
        ValveState::Undefined => unreachable!("Can only open or close value in workflows."),
    }
    send_cntrl_cmd_now(ControllerCommands::TransferValve(valve_state));
    Ok(())
}

/// Function to set the pump valve.
pub fn set_pump_valve(
    inst_status: Arc<Mutex<InstrumentStatus>>,
    auths: &Authorizations,
    valve_state: ValveState,
) -> Result<(), OpenValvePermissionError> {
    if valve_state == ValveState::Open {
        get_open_valve_permission(inst_status, auths)?;
    }
    send_cntrl_cmd_now(ControllerCommands::PumpValve(valve_state));
    Ok(())
}
