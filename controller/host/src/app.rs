//! This module contains the complete GUI logic for the application.

use std::{
    error::Error,
    sync::{Arc, Mutex},
    time::Duration,
};

use agilent_4uhv::HvState;
use icd::{BakingState, LightState, ValveState};
use measurements::Temperature;
use slint::{Model, SharedString, Weak};
use sunpower_cryotelgt::CoolerState;
use tokio::sync::{OnceCell, mpsc};

use crate::{
    PRG_CONF,
    controller::{ControllerCommands, send_cntrl_cmd_now},
    instruments::{
        InstrumentCommands,
        hi_cube::{HiCubeCommands, send_hicube_command_now},
        omnicontrol::Gauge,
        send_instr_cmd_now,
    },
    prg_config::PrgConfig,
    safety::test_alarm,
    samples::{get_sample_idx, swipe_swap_samples},
    status::PumpState,
    workflows::{WorkflowCommands, send_workflow_command_now},
};

slint::include_modules!();

pub static UI: OnceCell<Weak<AppWindow>> = OnceCell::const_new();

pub fn app_main() -> Result<(), Box<dyn Error>> {
    let ui = AppWindow::new()?;

    let _ = UI.set(ui.as_weak());
    let conf = PRG_CONF.get().expect("PrgConf is Initialized");

    // initialize the various GUI handlers
    let _controller_cmd_handler = ControllerCommandHandler::new(ui.as_weak());
    let _gui_cmd_handler = GuiCommandHandler::new(ui.as_weak(), Arc::clone(conf));
    let _instrument_cmd_handler = InstrumentCommandHandler::new(ui.as_weak());

    // Debug builds
    #[cfg(debug_assertions)]
    ui.global::<Logic>().set_admin_mode(true);

    ui.show()?;

    ui.run()?;
    Ok(())
}

/// Controller command handler for the GUI.
///
/// This handler controls all the Cryocooler controller commands and sends ControllerCommands to
/// the controller handler, which talks to poststation. This here is the intermediary between the
/// GUI and the rest of the logic.
struct ControllerCommandHandler {
    ui: AppWindow,
}

impl ControllerCommandHandler {
    /// Initialize all switches
    fn new(ui: Weak<AppWindow>) -> Self {
        let sf = Self { ui: ui.unwrap() };
        sf.init();
        sf
    }

    /// Initialize the controller command handler with the current and saved values.
    fn init(&self) {
        // init buttons
        self.baking();
        self.light_switch();
        self.transfer_valve_set_open();
        self.pump_valve_set_open();
    }

    fn baking(&self) {
        self.ui.global::<Logic>().on_baking_enabled({
            let ui = self.ui.as_weak();
            move |val| {
                let ui = ui.unwrap();
                let baking_state = match val {
                    true => {
                        let baking_time = ui.global::<Logic>().get_baking_time();
                        let time_sec = baking_time.hours * 3600
                            + baking_time.minutes * 60
                            + baking_time.seconds;
                        BakingState::On {
                            time_sec: time_sec as u64,
                        }
                    }
                    false => BakingState::Off,
                };
                send_workflow_command_now(WorkflowCommands::Baking(baking_state));
            }
        });
    }

    fn light_switch(&self) {
        self.ui.global::<Logic>().on_light_switch({
            move |val| {
                let light_stat = match val {
                    true => LightState::On,
                    false => LightState::Off,
                };
                send_cntrl_cmd_now(ControllerCommands::Light(light_stat));
            }
        });
    }

    fn pump_valve_set_open(&self) {
        self.ui.global::<Logic>().on_pump_valve_set_open({
            move |val| {
                let vst = match val {
                    true => ValveState::Open,
                    false => ValveState::Closed,
                };
                send_workflow_command_now(WorkflowCommands::PumpValve(vst));
            }
        });
    }

    fn transfer_valve_set_open(&self) {
        self.ui.global::<Logic>().on_transfer_valve_set_open({
            move |val| {
                let vst = match val {
                    true => ValveState::Open,
                    false => ValveState::Closed,
                };
                send_workflow_command_now(WorkflowCommands::TransferValve(vst));
            }
        });
    }
}

