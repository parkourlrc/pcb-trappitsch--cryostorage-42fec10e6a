//! Module for the sample transfer workflow.

use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use icd::{ValveState, VctHandshake};

use crate::{
    controller::{ControllerCommands, send_cntrl_cmd},
    get_inst_status_wait,
    prg_config::Authorizations,
    status::PressureReading,
    workflows::{MSG_FILL_N2, progress_windows::WorkflowProgress},
};

static PREAD_COUNTER: AtomicU8 = AtomicU8::new(0);

/// Run the sample transfer workflow.
///
/// While this is running, the UI is hidden by the progress window display.
///
/// Once the workflow has terminated, no result is returned. Errors are directly displayed to the
/// user and must be acted on by the user accordingly. Errors in communicating with the devices,
/// etc., are still logged, but user cancellations, workflow errors are only displayed to the user.
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

    // Display fill N2 message.
    let _ = wp.notify(MSG_FILL_N2, true).await;

    // Set VCT ready to attach status.
    send_cntrl_cmd(ControllerCommands::VctHandshake(VctHandshake::Ready)).await;

    // Close the pump valve.
    send_cntrl_cmd(ControllerCommands::PumpValve(ValveState::Closed)).await;
    send_cntrl_cmd(ControllerCommands::TransferValve(ValveState::Closed)).await;

    // Ask user to connect VCT and hit attach.
    if wp
        .notify(
            "Connect the VCT and click the \"Attach\" button on the VCT controller. Then hit next.",
            false,
        )
        .await
        .is_err()
    {
        // Set VCT not ready to attach status.
        wp.finalize(
            "Sample transfer cancelled. The VCT status was left in \"EM Ready\" mode.",
            true,
        )
        .await;
        return;
    }

    let dur = auths.sample_transfer.timer;
    let msg = format!(
        "Waiting for pressure in transfer chamber to fall below {:.1e} mbar and for VCT valves to open. \
        If you press cancel, the transfer valve will not open and you will have to manually unset the \"VCT ready\" status.",
        auths.sample_transfer.pressure_max_mbar
    );

    PREAD_COUNTER.store(0, Ordering::Relaxed);

    let check = {
        let inst_status = Arc::clone(&inst_status);
        let p_max = auths.sample_transfer.pressure_max_mbar;
        let cnt_pressure_readings = auths.sample_transfer.cnt_pressure_readings;
        move || -> bool {
            let guard = inst_status.lock().expect("InstrumentStatus lock poisoned");
            let is_attached = guard.is_vct_gate_open();
            if let (_p_ch, PressureReading::Value(p_tr)) = guard.get_pressures() {
                // Counter here is ~seconds, it's an empirical value. Might need tuning.
                if (p_tr.as_millibars() < p_max)
                    && is_attached
                    && PREAD_COUNTER.load(Ordering::Relaxed) >= cnt_pressure_readings
                {
                    true
                } else if (p_tr.as_millibars() < p_max) && is_attached {
                    let _cnt = PREAD_COUNTER.fetch_add(1, Ordering::Relaxed);
                    false
                } else {
                    PREAD_COUNTER.store(0, Ordering::Relaxed);
                    false
                }
            } else {
                false
            }
        }
    };

    wp.show_vacuum_plot(true);
    if wp.check_for(dur, &msg, check).await.is_err() {
        wp.show_vacuum_plot(false);
        wp.finalize(
            "Failed to attach and pump the VCT.  The VCT status was left in \"EM Ready\" mode.",
            true,
        )
        .await;
        return;
    }
    wp.show_vacuum_plot(false);

    if wp
        .notify(
            "Click next when you are ready to open the transfer valve \
                and the VCT has opened both valves.\n \
                If you click cancel, the sample transfer is aborted.",
            false,
        )
        .await
        .is_err()
    {
        wp.finalize(
            "Sample transfer cancelled. The VCT status was left in \"EM Ready\" mode.",
            true,
        )
        .await;
        return;
    }

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
            "Could not detach the VCT from the cryostorage chamber. The transfer valve stays open. \
                The VCT status was left in \"EM Ready\" mode.",
            true,
        )
        .await;
        return;
    }

    send_cntrl_cmd(ControllerCommands::TransferValve(ValveState::Closed)).await;
    send_cntrl_cmd(ControllerCommands::VctHandshake(VctHandshake::NotReady)).await;

    wp.finalize("Sample transfer successfully completed. Don't forget to log the new sample in the program and in the lab book!", false).await;
}
