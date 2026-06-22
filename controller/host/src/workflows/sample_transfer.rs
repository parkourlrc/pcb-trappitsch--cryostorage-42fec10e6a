//! Module for the sample transfer workflow.

use std::sync::Arc;

use icd::{ValveState, VctHandshake};

use crate::{
    controller::{ControllerCommands, send_cntrl_cmd},
    get_inst_status_wait, get_prg_conf_wait,
    prg_config::Authorizations,
    safety::check_sample_safe,
    status::PressureReading,
    workflows::progress_windows::WorkflowProgress,
};

/// Run the sample transfer workflow.
///
/// WHile this is running, the UI is hidden by the progress window display.
///
/// Once the workflow has terminated, no result is returned. Errors are directly displayed to the
/// user and must be acted on by the user accordingly. Errors in communicating with the devices,
/// etc., are still logged, but user cancellations, workflow errors are only displaye dto the user.
pub async fn sample_transfer_workflow(auths: &Authorizations) {
    let inst_status = get_inst_status_wait().await;

    let desc = "Transfer a sample to/from the cryostorage chamber.";
    let mut wp = WorkflowProgress::new("Sample transfer", desc).await;

    // Make sure user did not press the button accidentally.
    if wp
        .notify(
            "Press next to continue with the sample transfer or cancel to abort.",
            false,
        )
        .await
        .is_err()
    {
        wp.close();
        return;
    }

    // Check if the cryocooler is on.
    if inst_status
        .lock()
        .expect("InstrumentStatus lock poisoned")
        .is_cryo_cooler_off()
    {
        wp.finalize(
            "Cryocooler must be on and at temperature before a sample transfer can be started.",
            true,
        )
        .await;
        return;
    }

    // Run sample safety check
    let prg_conf = get_prg_conf_wait().await;
    let (min_safe_temp, max_safe_temp) = prg_conf
        .lock()
        .expect("PrgConfig lock poisoned")
        .get_safe_sample_temperatures();
    let res = {
        let inst_stat = inst_status.lock().expect("InstrumentStatus lock poisoned");
        check_sample_safe(&inst_stat, (&min_safe_temp, &max_safe_temp))
    };
    if let Err(msg) = res {
        wp.finalize(&msg, true).await;
        return;
    }

    // Set VCT ready to attach status.
    send_cntrl_cmd(ControllerCommands::VctHandshake(VctHandshake::Ready)).await;

    // Ask user to connect VCT and hit attach.
    if wp.notify("Connect the VCT and click the \"Attach\" button on the VCT controller. When done, hit next.", false).await.is_err() {
        // Set VCT not ready to attach status.
        send_cntrl_cmd(ControllerCommands::VctHandshake(VctHandshake::NotReady)).await;
        wp.finalize("Sample transfer cancelled. All is back to normal.", false).await;
        return;
    }

    let dur = auths.sample_transfer.timer;
    let msg = format!(
        "Waiting for pressure in transfer chamber to fall below {:.1E} mbar and for VCT valves to open. \
        If you press cancel, the transfer valve will not open and you will have to manually unset the \"VCT ready\" status.",
        auths.sample_transfer.pressure_max.as_millibars()
    );
    let check = {
        let inst_status = Arc::clone(&inst_status);
        let p_max = auths.sample_transfer.pressure_max;
        move || -> bool {
            let guard = inst_status.lock().expect("InstrumentStatus lock poisoned");
            let is_attached = guard.is_vct_gate_open();
            if let (_p_ch, PressureReading::Value(p_tr)) = guard.get_pressures() {
                (p_tr < p_max) && is_attached
            } else {
                false
            }
        }
    };

    wp.show_vacuum_plot(true);
    if wp.check_for(dur, &msg, check).await.is_err() {
        wp.show_vacuum_plot(false);
        send_cntrl_cmd(ControllerCommands::VctHandshake(VctHandshake::NotReady)).await;
        wp.finalize("Failed to attach and pump the VCT.", true)
            .await;
        return;
    }
    wp.show_vacuum_plot(false);

    // Open the transfer valve.
    send_cntrl_cmd(ControllerCommands::TransferValve(ValveState::Open)).await;

    // Check for pressure and VCT valve opening for a given time.
    let msg = "Ready for sample transfer. Please transfer the sample, retract the arm, \
        and then hit \"Detach\" on the VCT controller. \
        Once the VCT is detached, the transfer valve will automatically close. \
        If you press cancel, the transfer valve will stay open.";
    let check = {
        let inst_status = Arc::clone(&inst_status);
        move || -> bool {
            !inst_status
                .lock()
                .expect("InstrumentStatus lock poisoned")
                .is_vct_gate_open()
        }
    };

    if wp.check_for_infinite_time(msg, check).await.is_err() {
        wp.finalize(
            "Could not detach the VCT from the cryostorage chamber. The transfer valve stays open.",
            true,
        )
        .await;
        send_cntrl_cmd(ControllerCommands::VctHandshake(VctHandshake::NotReady)).await;
        return;
    }

    send_cntrl_cmd(ControllerCommands::TransferValve(ValveState::Closed)).await;
    send_cntrl_cmd(ControllerCommands::VctHandshake(VctHandshake::NotReady)).await;

    wp.finalize("Sample transfer successfully completed. Don't forget to log the new sample in the program and in the lab book!", false).await;
}