/// Handler for GUI-related command.
///
/// Examples here are: admin mode, exit, etc.
struct GuiCommandHandler {
    ui: AppWindow,
    conf: Arc<Mutex<PrgConfig>>,
}

impl GuiCommandHandler {
    fn new(ui: Weak<AppWindow>, conf: Arc<Mutex<PrgConfig>>) -> Self {
        let sf = Self {
            ui: ui.unwrap(),
            conf,
        };
        sf.init();
        sf
    }

    fn init(&self) {
        // set the sample names from config
        let model = self.ui.global::<Logic>().get_sample_model();
        let curr_samples = { self.conf.lock().expect("Poisoned").get_samples() };
        for (idx, (pos, smp)) in curr_samples.into_iter().enumerate() {
            let name = smp.get_name();
            let date_str = smp.get_date();
            model.set_row_data(idx, (date_str.into(), name.into(), pos.into()));
        }

        // buttons
        self.ready_vct_attach();
        self.test_alarm();
        self.admin_mode();
        self.close_button();
        self.edit_sample_name();
        self.sample_swiped();
    }

    fn ready_vct_attach(&self) {
        self.ui.global::<Logic>().on_send_ready_vct_attach({
            move |st| {
                send_cntrl_cmd_now(ControllerCommands::VctHandshake(st.into()));
                // State of the switch is set where the command is forwarded to controller.
            }
        });
    }

    fn admin_mode(&self) {
        self.ui.global::<Logic>().on_enter_admin_mode({
            let ui = self.ui.as_weak();
            let conf = Arc::clone(&self.conf);
            move || {
                let ui = ui.unwrap();
                if ui.global::<Logic>().get_admin_mode() {
                    ui.global::<Logic>().set_admin_mode(false);
                } else {
                    let keypad = Keypad::new().unwrap();
                    keypad.set_keypad_title("Enter PIN".into());
                    keypad.show().unwrap();

                    keypad.global::<KeypadLogic>().on_cancel_pressed({
                        let keypad = keypad.as_weak();
                        move || {
                            keypad.unwrap().hide().unwrap();
                        }
                    });

                    keypad.global::<KeypadLogic>().on_ok_pressed({
                        let keypad = keypad.as_weak();
                        let ui = ui.as_weak();
                        let pin_expected = conf.lock().expect("Poisoned").get_admin_pin();
                        move |code| {
                            if code.as_str() == pin_expected.as_str() {
                                ui.unwrap().global::<Logic>().set_admin_mode(true);
                            }
                            keypad.unwrap().hide().unwrap();
                        }
                    });
                }
            }
        });
    }

    fn test_alarm(&self) {
        self.ui.global::<Logic>().on_test_alarm_pressed({
            move || {
                test_alarm();
            }
        });
    }

    fn close_button(&self) {
        self.ui.global::<Logic>().on_close_program({
            move || {
                slint::quit_event_loop().unwrap();
            }
        });
    }

    fn edit_sample_name(&self) {
        self.ui.global::<Logic>().on_edit_sample_name({
            let ui = self.ui.as_weak();
            let cfg = Arc::clone(&self.conf);

            move |pos, name, _date_str| {
                let message = format!("Edit name of sample at position {}", pos.clone());
                let (tx, mut rx) = mpsc::channel(1);
                let kb = KeyboardInput::new(tx);
                kb.get_text_input(message.as_str(), name);

                let cfg = Arc::clone(&cfg);
                let ui = ui.clone();
                let fut = async move {
                    let answer = rx.recv().await;
                    if let Some(ans) = answer {
                        if let Ok(smp) = cfg.lock().expect("Poisoned").update_sample(&pos, &ans)
                            && let Some(idx) = get_sample_idx(&pos)
                        {
                            let model = ui.unwrap().global::<Logic>().get_sample_model();
                            model.set_row_data(
                                idx,
                                (smp.get_date().into(), smp.get_name().into(), pos),
                            );
                        } else {
                            eprintln!(
                                "Failed to update sample name or get index: no position {pos}"
                            );
                        }
                    } else {
                        eprintln!("Failed to update sample name: no answer from keyboard");
                    };
                };

                slint::spawn_local(async_compat::Compat::new(fut)).unwrap();
            }
        });
    }

