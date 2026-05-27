//! Implementations for handling instruments.

use std::collections::HashMap;

use anyhow::Result;

use icd::ValveState;
use measurements::{Power, Pressure, Temperature};
use pfeiffer_hicube::PumpStandState;
use slint::ComponentHandle;
use sunpower_cryotelgt::CoolerState;

use crate::{
    app::{Logic, PumpStandStateGUI, UI},
    instruments::omnicontrol::{Gauge, GaugeStatus},
    plots::{
        PressureDataPoint, PressurePlotCommands, TemperatureDataPoint, TemperaturePlotCommands,
        send_pressure_plot_cmd_now, send_temperature_plot_cmd_now,
    },
    status::{InstrumentStatus, PressureReading, PumpState},
};

impl InstrumentStatus {
    /// Set the Pfeiffer HiCube pump stand state and update the UI.
    ///
    /// Note that we only call the pump in an ON state when the whole pump stand is on!
    pub fn set_hicube_pump_stand_state_and_ui(&mut self, state: PumpStandState) -> Result<()> {
        self.hi_cube_pump_stand_state = state;

        let gui_state = match state {
            PumpStandState::On => PumpStandStateGUI::On,
            PumpStandState::Off => PumpStandStateGUI::Off,
            PumpStandState::SpinningUp => PumpStandStateGUI::SpinningUp,
            PumpStandState::SpinningDown => PumpStandStateGUI::SpinningDown,
            PumpStandState::Other => PumpStandStateGUI::Error,
        };

        let pump_stand_is_on = match gui_state {
            PumpStandStateGUI::On => true,
            PumpStandStateGUI::Off => false,
            PumpStandStateGUI::SpinningUp => true,
            PumpStandStateGUI::SpinningDown => false,
            PumpStandStateGUI::Error => false,
        };
        let ui = UI
            .get()
            .ok_or_else(|| anyhow::anyhow!("UI not set"))?
            .clone();
        ui.upgrade_in_event_loop(move |ui| {
            ui.global::<Logic>().set_pump_stand_state(gui_state);
            ui.global::<Logic>().set_pump_stand_is_on(pump_stand_is_on);
        })?;

        Ok(())
    }

    /// Set the Pfeiffer HiCube vent valve state and update UI.
    pub fn set_hicube_vent_valve_state_and_ui(&mut self, state: ValveState) -> Result<()> {
        self.hi_cube_venting_valve = state;
        let valve_is_open = matches!(state, ValveState::Open);

        let ui = UI
            .get()
            .ok_or_else(|| anyhow::anyhow!("UI not set"))?
            .clone();
        ui.upgrade_in_event_loop(move |ui| {
            ui.global::<Logic>().set_vent_valve_state(state.into());
            ui.global::<Logic>().set_vent_valve_is_open(valve_is_open);
        })?;

        Ok(())
    }

    /// Set ion pump state and update UI.
    pub fn set_ion_pump_state_and_ui(&mut self, state: PumpState) -> Result<()> {
        self.ion_pump_state = state;

        let ion_pump_switch_state = matches!(state, PumpState::On);
        let ui = UI
            .get()
            .ok_or_else(|| anyhow::anyhow!("UI not set"))?
            .clone();
        ui.upgrade_in_event_loop(move |ui| {
            ui.global::<Logic>().set_ion_pump_state(state.into());
            ui.global::<Logic>()
                .set_ion_pump_is_on(ion_pump_switch_state);
        })?;

        Ok(())
    }

    /// Set the setpoint temperature for the cooler and update UI.
    pub fn set_temperature_setpoint_and_ui(&mut self, setpoint: Temperature) -> Result<()> {
        self.temperature_setpoint = setpoint;

        let ui = UI
            .get()
            .ok_or_else(|| anyhow::anyhow!("UI not set"))?
            .clone();
        ui.upgrade_in_event_loop(move |ui| {
            ui.global::<Logic>()
                .set_target_temp(setpoint.as_kelvin().round() as i32);
        })?;

        Ok(())
    }

    /// Set the cooler state and the UI.
    pub fn set_cooler_state_and_ui(&mut self, state: CoolerState) -> Result<()> {
        self.cooler_state = state.clone();

        let ui = UI
            .get()
            .ok_or_else(|| anyhow::anyhow!("UI not set"))?
            .clone();
        ui.upgrade_in_event_loop(move |ui| {
            ui.global::<Logic>()
                .set_cryocooler_is_on(matches!(state, CoolerState::Enabled));
        })?;
        Ok(())
    }

    /// Set the current power of the cooler.
    pub fn set_power_cooler_current(&mut self, power: Power) {
        self.power_cooler_current = power;
    }

