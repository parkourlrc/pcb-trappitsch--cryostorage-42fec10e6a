//! Create slint dialogs that display with the proper styling.

use std::fmt::Display;

use anyhow::Result;
use slint::ComponentHandle;

use crate::{
    app::{ErrorDialog, UI},
    log,
};

/// Show a generic error dialog with the title Error and then the given message.
fn show_error_dialog<D: Display>(msg: D) -> Result<()> {
    let dialog = ErrorDialog::new()?;

    let msg = format!("{}", msg);
    dialog.set_error_message(msg.into());

    let dlg = dialog.as_weak();
    dialog.on_close_pressed(move || {
        dlg.unwrap().window().hide().unwrap();
    });

    dialog.show()?;
    Ok(())
}

/// Helper function to show an error dialog in the UI from an error type.
///
/// This properly spawns in the main thread and thus can be pubic.
pub fn error_dialog_helper(e: anyhow::Error) {
    if let Some(ui) = UI.get() {
        ui.upgrade_in_event_loop(move |_| {
            if let Err(e_show) = show_error_dialog(e) {
                log::err_now!("Failed to show error dialog: {}", e_show);
            };
        })
        .expect("UI must be alive");
    } else {
        log::err_now!("Cannot get ui in error dialog helper.");
    }
}