    fn sample_swiped(&self) {
        self.ui.global::<Logic>().on_sample_swiped({
            move |pos, start, end| {
                let dx = end.x - start.x;
                let dy = end.y - start.y;

                let fut = swipe_swap_samples(pos, dx, dy);
                slint::spawn_local(async_compat::Compat::new(fut)).unwrap();
            }
        });
    }
}

/// Command handler for connected instruments.
struct InstrumentCommandHandler {
    ui: AppWindow,
}

impl InstrumentCommandHandler {
    fn new(ui: Weak<AppWindow>) -> Self {
        let sf = Self { ui: ui.unwrap() };
        sf.init();
        sf
    }

    fn init(&self) {
        self.chamber_vacuum();
        self.cryocooler_set_on();
        self.sample_temperature_setpoint();
        self.ion_pump_set_on();
        self.gauge_chamber_set_on();
        self.gauge_transfer_set_on();
        self.pump_stand_set_on();
        self.start_sample_transfer();
        self.vent_valve_set_on();
    }

    /// Change the chamber vacuum state upon request
    fn chamber_vacuum(&self) {
        self.ui.global::<Logic>().on_vent_chamber({
            move || {
                send_workflow_command_now(WorkflowCommands::ChamberVent);
            }
        });

        self.ui.global::<Logic>().on_pump_chamber({
            move || {
                send_workflow_command_now(WorkflowCommands::ChamberPump);
            }
        });
    }

    fn sample_temperature_setpoint(&self) {
        self.ui.global::<Logic>().on_set_setpoint_temp({
            move || {
                let keypad = Keypad::new().unwrap();
                keypad.set_keypad_title("New Setpoint Temperature (K)".into());
                keypad.set_spread_out_entry(false);
                keypad.show().unwrap();

                keypad.global::<KeypadLogic>().on_cancel_pressed({
                    let keypad = keypad.as_weak();
                    move || {
                        keypad.unwrap().hide().unwrap();
                    }
                });

                keypad.global::<KeypadLogic>().on_ok_pressed({
                    let keypad = keypad.as_weak();
                    move |setpoint| {
                        if let Ok(new_setpoint) = setpoint.as_str().parse::<f64>() {
                            send_instr_cmd_now(InstrumentCommands::SampleTargetTemperature(
                                Temperature::from_kelvin(new_setpoint),
                            ));
                        }
                        keypad.unwrap().hide().unwrap();
                    }
                });
            }
        });
    }

    // Toggle cooler state. UI is updated after command to instrument succeeded.
    fn cryocooler_set_on(&self) {
        self.ui.global::<Logic>().on_cryocooler_set_on({
            move |val| {
                let state = match val {
                    true => CoolerState::Enabled,
                    false => CoolerState::Disabled,
                };
                send_workflow_command_now(WorkflowCommands::CryoCoolerState(state));
            }
        });
    }

    // Toggle ion pump state. UI is updated after command to instrument succeeded.
    fn ion_pump_set_on(&self) {
        self.ui.global::<Logic>().on_ion_pump_set_on({
            move |val| {
                let state = match val {
                    true => HvState::On,
                    false => HvState::Off,
                };
                send_instr_cmd_now(InstrumentCommands::IonPumpState(state));
            }
        });
    }

    // Toggle chamber gauge state. UI is updated after subsequent pressure read.
    fn gauge_chamber_set_on(&self) {
        let ui = self.ui.as_weak();
        self.ui.global::<Logic>().on_chamber_gauge_set_on({
            move |val| {
                send_instr_cmd_now(InstrumentCommands::GaugeState((Gauge::Chamber, val.into())));
                ui.unwrap().global::<Logic>().set_chamber_gauge_is_on(val);
            }
        });
    }

