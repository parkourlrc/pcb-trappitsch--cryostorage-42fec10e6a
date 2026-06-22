//! Handles our various safeties that we want to integrate.
//!
//! See the individual safety routines to check what they actually do.
//! This module file integrates the general features we need for safety.

mod buzzer;
mod sample_safety;
mod test_alarm;

pub use buzzer::buzzer;
pub use sample_safety::sample_safety;
pub use test_alarm::test_alarm;

use anyhow::Result;
use slint::ComponentHandle;

use std::{
    fmt::Display,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use tokio::time::sleep;

use crate::app::ErrorDialog;

/// Duration and status of alarm muting.
pub static IS_ALARM_MUTED: AtomicBool = AtomicBool::new(false);
static ALARM_PAUSE: Duration = Duration::from_mins(5);

/// This routine can be spawned in order to pause a given alarm an let the user fix things.
///
/// This routine will generally be triggered by the user acknowledging the issue dialog by clicking
/// the button. This routine will then first deactivate the buzzer, then wait for 5 minutes, and
/// then reactivate the given safety feature again.
async fn pause_sfty(is_alarm_muted: &'static AtomicBool) {
    buzzer().turn_off();
    sleep(ALARM_PAUSE).await;
    is_alarm_muted.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Show an alarm dialog with a given message.
///
/// When the `Close` button is pressed, the buzzer turns off.
///
/// Arguments:
/// - `D`: What should be displayed.
/// - `atomic_check`: AtomicBool that will be turned off (no further checking) when the dialog is
///   activated. After the user hits "Close", this will be rea-activated in a given time span.
///   If `None`, nothign will be reactivated.
pub fn show_alarm_dialog<D: Display>(
    msg: D,
    is_alarm_muted: Option<&'static AtomicBool>,
) -> Result<()> {
    let dialog = ErrorDialog::new()?;

    let msg = format!(
        "{}\n\n\
        Press the Close button below to mute the alarm and fix the issue(s). \
        The alarm will reactivate, if necessary, in {} seconds.",
        msg,
        ALARM_PAUSE.as_secs()
    );

    dialog.set_error_header("Alarm!".into());
    dialog.set_error_message(msg.into());

    let dlg = dialog.as_weak();
    dialog.on_close_pressed({
        move || {
            dlg.unwrap().window().hide().unwrap();
            if let Some(a) = is_alarm_muted {
                slint::spawn_local(async_compat::Compat::new(pause_sfty(a))).unwrap();
            } else {
                // Since no `is_alarm_muted` was provided, we just turn the alarm off for good
                buzzer().turn_off();
            }
        }
    });

    // turn off the retriggering of the alarm right before we show the dialog now.
    if let Some(a) = is_alarm_muted {
        a.store(true, Ordering::Relaxed);
    }

    dialog.show()?;
    Ok(())
}
