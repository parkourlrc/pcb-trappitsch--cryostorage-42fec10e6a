//! All messages and states for a single valve.

use postcard_schema::Schema;
use serde::{Deserialize, Serialize};

/// State of a valve.
///
/// Undefined means that the state pins are either both high or both low. This is not a state that
/// can be set, but can occur if the valve transition does not complete successfully. It is also
/// used as the default state, as we do not know the state on first power-up.
#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize, Schema, PartialEq, Eq)]
pub enum ValveState {
    Open,
    Closed,
    #[default]
    Undefined,
}

impl ValveState {
    /// Get a bool representing whether the valve is open or not.
    pub fn is_open(&self) -> bool {
        matches!(self, ValveState::Open)
    }
}