    // Toggle transfer gauge state. UI is updated after subsequent pressure read.
    fn gauge_transfer_set_on(&self) {
        let ui = self.ui.as_weak();
        self.ui.global::<Logic>().on_transfer_gauge_set_on({
            move |val| {
                send_instr_cmd_now(InstrumentCommands::GaugeState((
                    Gauge::Transfer,
                    val.into(),
                )));
                ui.unwrap().global::<Logic>().set_transfer_gauge_is_on(val);
            }
        });
    }

    // Turn the pump stand on/off. UI is updated from readback from instrument.
    fn pump_stand_set_on(&self) {
        let ui = self.ui.as_weak();
        self.ui.global::<Logic>().on_pump_stand_set_on({
            move |val| {
                let state = match val {
                    true => PumpState::On,
                    false => PumpState::Off,
                };
                send_hicube_command_now(HiCubeCommands::SetPumpStandState(state));
                ui.unwrap().global::<Logic>().set_pump_stand_is_on(val);
            }
        });
    }

    /// Start a sample transfer
    fn start_sample_transfer(&self) {
        self.ui.global::<Logic>().on_initiate_sample_transfer({
            move || {
                send_workflow_command_now(WorkflowCommands::StartSampleTransfer);
            }
        });
    }

    // Turn on/off the venting process (open/close vent valve).
    fn vent_valve_set_on(&self) {
        let ui = self.ui.as_weak();
        self.ui.global::<Logic>().on_vent_valve_set_open({
            move |val| {
                let state = match val {
                    true => ValveState::Open,
                    false => ValveState::Closed,
                };
                send_hicube_command_now(HiCubeCommands::SetVentingValveState(state));
                ui.unwrap().global::<Logic>().set_vent_valve_is_open(val);
            }
        });
    }
}

struct KeyboardInput {
    tx: mpsc::Sender<SharedString>,
}

impl KeyboardInput {
    fn new(tx: mpsc::Sender<SharedString>) -> Self {
        Self { tx }
    }

    fn get_text_input(&self, message: &str, initial_text: SharedString) {
        let keyboard = Keyboard::new().unwrap();
        keyboard
            .global::<KeyboardLogic>()
            .set_message(message.into());
        keyboard
            .global::<KeyboardLogic>()
            .set_text_entered(initial_text.clone());

        keyboard.show().unwrap();

        keyboard.global::<KeyboardLogic>().on_cancel_pressed({
            let keyboard = keyboard.as_weak();
            let tx = self.tx.clone();
            move || {
                let initial_text = initial_text.clone();
                let tx = tx.clone();
                let fut = async move {
                    tx.send(initial_text.clone()).await.unwrap();
                };
                slint::spawn_local(async_compat::Compat::new(fut)).unwrap();
                keyboard.unwrap().hide().unwrap();
            }
        });

        keyboard.global::<KeyboardLogic>().on_ok_pressed({
            let keyboard = keyboard.as_weak();
            let tx = self.tx.clone();
            move |new_text| {
                let tx = tx.clone();
                let fut = async move {
                    tx.send(new_text).await.unwrap();
                };
                slint::spawn_local(async_compat::Compat::new(fut)).unwrap();
                keyboard.unwrap().hide().unwrap();
            }
        });

        keyboard.global::<KeyboardLogic>().on_backspace_pressed({
            let keyboard = keyboard.as_weak();
            move || {
                let keyboard = keyboard.unwrap();
                let text = keyboard.global::<KeyboardLogic>().get_text_entered();
                // cut the last character off the string
                let text = match text.as_str().char_indices().next_back() {
                    Some((idx, _)) => &text[0..idx],
                    None => text.as_str(),
                };
                keyboard
                    .global::<KeyboardLogic>()
                    .set_text_entered(text.into());
            }
        });
    }
}

/// An async function that allows us to wait for the UI to be set to the OnceCell.
///
/// If the UI is set, a clone to its weak handle will be returned immediately. Otherwise, it will
/// recheck every 100ms until the UI is set. This wait should only happen in the very beginning.
pub async fn get_ui_wait() -> Weak<AppWindow> {
    loop {
        if let Some(ui) = UI.get() {
            return ui.clone();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
