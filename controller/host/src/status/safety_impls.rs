//! Implementations for safety checks.

use pfeiffer_hicube::PumpStandState;

use crate::{app::ValveOrPumpState, status::InstrumentStatus};

impl InstrumentStatus {
    /// Check if the ion pump is currently on.
    pub fn is_ion_pump_on(&self) -> bool {
        matches!(self.ion_pump_state, ValveOrPumpState::OpenOrOn)
    }

    /// Check if the pump stand is on.
    pub fn is_pumpstand_on(&self) -> bool {
        matches!(self.hi_cube_pump_stand_state, PumpStandState::On)
    }
}
