//! Checks if a cool sample is safe.
//!
//! If the cryocooler is on, we want to make sure whenever the instruments are pulled that some of
//! them are still behaving as required.
//!
//! When the cryocooler turns on, the `COOL_SAMPLE_SAFETY_ACTIVE` AtomicBool will be set to true.
//! This bool can then be checked from the instruments polling routine.
//!
//! The instrument polling routine will run the safety function. If any of the following conditions
//! are `false`, the Buzzer will turn on and a safety message will be displayed.
//! - Waterflow fails (this means that the cryocooler is off due to the hardware interlock).
//! - Ion pump turned off.
//! - Pump stand turns off.

use std::sync::{Mutex, atomic::Ordering};

use measurements::{Temperature, TemperatureDelta};
use slint::{ComponentHandle, Weak};

use crate::{
    app::{ErrorDialog, UI},
    instruments::{InstrumentCommands, send_instr_cmd_now},
    log,
    safety::{IS_ALARM_RAISED, buzzer, show_alarm_dialog},
    status::InstrumentStatus,
};

// Store the safety UI dialog here if it exists.
static SMP_SFTY_UI: Mutex<Option<Weak<ErrorDialog>>> = Mutex::new(None);

// Take a reference to an instrument status and checks if the samples are safe.
//
// This is the cooling sample workflow. It makes sure that in the current instrument status, the
// following conditions are okay:
// - Waterflow is still okay (indicator that the cryocooler is good).
// - The ion pump is still on.
// - The pump stand is still on.
//
// If any of these conditions are not met, the buzzer is activated and the and an error dialog is
// raised. This error dialog should describe the problem and, when confirmed, pause the buzzer and
// safety check for some time for the user to fix the issue.
pub fn sample_safety(
    inst_stat: &InstrumentStatus,
    temperature_setpoint: &Temperature,
    safe_temperature_half_band: &TemperatureDelta,
) {
    // skip any safety check if the cryocooler is off or the alarm is muted.
    if inst_stat.is_cryo_cooler_off() {
        return;
    }

    // if let Err(err_msg) =
    match check_sample_safe(inst_stat, temperature_setpoint, safe_temperature_half_band) {
        Err(err_msg) => {
            // If the alarm is not muted, go in here.
            if !IS_ALARM_RAISED.load(Ordering::Relaxed) {
                buzzer().turn_on();

                // If UI is not yet set, the alarm will trigger again the next time around.
                if let Some(ui) = UI.get() {
                    ui.upgrade_in_event_loop(move |_| {
                        match show_alarm_dialog(err_msg, Some(&IS_ALARM_RAISED)) {
                            Ok(dialog) => {
                                let mut guard = SMP_SFTY_UI.lock().expect("Not poisoned");
                                *guard = Some(dialog);
                            }
                            Err(e_show) => log::err_now!("Failed to show alarm dialog: {}", e_show),
                        };
                    })
                    .expect("UI must be alive");
                }
            }
        }
        Ok(_) => {
            // so we recovered automatically
            if IS_ALARM_RAISED
                .compare_exchange(true, false, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                buzzer().turn_off();
                if let Some(ui) = UI.get() {
                    ui.upgrade_in_event_loop({
                        move |_| {
                            let mut guard = SMP_SFTY_UI.lock().expect("Not poisoned");
                            if let Some(wdr) = &*guard {
                                let wd = wdr.clone();
                                if let Some(diag) = wd.upgrade() {
                                    let _ = diag.hide();
                                }
                            }
                            *guard = None;
                        }
                    })
                    .expect("UI is running");
                }
                log::info_now!("Sample safety has been restored automatically.");
            }
        }
    }
}

/// Routine to check if the sample is actually safe.
///
/// Arguments:
/// - `inst_stat`: Ref to instrument status.
/// - `safe_temps`: Tuple of refs to minimum and maximum safe temperature.
///
/// Returns:
/// - Ok(()) if the sample is safe.
/// - Err() describing the problem (user displayable) if the sample is not safe.
pub fn check_sample_safe(
    inst_stat: &InstrumentStatus,
    temperature_setpoint: &Temperature,
    safe_temperature_half_band: &TemperatureDelta,
) -> Result<(), String> {
    let sample_temp_curr = inst_stat.get_sample_temperature();
    let min_safe_temp = *temperature_setpoint - *safe_temperature_half_band;
    let max_safe_temp = *temperature_setpoint + *safe_temperature_half_band;

    // if the temperature is too low, try to turn on the heater in case it kicked out.
    if sample_temp_curr < min_safe_temp + *safe_temperature_half_band / 2. {
        send_instr_cmd_now(InstrumentCommands::TurnLakeshoreHeaterOn);
    }

    // rest of the checks that would raise an alarm
    // FIXME: put waterflow back into checks
    // let wf = inst_stat.is_water_flow_ok();
    let ip = inst_stat.is_ion_pump_on();
    let ps = inst_stat.is_pumpstand_on();
    let sample_temp_chk =
        (min_safe_temp <= sample_temp_curr) && (sample_temp_curr <= max_safe_temp);

    // FIXME: Add waterflows back in
    if ip && ps && sample_temp_chk {
        return Ok(());
    }

    // if alarm is already raised, we do nothing with this error message.
    if IS_ALARM_RAISED.load(Ordering::Relaxed) {
        return Err(String::from("Alarm already raised."));
    }

    // so we need to fail with a proper error message.
    let mut err_msg = String::from(
        "The following component(s) have failed, which puts the sample in jeopardy.\n",
    );
    let mut log_msg = String::from("Sample safety alarm errors: ");
    // FIXME: Add waterflow back in
    // if !wf {
    //     err_msg.push_str("- Waterflow for cooler is not maintained.\n");
    //     log_msg.push_str("Water flow; ");
    // }
    if !ip {
        err_msg.push_str("- Ion pump is not turned on.\n");
        log_msg.push_str("Ion pump; ");
    }
    if !ps {
        err_msg.push_str("- Pump stand is not on.\n");
        log_msg.push_str("HiCube; ");
    }
    if !sample_temp_chk {
        err_msg.push_str(&format!(
            "- Sample temperature {:.1} K is not between safe range between {:.1} K and {:.1} K.",
            sample_temp_curr.as_kelvin(),
            min_safe_temp.as_kelvin(),
            max_safe_temp.as_kelvin(),
        ));
        log_msg.push_str("Sample temperature;");
    }

    log::warning_now!(&log_msg);
    Err(err_msg)
}
