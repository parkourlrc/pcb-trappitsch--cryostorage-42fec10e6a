//! VCT state and handshake messages.

use postcard_schema::Schema;
use serde::{Deserialize, Serialize};

/// State of a VCT (gate or attach) connection.
#[derive(Clone, Debug, Default, Serialize, Deserialize, Schema)]
pub enum VctConnectedState {
    /// Valves are open or might be open / attach is active.
    Connected,
    /// Valves are closed / attach is inactive.
    #[default]
    Disconnected,
}

/// Full VCT state (gate and attach).
///
/// This is the state of the gate and attach connection signaled by the VCT.
#[derive(Clone, Debug, Default, Serialize, Deserialize, Schema)]
pub struct VctState {
    /// Connected state if the dock valves are open (connected) or closed (disconnected).
    pub gate: VctConnectedState,
    /// Connected state if the attach procedure has been initialized/completed by Leica controller.
    pub attach: VctConnectedState,
}

impl VctState {
    /// Get if the VCT is attached (or the attachment procedure has been started).
    pub fn is_attached(&self) -> bool {
        matches!(self.attach, VctConnectedState::Connected)
    }

    /// Get if the dock gate valve is open.
    pub fn is_gate_open(&self) -> bool {
        matches!(self.gate, VctConnectedState::Connected)
    }
}

/// Handshake message.
///
/// This represents the message we want to send to our electronics to signal if we are ready or not
/// for the attach procedure from the Leica controller.
#[derive(Clone, Debug, Default, Serialize, Deserialize, Schema)]
pub enum VctHandshake {
    /// We are ready for the attach procedure.
    Ready,
    /// We are not ready for the attach procedure.
    #[default]
    NotReady,
}

impl From<bool> for VctHandshake {
    fn from(value: bool) -> Self {
        if value {
            VctHandshake::Ready
        } else {
            VctHandshake::NotReady
        }
    }
}

impl From<&VctHandshake> for bool {
    fn from(value: &VctHandshake) -> Self {
        match value {
            VctHandshake::Ready => true,
            VctHandshake::NotReady => false,
        }
    }
}
