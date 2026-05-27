//! This module handles communication with the controller firmware via poststation.
use std::time::{self, Duration};

use icd::{BakingState, BcInstStatus, LightState, ValveState, VctHandshake};
use poststation_sdk::{PoststationClient, connect};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{OnceCell, mpsc},
    task::JoinHandle,
    time::sleep,
};

use crate::{app::get_ui_wait, get_inst_status_wait, get_prg_conf_wait, log};

mod client;

use client::ControllerClient;

pub static CONTROLLER_COMMAND_SENDER: OnceCell<mpsc::Sender<ControllerCommands>> =
    OnceCell::const_new();

pub enum ControllerCommands {
    InitializeLight(),
    Light(LightState),
    Baking(BakingState),
    TransferValve(ValveState),
    PumpValve(ValveState),
    VctHandshake(VctHandshake),
}

/// Start the controller tasks
///
/// Starts the two controller tasks:
/// - Listen to commands.
/// - Listen to broadcasts and react on them.
///
/// # Arguments
/// - `config`: Configuration for the controller.
/// - `inst_status`: Shared instrument status to update from broadcasts.
/// - `rx_ctrl`: Receiver for controller commands.
///
/// # Returns
/// Tuple of joint handles for the two async tasks, which we will await later in main.
pub async fn start_controller_tasks(
    rx_ctrl: mpsc::Receiver<ControllerCommands>,
) -> (JoinHandle<()>, JoinHandle<()>) {
    let config = get_prg_conf_wait()
        .await
        .lock()
        .expect("Locking config must work")
        .get_controller_config();

    let tic = time::Instant::now();
    let ps_client = loop {
        if let Ok(conn) = connect(&config.address).await {
            break Some(conn);
        };

        // if we fail to connect, wait for a while to try again until timeout reached
        if tic.elapsed() > Duration::from_secs(5) {
            break None;
        } else {
            sleep(Duration::from_millis(250)).await;
        };
    };

    let ps_client = match ps_client {
        Some(client) => client,
        None => {
            log::err!("Failed to connect to poststation at '{}'. Is poststation running? Restart the program.", { config.address }).await;
            return (tokio::spawn(async {}), tokio::spawn(async {}));
        }
    };

    let serial = if let Some(device) = ps_client
        .get_devices()
        .await
        .expect("Poststation must return list of devices")
        .iter()
        .find(|d| d.product == Some(config.product_name.clone()) && d.is_connected)
    {
        device.serial
    } else {
        log::err!("No connected device with product name '{}' found.", {
            config.product_name
        })
        .await;
        0
    };

    // Get a new controller client.
    let cntrl_client = ControllerClient::new(ps_client.clone(), serial);

    // Spawn the controller command handling task.
    let cntrl_task = tokio::spawn(controller_task(cntrl_client, rx_ctrl));

    // Broadcast listener task.
    let cntrl_bc_task = tokio::spawn(controller_broadcast_listener(ps_client, serial));

    (cntrl_task, cntrl_bc_task)
}

/// Task that communicates with the controller firmware via poststation.
pub async fn controller_task(cntrl: ControllerClient, mut rx: mpsc::Receiver<ControllerCommands>) {
    let mut rx_shutdown = crate::HALT_SENDER.get().unwrap().subscribe();

    let inst_status = get_inst_status_wait().await;

    loop {
        tokio::select! {
            Some(cmd) = rx.recv() => {
                match cmd {
                    ControllerCommands::InitializeLight() => {
                        match cntrl.get_light().await {
                            Ok(st) => {
                                if inst_status.lock().expect("Poisoned").set_chamber_light_and_ui(st).is_err() {
                                    log::err_now!("Failed to initialize chamber light state on GUI.");
                                }
                            }
                            Err(e) => {
                                log::err_now!("Failed to get chamber light state from controller: {}", e);
                            }
                        };
                    }
                    ControllerCommands::Light(state) => {
                        inst_status.lock().expect("Poisoned").set_chamber_light(state.clone());
                        cntrl.set_light(state).await;
                    }
                    ControllerCommands::Baking(state) => {
                        cntrl.baking(state).await;
                    }
                    ControllerCommands::PumpValve(state) => {
                        inst_status.lock().expect("Poisoned").set_valve_pump_call(state.clone());
                        cntrl.pump_valve(state).await;
                    }
                    ControllerCommands::TransferValve(state) => {
                        inst_status.lock().expect("Poisoned").set_valve_transfer_call(state.clone());
                        cntrl.transfer_valve(state).await;
                    }
                    ControllerCommands::VctHandshake(handshake) => {
                        cntrl.vct_handshake(handshake).await;
                    }
                }
            }
            _ = rx_shutdown.recv() => {
                break;
            }
        }
    }
}

/// Task that listens to broadcast messages from the controller firmware via poststation.
///
/// This task also updates the UI accordingly.
pub async fn controller_broadcast_listener(client: PoststationClient, serial: u64) {
    let mut rx_shutdown = crate::HALT_SENDER.get().unwrap().subscribe();

    let inst_status = get_inst_status_wait().await;

    let mut run_initialize = true; // If true, runs initialize after broadcast

    // Wait for UI to be globally available.
    let _ = get_ui_wait().await;

    // Tell the light to initialize itslef :)
    send_cntrl_cmd(ControllerCommands::InitializeLight()).await;

    // subscription to broadcast
    let mut sub = match client.stream_topic::<BcInstStatus>(serial).await {
        Ok(s) => s,
        Err(e) => {
            log::err!(
                "Broadcast subscription failed. Restart the program. Error: {}",
                e
            )
            .await;
            return;
        }
    };

    loop {
        tokio::select! {
            msg = sub.recv() => {
                if let Some(status) = msg {
                    inst_status.lock().expect("Poisoned").update_from_controller_broadcast(status);

                    // initialization on first start, only if UI is set
                    if run_initialize && inst_status.lock().expect("Poisoned").initialize_call_states_from_bc().is_ok() {
                            run_initialize = false;
                        };
                } else {
                    log::err!("Poststation broadcast stream closed unexpectedly. Is poststation running? Restart the program.").await;
                    break;
                }
            }
            _ = rx_shutdown.recv() => {
                break;
            }
        }
    }
}

/// Get a clone of the controller command sender.
fn get_cntrl_cmd_sender() -> mpsc::Sender<ControllerCommands> {
    CONTROLLER_COMMAND_SENDER
        .get()
        .expect("Uninitialized")
        .clone()
}

/// Convenience function to await sending a controller command.
///
/// If an error occurs, this error is logged. Otherwise, the program will continue
/// as normal.
pub async fn send_cntrl_cmd(cmd: ControllerCommands) {
    let sender = get_cntrl_cmd_sender();
    if let Err(e) = sender.send(cmd).await {
        log::err_now!("Failed to send controller command: {}", e);
    }
}

/// Convenience function to send a controller command without awaiting.
///
/// If an error occurs, this error is logged. Otherwise, the program will continue
/// as normal.
pub fn send_cntrl_cmd_now(cmd: ControllerCommands) {
    let sender = get_cntrl_cmd_sender();
    if let Err(e) = sender.try_send(cmd) {
        log::err_now!("Failed to send controller command now: {}", e);
    }
}

/// A structure that holds the configuration for the controller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControllerConfig {
    /// The product name that is displayed in poststation.
    pub product_name: String,
    /// Address and port of the poststation serve.
    pub address: String,
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            product_name: String::from("Cryostorage Controller"),
            address: String::from("127.0.0.1:51837"),
        }
    }
}