    /// Set the current pressures and gauge statuses.
    pub fn set_pressures(&mut self, pressure_hm: HashMap<Gauge, Option<Pressure>>) {
        // pressures
        self.pressure_chamber_current = match pressure_hm.get(&Gauge::Chamber) {
            Some(Some(p)) => PressureReading::Value(*p),
            Some(None) => PressureReading::Off,
            None => PressureReading::Error,
        };
        self.pressure_transfer_current = match pressure_hm.get(&Gauge::Transfer) {
            Some(Some(p)) => PressureReading::Value(*p),
            Some(None) => PressureReading::Off,
            None => PressureReading::Error,
        };

        // gauge statuses
        match self.pressure_chamber_current {
            PressureReading::Off => self.pressure_chamber_gauge = GaugeStatus::Off,
            PressureReading::Value(_) => self.pressure_chamber_gauge = GaugeStatus::On,
            _ => {}
        }
        match self.pressure_transfer_current {
            PressureReading::Off => self.pressure_transfer_gauge = GaugeStatus::Off,
            PressureReading::Value(_) => self.pressure_transfer_gauge = GaugeStatus::On,
            _ => {}
        }

        self.send_pressures_to_plot();
    }

    fn send_pressures_to_plot(&self) {
        if let (PressureReading::Value(p_chamber), PressureReading::Value(p_transfer)) = (
            self.pressure_chamber_current,
            self.pressure_transfer_current,
        ) {
            let dp = PressureDataPoint {
                ts: chrono::Local::now(),
                chamber: p_chamber.as_millibars(),
                transfer: p_transfer.as_millibars(),
            };

            send_pressure_plot_cmd_now(PressurePlotCommands::AddDataPoint(dp));
        }
    }

    /// Set temperature values from instrument status.
    pub fn set_temperatures(
        &mut self,
        bridge: Temperature,
        cooler: Temperature,
        sample: Temperature,
    ) {
        self.temperature_bridge = bridge;
        self.temperature_cooler = cooler;
        self.temperature_sample = sample;

        self.send_temperatures_to_plot();
    }

    /// Send temperatures to plot
    ///
    /// We only want to plot (and save) data if at least one temperature is non-zero, as zero
    /// values represent an error in reading (i.e., sensor is disconnected).
    pub fn send_temperatures_to_plot(&self) {
        if self.temperature_bridge.as_kelvin() == 0.0
            && self.temperature_cooler.as_kelvin() == 0.0
            && self.temperature_sample.as_kelvin() == 0.0
        {
            return;
        }

        let dp = TemperatureDataPoint {
            ts: chrono::Local::now(),
            sample: self.temperature_sample.as_kelvin(),
            bridge: self.temperature_bridge.as_kelvin(),
            cooler: self.temperature_cooler.as_kelvin(),
        };
        send_temperature_plot_cmd_now(TemperaturePlotCommands::AddDataPoint(dp));
    }

    /// Update the UI with the current instrument status.
    pub fn update_ui_instrument_status(&self) {
        if let Some(ui) = UI.get() {
            let sample_temp = self.temperature_sample.as_kelvin().round() as i32;
            let bridge_temp = self.temperature_bridge.as_kelvin().round() as i32;
            let cooler_temp = self.temperature_cooler.as_kelvin().round() as i32;

            let cooler_current_power = if self.cooler_state == CoolerState::Enabled {
                self.power_cooler_current.as_watts().round() as i32
            } else {
                0
            };

            let pressure_chamber_display = self.pressure_chamber_current.as_shared_string();
            let pressure_chamber_gauge_is_on: bool = self.pressure_chamber_gauge.into();
            let pressure_transfer_display = self.pressure_transfer_current.as_shared_string();
            let pressure_transfer_gauge_is_on: bool = self.pressure_transfer_gauge.into();

            ui.upgrade_in_event_loop(move |ui| {
                ui.global::<Logic>().set_sample_temp(sample_temp);
                ui.global::<Logic>().set_bridge_temp(bridge_temp);
                ui.global::<Logic>().set_cooler_temp(cooler_temp);
                ui.global::<Logic>().set_current_power(cooler_current_power);
                ui.global::<Logic>()
                    .set_chamber_pressure(pressure_chamber_display);
                ui.global::<Logic>()
                    .set_chamber_gauge_is_on(pressure_chamber_gauge_is_on);
                ui.global::<Logic>()
                    .set_transfer_pressure(pressure_transfer_display);
                ui.global::<Logic>()
                    .set_transfer_gauge_is_on(pressure_transfer_gauge_is_on);
            })
            .unwrap();
        }
    }
}
