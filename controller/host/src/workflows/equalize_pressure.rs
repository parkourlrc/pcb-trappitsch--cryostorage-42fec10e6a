//! Contains shared workflows for pumping and venting the chamber.
//!
//! All of these shared workflows come into play when opening valves is not authorized. This means
//! the pressure is to uneven between the two sides and must first be equalized.

use anyhow::{Result, anyhow};
use icd::ValveState;

use crate::{
    app::ValveOrPumpState,
    controller::{ControllerCommands, send_cntrl_cmd},
    get_inst_status_wait,
    instruments::hi_cube::{HiCubeCommands, send_hicube_cmd_w_err},
    prg_config::Authorizations,
    status::PressureReading,
    workflows::{
        MSG_FILL_N2, progress_windows::WorkflowProgress, vent_chamber::stop_pumps_open_vv,
    },
};

/// Equalize the pressure if p_chamber >> p_transfer.
///
/// This means that the transfer system is first vented until we can open the pump valve.
///
/// Arguments:
/// * `wp`: A mutable reference to the WorkflowProgress window.
/// * `inst_status`: The current instrument status.
/// * `auths`: Authorizations for workflows. Contains the important limits.
/// * `n2_note`: If `true`, a notification will be displayed to fill the N2 balloon. This is required
///   if this workflow is called during the pump down workflow, as the pump down workflow does not
///   automatically display such a notification in the beginning.
///
/// Returns:
/// If any of these commands fail, an error is returned. If an `Ok(())` is returned, all has
/// succeeded (including timers) and we will continue to work. The error can directly be used to
/// display the message from the main routine.
pub async fn eq_pch_too_high(
    wp: &mut WorkflowProgress,
    auths: &Authorizations,
    n2_note: bool,
) -> Result<()> {
    let inst_status = get_inst_status_wait().await;

    // Display fill N2 message if requested. This can only be Ok(()) as no next button exists.
    if n2_note {
        let _ = wp.notify(MSG_FILL_N2, true).await;
    }

    // stop the pumps and open the vent valve
    stop_pumps_open_vv().await?;

    // set up the with the escape check and the time.
    let check = {
        let cmp_ratio_ul = auths.open_valve.valve_ratio_range.upper_limit;
        move || -> bool {
            let (p_chamber, p_transfer) = inst_status.lock().expect("Poisoned").get_pressures();
            if let PressureReading::Value(pch) = p_chamber
                && let PressureReading::Value(ptr) = p_transfer
            {
                pch.as_millibars() / ptr.as_millibars() < cmp_ratio_ul
            } else {
                false
            }
        }
    };

    let tim_duration = auths.equalize_pressures.timer;
    let msg = "Venting the transfer chamber to equalize pressure before opening the pump valve. \
        Hitting cancel to stop the timer manually early might abort the process.";

    wp.show_vacuum_plot(true);

    let res = match wp.check_for(tim_duration, msg, check).await {
        Ok(_) => {
            send_cntrl_cmd(ControllerCommands::PumpValve(ValveState::Open)).await;
            Ok(())
        }
        Err(_) => {
            let _ = send_hicube_cmd_w_err(HiCubeCommands::SetVentingValveState(
                ValveOrPumpState::ClosedOrOff,
            ))
            .await;
            Err(anyhow!(
                "Could not equalize the pressure within the given time. \
                Closing vent valve and stopping here."
            ))
        }
    };

    wp.show_vacuum_plot(false);

    res
}

/// Equalize the pressure if p_chamber << p_transfer.
///
/// This means that the transfer system must first be pumped before we can open the pump valve and
/// continue with whatever workflow we are in.
///
/// Arguments:
/// * `wp`: A mutable reference to the WorkflowProgress window.
/// * `inst_status`: The current instrument status.
/// * `auths`: Authorizations for workflows. Contains the important limits.
///
/// Returns:
/// If any of these commands fail, an error is returned. If an `Ok(())` is returned, all has
/// succeeded (including timers) and we will continue to work. The error can directly be used to
/// display the message from the main routine.
pub async fn eq_pch_too_low(wp: &mut WorkflowProgress, auths: &Authorizations) -> Result<()> {
    let inst_status = get_inst_status_wait().await;

    // start the primary pump
    send_hicube_cmd_w_err(HiCubeCommands::SetPumpStandState(
        ValveOrPumpState::OpenOrOn,
    ))
    .await?;

    // set up the with the escape check and the time.
    let check = {
        let cmp_ratio_ll = auths.open_valve.valve_ratio_range.lower_limit;
        let cmp_low_pressure_lim = auths.open_valve.low_pressure_limit_mbar;
        move || -> bool {
            let (p_chamber, p_transfer) = inst_status.lock().expect("Poisoned").get_pressures();
            if let PressureReading::Value(pch) = p_chamber
                && let PressureReading::Value(ptr) = p_transfer
            {
                let pch = pch.as_millibars();
                let ptr = ptr.as_millibars();
                (pch / ptr > cmp_ratio_ll)
                    || (pch < cmp_low_pressure_lim && ptr < cmp_low_pressure_lim)
            } else {
                false
            }
        }
    };
    let msg = "Pumping the transfer chamber to equalize pressure before opening the pump valve. \
        Hit cancel to stop the timer.";
    let tim_duration = auths.equalize_pressures.timer;

    wp.show_vacuum_plot(true);

    let res = match wp.check_for(tim_duration, msg, check).await {
        Ok(_) => {
            send_cntrl_cmd(ControllerCommands::PumpValve(ValveState::Open)).await;
            Ok(())
        }
        Err(_) => {
            let _ = send_hicube_cmd_w_err(HiCubeCommands::SetPumpStandState(
                ValveOrPumpState::ClosedOrOff,
            ))
            .await;
            Err(anyhow!(
                "Could not equalize the pressure within the given time. Turning the pump stand off."
            ))
        }
    };

    wp.show_vacuum_plot(false);

    res
}
