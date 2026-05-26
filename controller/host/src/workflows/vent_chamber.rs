//! A workflow for venting the chamber.

use agilent_4uhv::HvState;
use anyhow::{Result, anyhow};

use std::sync::{Arc, Mutex};

use crate::{
    app::ValveOrPumpState,
    instruments::{
        InstrumentCommands,
        hi_cube::{HiCubeCommands, send_hicube_cmd_w_err},
        send_instr_cmd_w_err,
    },
    prg_config::Authorizations,
    status::InstrumentStatus,
    workflows::{
        MSG_FILL_N2,
        equalize_pressure::{eq_pch_too_high, eq_pch_too_low},
        progress_windows::WorkflowProgress,
        valves::{OpenValvePermissionError, set_pump_valve},
    },
};

/// Run the vent chamber workflow.
///
/// While this is running, the UI is hidden by the progress window display.
///
/// Once the workflow has terminated, no result is returned. If an error occured, This will be
/// displayed in the progress window and the user is expected to hit "Done" after reading the final
/// problem message and then this workflow is over.
///
/// Arguments:
/// - `inst_status`: Instrument status.
/// - `auths`: Authorizations.
/// - `ui`: A weak handle to the main user interface.
pub async fn vent_chamber_workflow(
    inst_status: Arc<Mutex<InstrumentStatus>>,
    auths: &Authorizations,
) {
    // THE DEMO
    // let desc =
    //     "Vent the chamber and transfer system. This process takes between 25 and 45 minutes.";
    // let mut wp = WorkflowProgress::new("Venting the chamber", desc, ui.clone()).await;
    //
    // if wp.notify("This is a notification.").await.is_err() {
    //     wp.finalize("Screw you", false).await;
    //     return;
    // };
    //
    // // show the vacuum plot till the timer expires or the user presses cancel.
    // wp.show_vacuum_plot(true);
    // let _ = wp
    //     .wait_for(
    //         Duration::from_secs(13),
    //         "Here's the vacuum plot for the duration of the timer or till you hit cancel.",
    //     )
    //     .await;
    // wp.show_vacuum_plot(false);
    //
    // // start a timer of 10 seconds.
    // if wp
    //     .wait_for(
    //         Duration::from_secs(10),
    //         "Please be a nice person and wait for the timer to finish.",
    //     )
    //     .await
    //     .is_ok()
    // {
    //     wp.finalize("Good boy, have a cookie", false).await;
    //     return;
    // };
    //
    // // somebody pressed cancel, let's give them a really long timer that expires after 5 sec with a
    // // message.
    // let check = {
    //     let now = std::time::Instant::now();
    //     move || std::time::Instant::now() - now > Duration::from_secs(5)
    // };
    //
    // if wp
    //     .check_for(
    //         Duration::from_hours(200),
    //         "Bad boy, no chocolate for you!",
    //         check,
    //     )
    //     .await
    //     .is_ok()
    // {
    //     wp.finalize("Haha, got you. Next time be better", false)
    //         .await;
    // } else {
    //     wp.finalize("WTF, again?????", true).await;
    // };
    // END DEMO

    let desc =
        "Vent the chamber and transfer system. This process takes between 25 and 45 minutes.";
    let mut wp = WorkflowProgress::new("Venting the chamber", desc).await;

    // N2 fill notification.
    if wp.notify(MSG_FILL_N2, false).await.is_err() {
        wp.close();
        return;
    };

    // Check that cryocooler is off
    if !inst_status.lock().expect("Poisoned").is_cryo_cooler_off() {
        wp.finalize("Cannot vent the chamber while the cryo cooler is on.", true)
            .await;
        return;
    }

    // Check that sample temperature is higher than the set limit.
    if inst_status
        .lock()
        .expect("Poisoned")
        .get_sample_temperature()
        <= auths.vent_chamber.sample_t_min
    {
        wp.finalize(
            &format!(
                "Cannot vent because the sample is too cold. The sample must be warmer than {}.",
                auths.vent_chamber.sample_t_min
            ),
            true,
        )
        .await;
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
            OpenValvePermissionError::ChamberPressureTooLow => {
                eq_pch_too_low(&mut wp, Arc::clone(&inst_status), auths).await
            }
            OpenValvePermissionError::ChamberPressureTooHigh => {
                eq_pch_too_high(&mut wp, Arc::clone(&inst_status), auths, false).await
            }
        };

        // pressure can't be equalized, we error out of here.
        if let Err(eq_err) = eq_result {
            wp.finalize(&format!("{eq_err}"), true).await;
            return;
        }
    }

    // vent the chamber.
    if let Err(e) = vent_chamber(&mut wp, auths).await {
        let msg = format!("Venting not finished: {}", e);
        wp.finalize(&msg, true).await;
        return;
    };

    // Success, wait for user to press done button, then close
    wp.finalize("Chamber vented successfully.", false).await;
}

async fn vent_chamber(wp: &mut WorkflowProgress, auths: &Authorizations) -> Result<()> {
    stop_pumps_open_vv().await?;

    let msg = "Vent valve is open. \
        Waiting for timer to close and finish venting the chamber. \
        If you hit cancel, the vent valve will immediately be closed \
        and the venting process finished.";
    let tim_duration = auths.vent_chamber.timer_3;

    wp.show_vacuum_plot(true);

    // Escape timer when cancel button is pressed.
    let res = match wp.wait_for(tim_duration, msg).await {
        Ok(_) => Ok(()),
        Err(_) => Err(anyhow!(
            "Cancel was pressed during final vent. Please close the vent valve manually."
        )),
    };

    wp.show_vacuum_plot(false);

    res
}

/// Stop both pumps and open the vent valve
///
/// This part is broken out since it is also used to equalize the pressure.
pub async fn stop_pumps_open_vv() -> Result<()> {
    // Stop the ion pump
    send_instr_cmd_w_err(InstrumentCommands::IonPumpState(HvState::Off)).await?;
    // Stop the primary pump
    send_hicube_cmd_w_err(HiCubeCommands::SetPumpStandState(
        ValveOrPumpState::ClosedOrOff,
    ))
    .await?;
    // Open vent valve
    send_hicube_cmd_w_err(HiCubeCommands::SetVentingValveState(
        ValveOrPumpState::OpenOrOn,
    ))
    .await?;

    Ok(())
}
