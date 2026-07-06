//! Handle the instrument status and, when status changes, update UI.
//!
//! Since InstrumentStatus is fairly big and shared between different modules in Arc<Mutex<>>,
//! implementations are broken up into differrent files in this module to keep a better overview.
//!
//! - `controller_impls.rs`: Handle updates from controller broadcasts and related UI updates.
//! - `instruments_impls.rs`: Handle updates from instruments and related UI updates.

use icd::{BakingState, FlowMeterState, LightState, ValveState, VctState};
use measurements::{Fraction, Power, Pressure, Temperature};
use pfeiffer_hicube::PumpStandState;
use sunpower_cryotelgt::CoolerState;

use crate::{app::ValveOrPumpState, instruments::omnicontrol::GaugeStatus};

mod controller_impls;
mod instruments_impls;
mod safety_impls;
mod workflow_impls;

#[derive(Debug)]
pub struct InstrumentStatus {
    baking_call: BakingState,
    baking_curr: BakingState,
    chamber_light: LightState,
    cooler_state: CoolerState,
    flow_meter_curr: FlowMeterState,
    ion_pump_state: PumpState,
    hi_cube_pump_stand_state: PumpStandState,
    hi_cube_venting_valve: ValveState,
    power_cooler_current: Power,
    heater_current_output: Fraction,
    pressure_chamber_current: PressureReading,
    pressure_chamber_gauge: GaugeStatus,
    pressure_transfer_current: PressureReading,
    pressure_transfer_gauge: GaugeStatus,
    temperature_bridge: Temperature,
    temperature_cooler: Temperature,
    temperature_sample: Temperature,
    temperature_setpoint: Temperature,
    valve_pump_call: ValveState,
    valve_pump_curr: ValveState,
    valve_transfer_call: ValveState,
    valve_transfer_curr: ValveState,
    vct_curr: VctState,
}

impl InstrumentStatus {
    /// Create a new InstrumentStatus with a given ui and default values.
    pub fn new() -> Self {
        Self {
            baking_call: BakingState::default(),
            baking_curr: BakingState::default(),
            chamber_light: LightState::Off,
            cooler_state: CoolerState::Disabled,
            flow_meter_curr: FlowMeterState::default(),
            hi_cube_pump_stand_state: PumpStandState::Other,
            hi_cube_venting_valve: ValveState::default(),
            ion_pump_state: PumpState::default(),
            power_cooler_current: Power::default(), // 0.0 W
            heater_current_output: Fraction::default(), // 0%
            pressure_chamber_current: PressureReading::default(), // Off
            pressure_chamber_gauge: GaugeStatus::Off,
            pressure_transfer_current: PressureReading::default(), // Off
            pressure_transfer_gauge: GaugeStatus::Off,
            temperature_bridge: Temperature::default(), // 0.0 K
            temperature_cooler: Temperature::default(), // 0.0 K
            temperature_sample: Temperature::default(), // 0.0 K
            temperature_setpoint: Temperature::default(), // 0.0 K
            valve_pump_call: ValveState::default(),
            valve_pump_curr: ValveState::default(),
            valve_transfer_call: ValveState::default(),
            valve_transfer_curr: ValveState::default(),
            vct_curr: VctState::default(),
        }
    }
}

/// Pressure reading of a givne gauge.
#[derive(Debug, Default, Copy, Clone)]
pub enum PressureReading {
    /// Valid pressure reading.
    Value(Pressure),
    /// Gauge is off.
    #[default]
    Off,
    /// Error occured.
    Error,
}

impl PressureReading {
    /// Get a string to directly display on the UI.
    pub fn as_shared_string(&self) -> slint::SharedString {
        let display_str = match self {
            PressureReading::Value(p) => &format!("{:.2E} mbar", p.as_millibars()),
            PressureReading::Off => "Off",
            PressureReading::Error => "Error",
        };
        display_str.into()
    }
}

/// Define a pump state.
///
/// A pump can generally be On, Off, or in an Undefined/Error state.
#[derive(Debug, Default, Copy, Clone)]
pub enum PumpState {
    /// Pump is on.
    On,
    /// Pump is off.
    Off,
    /// Pump is undefined or erroring.
    #[default]
    Error,
}

impl From<PumpState> for ValveOrPumpState {
    fn from(value: PumpState) -> Self {
        match value {
            PumpState::On => ValveOrPumpState::OpenOrOn,
            PumpState::Off => ValveOrPumpState::ClosedOrOff,
            PumpState::Error => ValveOrPumpState::SetDifferentFromCalled,
        }
    }
}

impl From<ValveState> for ValveOrPumpState {
    fn from(value: ValveState) -> Self {
        match value {
            ValveState::Open => ValveOrPumpState::OpenOrOn,
            ValveState::Closed => ValveOrPumpState::ClosedOrOff,
            ValveState::Undefined => ValveOrPumpState::UndefinedOrError,
        }
    }
}
