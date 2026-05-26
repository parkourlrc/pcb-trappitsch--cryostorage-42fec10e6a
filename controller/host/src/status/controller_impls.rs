//! Implementations for handling the controller.

use anyhow::{Result, bail};
use icd::{BakingState, FlowMeterState, InstrumentState, LightState, ValveState};
use slint::ComponentHandle;

use crate::{
    app::{BakingTime, Logic, UI, ValveOrPumpState},
    status::InstrumentStatus,
};

impl InstrumentStatus {
    /// Initialize the call states to the current states.
    ///
    /// This is used at startup to avoid false error states.
    /// If the status is set properly, this will return an `Ok(())`, otherwise an error.
    pub fn initialize_call_states_from_bc(&mut self) -> Result<()> {
        self.baking_call = self.baking_curr.clone();
        self.valve_pump_call = self.valve_pump_curr.clone();
        self.valve_transfer_call = self.valve_transfer_curr.clone();

        // set UI with buttons (baking is set from broadcast)
        let valve_pump_is_open = self.valve_pump_call.is_open();
        let valve_transfer_is_open = self.valve_transfer_call.is_open();
        if let Some(ui) = UI.get() {
            ui.upgrade_in_event_loop(move |ui| {
                ui.global::<Logic>()
                    .set_pump_valve_is_open(valve_pump_is_open);
                ui.global::<Logic>()
                    .set_transfer_valve_is_open(valve_transfer_is_open);
            })?;
            Ok(())
        } else {
            bail!("UI not set");
        }
    }

    /// Set the state of the light.
    pub fn set_chamber_light(&mut self, state: LightState) {
        self.chamber_light = state;
    }

    /// Set the state of the light and update UI.
    pub fn set_chamber_light_and_ui(&mut self, state: LightState) -> Result<()> {
        self.chamber_light = state.clone();

        let light_is_on = matches!(state, LightState::On);
        let ui = UI
            .get()
            .ok_or_else(|| anyhow::anyhow!("UI not set"))?
            .clone();
        ui.upgrade_in_event_loop(move |ui| {
            ui.global::<Logic>().set_light_is_on(light_is_on);
        })?;

        Ok(())
    }

    /// Set valve pump called state.
    pub fn set_valve_pump_call(&mut self, state: ValveState) {
        self.valve_pump_call = state;
    }

    /// Set valve transfer called state.
    pub fn set_valve_transfer_call(&mut self, state: ValveState) {
        self.valve_transfer_call = state;
    }

    /// Update status from a broadcast message. Then update UI.
    pub fn update_from_controller_broadcast(&mut self, status: InstrumentState) {
        self.baking_curr = status.baking;
        self.flow_meter_curr = status.flow_meter;
        self.valve_pump_curr = status.pump_valve;
        self.valve_transfer_curr = status.transfer_valve;
        self.vct_curr = status.vct;
        self.update_ui_controler_broadcast();
    }

    /// Update the UI after a controller broadcast.
    fn update_ui_controler_broadcast(&self) {
        if let Some(ui) = UI.get() {
            // Baking
            let (baking_is_enabled, baking_time) = match self.baking_curr {
                BakingState::On { time_sec } => {
                    let baking_time = BakingTime {
                        hours: (time_sec.div_euclid(3600)) as i32,
                        minutes: ((time_sec % 3600).div_euclid(60)) as i32,
                        seconds: (time_sec % 60) as i32,
                    };
                    (true, baking_time)
                }
                BakingState::Off => (false, BakingTime::default()),
            };

            // Water flow
            let water_flow_ok = matches!(self.flow_meter_curr, FlowMeterState::Ok);

            // Pump valve status
            let valve_pump_state = if self.valve_pump_call == self.valve_pump_curr {
                match self.valve_pump_curr {
                    ValveState::Open => ValveOrPumpState::OpenOrOn,
                    ValveState::Closed => ValveOrPumpState::ClosedOrOff,
                    // Undefined in call state only possible at startup.
                    ValveState::Undefined => ValveOrPumpState::UndefinedOrError,
                }
            } else {
                match self.valve_pump_curr {
                    ValveState::Undefined => ValveOrPumpState::UndefinedOrError,
                    _ => ValveOrPumpState::SetDifferentFromCalled,
                }
            };

            // Transfer valve status
            let valve_transfer_state = if self.valve_transfer_call == self.valve_transfer_curr {
                match self.valve_transfer_curr {
                    ValveState::Open => ValveOrPumpState::OpenOrOn,
                    ValveState::Closed => ValveOrPumpState::ClosedOrOff,
                    // Undefined in call state only possible at startup.
                    ValveState::Undefined => ValveOrPumpState::UndefinedOrError,
                }
            } else {
                match self.valve_transfer_curr {
                    ValveState::Undefined => ValveOrPumpState::UndefinedOrError,
                    _ => ValveOrPumpState::SetDifferentFromCalled,
                }
            };

            // Is the VCT attached?
            let is_vct_attached = self.vct_curr.is_attached();

            // Update UI in event loop
            ui.upgrade_in_event_loop(move |ui| {
                ui.global::<Logic>()
                    .set_baking_is_enabled(baking_is_enabled);
                if baking_is_enabled {
                    ui.global::<Logic>().set_baking_time(baking_time);
                };
                ui.global::<Logic>().set_water_flow_ok(water_flow_ok);
                ui.global::<Logic>().set_pump_valve_state(valve_pump_state);
                ui.global::<Logic>()
                    .set_transfer_valve_state(valve_transfer_state);
                ui.global::<Logic>().set_is_vct_attached(is_vct_attached);
            })
            .unwrap();
        }
    }
}
