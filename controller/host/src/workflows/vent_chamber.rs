//! A workflow for venting the chamber.

use agilent_4uhv::HvState;
use anyhow::{Result, anyhow};

use slint::Weak;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot, watch};

use crate::{
    app::{AppWindow, ValveOrPumpState},
    instruments::{
        InstrumentCommands,
        hi_cube::{HiCubeCommands, send_hicube_cmd_w_err},
        send_instr_cmd_w_err,
    },
    prg_config::Authorizations,
    status::InstrumentStatus,
    workflows::{
        equalize_pressure::{eq_pch_too_high, eq_pch_too_low},
        progress_windows::{
            ProgressWindowCommands, close_prg_window, disp_error_and_close, n2_notification,
            new_progress_window, prg_cmd, prg_msg,
        },
        timer::Timer,
        valves::{OpenValvePermissionError, set_pump_valve},
    },
};

/// Run the vent chamber workflow.
///
/// While this is running, the UI is blocked from any further action.
/// TODO: Decide if this is also true for waiting for vent valve to close...
///
/// Once the workflow has terminated, no result is returned. If an error occured, This will be
/// displayed in the progress window and the user is expected to hit "Done" after reading the final
/// problem message and then this workflow is over.
pub async fn vent_chamber_workflow(
    inst_status: Arc<Mutex<InstrumentStatus>>,
    auths: &Authorizations,
    ui: Weak<AppWindow>,
) {
    let desc =
        "Vent the chamber and transfer system. This process takes between 25 and 45 minutes.";

    // Channels for sending commands to deal with the displayed progress window
    let (tx_window_cmd, rx_window_cmd) = mpsc::channel(50);

    // Temporary oneshot channel get a weak handle to the progress window.
    let (tx_get_prg_window, rx_get_prg_window) = oneshot::channel();
    // One shot channel to be sent when the user presses a button.
    let (tx_button_press, mut rx_button_press) = watch::channel(());
    rx_button_press.mark_unchanged();

    // Let's get the progress window going
    ui.upgrade_in_event_loop(move |_| {
        new_progress_window(
            rx_window_cmd,
            "Vacuum chamber venting",
            desc,
            tx_get_prg_window,
            tx_button_press,
        )
        .unwrap();
    })
    .expect("UI must be alive");

    // Now get the progress window to work with later.
    let pw = rx_get_prg_window.await.expect("This must work");

    // Start of the workflow
    //
    // Make user check for nitrogen in the balloon.
    n2_notification(&tx_window_cmd, &mut rx_button_press).await;

    // Check that cryocooler is off
    prg_cmd(
        &tx_window_cmd,
        ProgressWindowCommands::SetButtonText("Cancel".into()),
    )
    .await;
    prg_cmd(&tx_window_cmd, ProgressWindowCommands::EnableButton(false)).await;
    if !inst_status.lock().expect("Poisoned").is_cryo_cooler_off() {
        disp_error_and_close(
            &tx_window_cmd,
            "Cannot vent the chamber while the cryo cooler is on.",
            rx_button_press.clone(),
        )
        .await;
        close_prg_window(ui, pw);
        return;
    }

    // Check that sample temperature is higher than the set limit.
    if inst_status
        .lock()
        .expect("Poisoned")
        .get_sample_temperature()
        <= auths.vent_chamber.sample_t_min
    {
        disp_error_and_close(
            &tx_window_cmd,
            &format!(
                "Cannot vent because the sample is too cold. The sample must be warmer than {}.",
                auths.vent_chamber.sample_t_min
            ),
            rx_button_press.clone(),
        )
        .await;
        close_prg_window(ui, pw);
        return;
    }

    // If the pump valve is not open, try to open it!
    if !inst_status.lock().expect("Poisoned").is_pump_valve_open()
        && let Err(open_valve_err) =
            set_pump_valve(Arc::clone(&inst_status), auths, icd::ValveState::Open)
    {
        let eq_result = match open_valve_err {
            OpenValvePermissionError::GaugeError => Err(anyhow!(
                "Authorization for opening the pump valve failed. Make sure everything is connected, running, and try again."
            )),
            OpenValvePermissionError::ChamberPressureTooLow => {
                eq_pch_too_low(&tx_window_cmd, Arc::clone(&inst_status), auths).await
            }
            OpenValvePermissionError::ChamberPressureTooHigh => {
                eq_pch_too_high(
                    &tx_window_cmd,
                    Arc::clone(&inst_status),
                    auths,
                    &mut rx_button_press,
                    false,
                )
                .await
            }
        };

        // pressure can't be equalized, we error out of here.
        if let Err(eq_err) = eq_result {
            disp_error_and_close(
                &tx_window_cmd,
                &format!("{eq_err}"),
                rx_button_press.clone(),
            )
            .await;
            close_prg_window(ui, pw);
            return;
        }
    }

    // vent the chamber
    if let Err(e) = vent_chamber(&tx_window_cmd, auths, rx_button_press.clone()).await {
        let msg = format!("Could not vent chamber: {}", e);
        disp_error_and_close(&tx_window_cmd, &msg, rx_button_press.clone()).await;
        close_prg_window(ui, pw);
        return;
    };

    // Success, wait for user to press done button, then close
    prg_msg(&tx_window_cmd, "Chamber vented successfully.").await;
    tx_window_cmd
        .send(ProgressWindowCommands::SetButtonText("Done".into()))
        .await
        .unwrap();
    tx_window_cmd
        .send(ProgressWindowCommands::EnableButton(true))
        .await
        .unwrap();

    rx_button_press.mark_unchanged();
    let _ = rx_button_press.changed().await;
    close_prg_window(ui, pw);
}

async fn vent_chamber(
    tx_win: &mpsc::Sender<ProgressWindowCommands>,
    auths: &Authorizations,
    rx_button_press: watch::Receiver<()>,
) -> Result<()> {
    stop_pumps_open_vv().await?;

    // vent valve timer, after which we close the valve.
    prg_cmd(tx_win, ProgressWindowCommands::EnableButton(true)).await;

    // Set the message of what we are doing now.
    prg_msg(
        tx_win,
        "Vent valve is open. Waiting for timer to close and finish venting the chamber. 
        If you hit cancel, the vent valve will immediately be closed and 
        the venting process finished.",
    )
    .await;

    // Escape timer when cancel button is pressed.
    let check = {
        let mut rx = rx_button_press.clone();
        rx.mark_unchanged();
        move || -> bool { rx.has_changed().unwrap_or(false) }
    };

    let tim_duration = auths.vent_chamber.timer_3;
    let mut tim = Timer::new(tim_duration);
    tim.run_and_update(tx_win, check).await?;

    Ok(())
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
