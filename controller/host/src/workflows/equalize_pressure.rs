//! Contains shared workflows for pumping and venting the chamber.
//!
//! All of these shared workflows come into play when opening valves is not authorized. This means
//! the pressure is to uneven between the two sides and must first be equalized.

use std::sync::{Arc, Mutex};

use anyhow::{Result, bail};
use icd::ValveState;
use tokio::sync::{mpsc, watch};

use crate::{
    app::ValveOrPumpState,
    controller::{ControllerCommands, send_cntrl_cmd},
    instruments::hi_cube::{HiCubeCommands, send_hicube_cmd_w_err},
    prg_config::Authorizations,
    status::{InstrumentStatus, PressureReading},
    workflows::{
        progress_windows::{ProgressWindowCommands, n2_notification},
        timer::Timer,
        vent_chamber::stop_pumps_open_vv,
    },
};

/// Equalize the pressure if p_chamber >> p_transfer.
///
/// This means that the transfer system is first vented until we can open the pump valve.
///
/// Arguments:
/// * tx_win: Sender that allows to update the ProgressWindow.
/// * auths: Authorizations for workflows. Contains the important limits.
/// * rx_button_press: A watch channel that can be queried to see if the button in the Progress
///   Window was pressed.
/// * n2_note: If `true`, a notification will be displayed to fill the N2 balloon. This is required
///   if this workflow is called during the pump down workflow, as the pump down workflow does not
///   automatically display such a notification in the beginning.
///
/// Returns:
/// If any of these commands fail, an error is returned. If an `Ok(())` is returned, all has
/// succeeded (including timers) and we will continue to work.
pub async fn eq_pch_too_high(
    tx_win: &mpsc::Sender<ProgressWindowCommands>,
    inst_status: Arc<Mutex<InstrumentStatus>>,
    auths: &Authorizations,
    rx_button_press: &mut watch::Receiver<()>,
    n2_note: bool,
) -> Result<()> {
    // Display fill N2 message if requested.
    if n2_note {
        n2_notification(tx_win, rx_button_press).await;
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
    let mut tim = Timer::new(tim_duration);

    if let Err(e) = tim.run_and_update(tx_win, check).await {
        let _ = send_hicube_cmd_w_err(HiCubeCommands::SetVentingValveState(
            ValveOrPumpState::ClosedOrOff,
        ))
        .await;
        bail!(format!("Could not vent chamber: {}", e));
    }

    send_cntrl_cmd(ControllerCommands::PumpValve(ValveState::Open)).await;
    Ok(())
}

/// Equalize the pressure if p_chamber << p_transfer.
///
/// This means that the transfer system must first be pumped before we can open the pump valve and
/// continue with whatever workflow we are in.
///
/// Arguments:
/// * tx_win: Sender that allows to update the ProgressWindow.
/// * auths: Authorizations for workflows. Contains the important limits.
///
/// Returns:
/// If any of these commands fail, an error is returned. If an `Ok(())` is returned, all has
/// succeeded (including timers) and we will continue to work.
pub async fn eq_pch_too_low(
    tx_win: &mpsc::Sender<ProgressWindowCommands>,
    inst_status: Arc<Mutex<InstrumentStatus>>,
    auths: &Authorizations,
) -> Result<()> {
    // start the primary pump
    send_hicube_cmd_w_err(HiCubeCommands::SetPumpStandState(
        ValveOrPumpState::OpenOrOn,
    ))
    .await?;

    // set up the with the escape check and the time.
    let check = {
        let cmp_ratio_ll = auths.open_valve.valve_ratio_range.lower_limit;
        let cmp_upper_lim = auths.open_valve.low_pressure_limit_mbar;
        move || -> bool {
            let (p_chamber, p_transfer) = inst_status.lock().expect("Poisoned").get_pressures();
            if let PressureReading::Value(pch) = p_chamber
                && let PressureReading::Value(ptr) = p_transfer
            {
                let pch = pch.as_millibars();
                let ptr = ptr.as_millibars();
                (pch / ptr > cmp_ratio_ll) || (pch < cmp_upper_lim && ptr < cmp_upper_lim)
            } else {
                false
            }
        }
    };
    let tim_duration = auths.equalize_pressures.timer;
    let mut tim = Timer::new(tim_duration);

    if let Err(e) = tim.run_and_update(tx_win, check).await {
        let _ = send_hicube_cmd_w_err(HiCubeCommands::SetPumpStandState(
            ValveOrPumpState::ClosedOrOff,
        ))
        .await;
        bail!(format!("Could not vent chamber: {}", e));
    }

    send_cntrl_cmd(ControllerCommands::PumpValve(ValveState::Open)).await;
    Ok(())
}
