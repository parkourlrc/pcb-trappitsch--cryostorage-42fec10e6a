//! Module to handle workflows and system safety checks before executing commands.
//!
//! On commands from the UI, the functions in this module will be called to check if the command
//! can be executed safely and, if it can, will execute the command. Otherwise, error dialogs will
//! be shown in the UI.
//!
//! The module itself handles receiving workflow commands and running them in a blocking or async
//! way, depending on the function.
//!
//! The submodules handle the actual workflows for different components and execute the commands by
//! sending using the adequate command senders.
//! Blocking tasks will either run the command and return `Ok(())`, or return an error with a
//! message to be shown to the user if they fail.
//! TODO: Async task description
//! - Block the UI?
//! - How do we prevent long running background tasks from being started multiple times?

use std::sync::Arc;

use icd::{BakingState, ValveState};
use slint::ComponentHandle;
use sunpower_cryotelgt::CoolerState;
use tokio::sync::{OnceCell, mpsc};

use crate::{
    app::{Logic, get_ui_wait},
    dialog::error_dialog_helper,
    get_inst_status_wait, get_prg_conf_wait, log,
    workflows::{
        baking::set_baking,
        cooler::set_cooler,
        pump_chamber::pump_chamber_workflow,
        sample_transfer::sample_transfer_workflow,
        valves::{set_pump_valve, set_transfer_valve},
        vent_chamber::vent_chamber_workflow,
    },
};

mod baking;
mod cooler;
mod equalize_pressure;
mod progress_windows;
mod pump_chamber;
mod sample_transfer;
mod timer;
mod valves;
mod vent_chamber;

pub use progress_windows::PROGRESS_WINDOW;

/// Some static strings to use for consistent messaging.
static MSG_FILL_N2: &str = "Ensure that there is nitrogen in the balloon and press next.";

/// Sender for the controller commands.
pub static WORKFLOW_COMMAND_SENDER: OnceCell<mpsc::Sender<WorkflowCommands>> =
    OnceCell::const_new();

/// Commands for workflows.
///
/// These are the commands that the UI will send to activate the various workflows.
pub enum WorkflowCommands {
    /// Baking on/off.
    Baking(BakingState),
    /// Pump the vacuum chamber.
    ChamberPump,
    /// Vent the vacuum chamber.
    ChamberVent,
    /// Cryo cooler on/off.
    CryoCoolerState(CoolerState),
    /// Open/close the pump valve.
    PumpValve(ValveState),
    /// Open/close the transfer valve.
    TransferValve(ValveState),
    /// Initiate a new sample transfer that guides the user through a list of tasks.
    StartSampleTransfer,
}

/// The workflow task: Listen to WorkflowCommands and execute them.
pub async fn workflow_task(mut rx_wf: mpsc::Receiver<WorkflowCommands>) {
    let mut rx_shutdown = crate::HALT_SENDER.get().unwrap().subscribe();

    let ui = get_ui_wait().await;

    let inst_status = get_inst_status_wait().await;

    let conf = get_prg_conf_wait().await;
    let auths = conf.lock().expect("Poisoned").get_authorizations();

    loop {
        tokio::select! {
            Some(cmd) = rx_wf.recv() => {
                match cmd {
                    WorkflowCommands::Baking(state) => {
                        set_baking(Arc::clone(&inst_status), &auths, state)
                            .unwrap_or_else(|e| {
                                error_dialog_helper(e);
                        });
                    }
                    WorkflowCommands::ChamberPump => {
                        pump_chamber_workflow(&auths).await;
                    },
                    WorkflowCommands::ChamberVent => {
                        vent_chamber_workflow(&auths).await;
                    },
                    WorkflowCommands::CryoCoolerState(state) => {
                        set_cooler(Arc::clone(&inst_status), &auths, state).unwrap_or_else(|e| {
                            error_dialog_helper(e);
                        });
                    },
                    WorkflowCommands::PumpValve(state) => {
                        let button_value = state == ValveState::Open;

                        match set_pump_valve(Arc::clone(&inst_status), &auths, state) {
                            Ok(()) => {
                                ui.upgrade_in_event_loop(move |ui| {
                                    ui.global::<Logic>().set_pump_valve_is_open(button_value);
                                }).expect("UI must be alive");
                            },
                            Err(e) => {
                                error_dialog_helper(e.into())
                            }
                        };

                    }
                    WorkflowCommands::TransferValve(state) => {
                        let button_value = state == ValveState::Open;

                        match set_transfer_valve(Arc::clone(&inst_status), &auths, state) {
                            Ok(()) => {
                                ui.upgrade_in_event_loop(move |ui| {
                                    ui.global::<Logic>().set_transfer_valve_is_open(button_value);
                                }).expect("UI must be alive");
                            },
                            Err(e) => {
                                error_dialog_helper(e)
                            }
                        };
                    },
                    WorkflowCommands::StartSampleTransfer => {
                        sample_transfer_workflow(&auths).await;
                    }
                }
            }
            _ = rx_shutdown.recv() => {
                break;
            }
        }
    }
}

/// Get a cone of the workflow command sender.
fn get_workflow_command_sender() -> mpsc::Sender<WorkflowCommands> {
    WORKFLOW_COMMAND_SENDER.get().expect("Unitialized").clone()
}

/// Convenience function to send a workflow command now.
///
/// An error here means that something aside from the permissions checks has gone wrong. This will
/// be logged as any other error.
pub fn send_workflow_command_now(cmd: WorkflowCommands) {
    let sender = get_workflow_command_sender();
    if let Err(e) = sender.try_send(cmd) {
        log::err_now!("Failed to send workflow command now: {}", e);
    }
}
