//! Module to handle Pfeiffer HiCube tasks.
//!
//! Note that since this is an async server, this instrument is handled separately from the main
//! instruments module (where comms are all blocking).
//!
//! The main package (`pfeiffer_hicube`) is a separate crate that contains the client and the
//! variables we want to monitor and be able to set. These variables are:
//! - Venting valve (open/closed)
//! - Pump stand state (on/off)

use anyhow::Result;

use pfeiffer_hicube::{HiCubeClient, PumpStandState, Variables as HiCubeVariables, VentState};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::{
    app::ValveOrPumpState,
    connections::{TCP_IP_TIMEOUT, TcpIpAdapter},
    get_inst_status_wait, log,
};

/// Commands that can be sent to the HiCube task.
pub enum HiCubeCommands {
    /// Set the venting valve state.
    SetVentingValveState(ValveOrPumpState),
    /// Set the pump stand state.
    SetPumpStandState(ValveOrPumpState),
}

/// Async task to listen to the HiCube status.
///
/// This task also will spawn the writing listener task, which listens to commands from the UI.
pub async fn pfeiffer_hicube_task(
    conf: PfeifferHiCubeConf,
    rx_hicube: mpsc::Receiver<HiCubeCommands>,
) {
    let mut rx_shutdown = crate::HALT_SENDER.get().unwrap().subscribe();

    let inst_status = get_inst_status_wait().await;

    // FIXME: Make sure this timeout is long enough with the actual instrument!
    let hicube_inst =
        match HiCubeClient::try_new_and_connect(&conf.tcp_ip_adapter.get_address(), TCP_IP_TIMEOUT)
            .await
        {
            Ok(hc) => hc,
            Err(e) => {
                log::err!("Failed to connect to HiCube: {}", e).await;
                return;
            }
        };

    let mut sub = hicube_inst.subscribe().await.unwrap();

    // spawn writing task
    tokio::spawn(pfeiffer_hicube_writing_task(hicube_inst, rx_hicube));

    // listen to messages
    loop {
        tokio::select! {
            Some(message) = sub.recv() => {
                match message {
                    HiCubeVariables::PumpStand(state) => {
                        if inst_status                            .lock()
                            .expect("Locking InstrumentStatus must work")
                            .set_hicube_pump_stand_state_and_ui(state)
                            .is_err() {
                            log::err_now!("Could not set HiCube pump stand state to instrument status.");
                        };
                    }
                    HiCubeVariables::Venting(state) => {
                        let st = match state {
                            VentState::Enabled => ValveOrPumpState::OpenOrOn,
                            VentState::Disabled => ValveOrPumpState::ClosedOrOff,
                        };
                        if inst_status
                            .lock()
                            .expect("Locking InstrumentStatus must work")
                            .set_hicube_vent_valve_state_and_ui(st)
                            .is_err()
                        {
                            log::err_now!("Could not set HiCube vent valve state to instrument status.");
                        };
                    }
                    _ => { /* Ignore variables we don't care about for setting status */ }
                }
            },
            _ = rx_shutdown.recv() => {
                break;
            }
        }
    }
}

/// HiCube writing task.
///
/// Task that listens for commands to write to the HiCube.
pub async fn pfeiffer_hicube_writing_task(
    mut hicube: HiCubeClient,
    mut rx_hicube: mpsc::Receiver<HiCubeCommands>,
) {
    let mut rx_shutdown = crate::HALT_SENDER.get().unwrap().subscribe();

    loop {
        tokio::select! {
            Some(cmd) = rx_hicube.recv() => {
                match cmd {
                    HiCubeCommands::SetVentingValveState(state) => {
                        let to_send = match state {
                            ValveOrPumpState::OpenOrOn => HiCubeVariables::Venting(VentState::Enabled),
                            ValveOrPumpState::ClosedOrOff => HiCubeVariables::Venting(VentState::Disabled),
                            _ => {
                                log::warning!("Received invalid venting valve state command to write to HiCube.").await;
                                continue;
                            }
                        };
                        if hicube.write(to_send).await.is_err() {
                            log::err!("Failed to write venting valve state to HiCube.").await;
                        };
                    }
                    HiCubeCommands::SetPumpStandState(state) => {
                        match state {
                            ValveOrPumpState::OpenOrOn => {
                                let to_send = HiCubeVariables::PumpStand(PumpStandState::On);
                                if hicube.write(to_send).await.is_err() {
                                    log::err!("Failed to write pump stand state to HiCube.").await;
                                };
                            }
                            // turn pump stand off by turning roughing and turbo pump to `false`.
                            ValveOrPumpState::ClosedOrOff => {
                                let to_send = HiCubeVariables::PumpStand(PumpStandState::Off);
                                if hicube.write(to_send).await.is_err() {
                                    log::err!("Failed to write pump stand state to HiCube.").await;
                                };
                            }
                            _ => {
                                log::warning!("Received invalid pump stand state command to write to HiCube.").await;
                            }
                        }
                    }
                }
            }
            // Shutdown the writer task
            _ = rx_shutdown.recv() => {
                break;
            }
        }
    }
}

/// Configuration for the Pfeiffer HiCube.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PfeifferHiCubeConf {
    /// TCP/IP adapter for the HiCube.
    pub tcp_ip_adapter: TcpIpAdapter,
}

impl Default for PfeifferHiCubeConf {
    fn default() -> Self {
        Self {
            tcp_ip_adapter: TcpIpAdapter::new_from_str("192.168.1.100:4840"),
        }
    }
}

/// Get a clone of the hicube command sender.
fn get_hicube_cmd_sender() -> mpsc::Sender<HiCubeCommands> {
    crate::instruments::HICUBE_COMMAND_SENDER
        .get()
        .expect("HiCube command sender must be initialized")
        .clone()
}

/// Convenience function to send a hicube command without awaiting.
///
/// If an error occurs, this error is logged.
pub fn send_hicube_command_now(cmd: HiCubeCommands) {
    let sender = get_hicube_cmd_sender();
    if let Err(e) = sender.try_send(cmd) {
        log::err_now!("Failed to send HiCube command: {}", e);
    }
}

/// Convenience function to send a hicube command with awaiting.
///
/// If an error occurs, this error is logged and also returned.
pub async fn send_hicube_cmd_w_err(cmd: HiCubeCommands) -> Result<()> {
    let sender = get_hicube_cmd_sender();
    if let Err(e) = sender.send(cmd).await {
        log::err!("Failed to send HiCube command: {}", e).await;
        return Err(e.into());
    }
    Ok(())
}
