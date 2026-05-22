//! Test the buzzer alarm with the click of a button.

use crate::{
    app::UI,
    log,
    safety::{buzzer, show_alarm_dialog},
};

pub fn test_alarm() {
    let msg = String::from(
        "This is a test alarm. You should hear a buzzer. When you click Close below, the test alarm will be finished and will not reactivate.",
    );

    buzzer().turn_on();

    if let Some(ui) = UI.get() {
        ui.upgrade_in_event_loop(move |_| {
            if let Err(e_show) = show_alarm_dialog(msg, None) {
                log::err_now!("Failed to show alarm dialog: {}", e_show);
            };
        })
        .expect("UI must be alive");
    }
}
