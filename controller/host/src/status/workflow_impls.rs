//! Implementations of status for the workflows.

use icd::{BakingState, FlowMeterState, ValveState};
use measurements::Temperature;
use sunpower_cryotelgt::CoolerState;

use crate::status::{InstrumentStatus, PressureReading};

impl InstrumentStatus {
    /// Get the pressures as a tuple.
    ///
    /// Returns: (p_chamber, p_transfer)
    pub fn get_pressures(&self) -> (PressureReading, PressureReading) {
        (
            self.pressure_chamber_current,
            self.pressure_transfer_current,
        )
    }

    /// Get the current sample temperature 
    pub fn get_sample_temperature(&self) -> Temperature {
        self.temperature_sample
    }

    /// Is baking turned off?
    pub fn is_baking_off(&self) -> bool {
        self.baking_curr == BakingState::Off
    }

    /// Is the water flow ok?
    pub fn is_water_flow_ok(&self) -> bool {
        self.flow_meter_curr == FlowMeterState::Ok
    }

    /// Is the cryocooler off?
    pub fn is_cryo_cooler_off(&self) -> bool {
        self.cooler_state == CoolerState::Disabled
    }

    /// Is the pump valve open?
    pub fn is_pump_valve_open(&self) -> bool {
        self.valve_pump_curr == ValveState::Open
    }

    /// Is the VCT dock gate valve open?
    pub fn is_vct_gate_open(&self) -> bool {
        self.vct_curr.is_gate_open()
    }
}
