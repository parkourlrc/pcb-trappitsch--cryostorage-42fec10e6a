//! Workflow for pumping the chamber.

use anyhow::anyhow;
use icd::ValveState;

use std::sync::Arc;

use crate::{
    get_inst_status_wait,
    instruments::hi_cube::{HiCubeCommands, send_hicube_cmd_w_err},
    prg_config::Authorizations,
    status::{PressureReading, PumpState},
    workflows::{
        equalize_pressure::{eq_pch_too_high, eq_pch_too_low},
        progress_windows::WorkflowProgress,
        valves::{OpenValvePermissionError, set_pump_valve},
    },
};

/// Run the pump chamber workflow.
///
/// While this is running, the UI is hidden by the progress window display.
///
/// Once the workflow has terminated, no result is returned. Errors are directly displayed to the
/// user and must be acted on by the user accordingly. Errors in communicating with the devices,
/// etc., are still logged, but user cancellations, workflow errors are only displayed to the user.
///
/// Arguments:
/// - `inst_status`: Instrument status.
/// - `auths`: Authorizations.
pub async fn pump_chamber_workflow(auths: &Authorizations) {
    let inst_status = get_inst_status_wait().await;

    let desc = "Pumping the chamber and transfer system. This process can take up to around 3h.";
    let mut wp = WorkflowProgress::new("Venting the chamber", desc).await;

    // Make sure user didn't press the button accidentally.
    if wp
        .notify(
            "Press next to continue pumping the workflow or cancel to abort.",
            false,
        )
        .await
        .is_err()
    {
        wp.close();
        return;
    }

    // If the pump valve is not open, try to open it!
    if !inst_status.lock().expect("Poisoned").is_pump_valve_open()
        && let Err(open_valve_err) =
            set_pump_valve(Arc::clone(&inst_status), auths, icd::ValveState::Open)
    {
        let eq_result = match open_valve_err {
            OpenValvePermissionError::GaugeError => Err(anyhow!(
                "Authorization for opening the pump valve failed. \
                Make sure everything is connected, running, and try again."
            )),
            OpenValvePermissionError::ChamberPressureTooLow => eq_pch_too_low(&mut wp, auths).await,
            OpenValvePermissionError::ChamberPressureTooHigh => {
                let mut res = eq_pch_too_high(&mut wp, auths, false).await;
                // close vent valve if we are in p_ch >> p_tr and the outcome of the equalizing
                // workflow was ok
                if res.is_ok()
                    && send_hicube_cmd_w_err(HiCubeCommands::SetVentingValveState(ValveState::Open))
                        .await
                        .is_err()
                {
                    res = Err(anyhow!(
                        "Pressure in transfer and sample chamber was equalized but the vent valve could not be closed. \
                            Thus, the pumping workflow cannot be continued."
                    ))
                };
                res
            }
        };

        // pressure can't be equalized, we error out of here.
        if let Err(eq_err) = eq_result {
            wp.finalize(&format!("{eq_err}"), true).await;
            return;
        }

        // start primary pump.
        if send_hicube_cmd_w_err(HiCubeCommands::SetPumpStandState(PumpState::On))
            .await
            .is_err()
        {
            wp.finalize("Could not start primary pump.", true).await;
            return;
        };

        // Wait for pre-defined time to reach a pressure limit for continuing.
        let ppp_limit = auths.pump_chamber.pressure_limit_primary_pump;
        let dur = auths.pump_chamber.timer_primary_pump;
        let msg = format!(
            "Waiting for chamber pressure to drop below {:e} mbar.",
            ppp_limit.as_millibars()
        );
        let check = {
            let inst_status = Arc::clone(&inst_status);
            move || -> bool {
                let (p_ch, _) = inst_status.lock().expect("Poisoned").get_pressures();
                if let PressureReading::Value(p) = p_ch {
                    p < ppp_limit
                } else {
                    false
                }
            }
        };

        wp.show_vacuum_plot(true);
        if wp.check_for(dur, &msg, check).await.is_err() {
            wp.show_vacuum_plot(false);
            let mut msg = format!(
                "Did not reach a pressure of {:e} mbar in the alotted time.",
                ppp_limit.as_millibars()
            );

            // turn pump stand off.
            if send_hicube_cmd_w_err(HiCubeCommands::SetPumpStandState(PumpState::Off))
                .await
                .is_err()
            {
                msg.push_str(" Could not turn primary pump off.");
            } else {
                msg.push_str(" Turned primary pump off.")
            }

            wp.finalize(&msg, true).await;
            return;
        }

        // Now wait for a pre-defined time to turn on ion pump.
        let dur = auths.pump_chamber.timer_ion_pump;
        let msg = "Waiting until the timer expires to turn on the ion pump. \
            If you press cancel before, the ion pump will not be turned on.";

        if wp.wait_for(dur, msg).await.is_err() {
            wp.finalize(
                "Waiting for ion pump turn on was cancelled. The ion pump was not turned on.",
                true,
            )
            .await;
        } else {
            wp.finalize("Pumping the chamber successfully completed.", false)
                .await;
        }
        wp.show_vacuum_plot(false);
    }
}
