//! Create slint dialogs that display with the proper styling.

use std::fmt::Display;

use anyhow::Result;
use slint::ComponentHandle;

use tokio::sync::mpsc;

use crate::{
    app::{ConfirmDialog, ErrorDialog, UI},
    log,
};

/// Show a generic error dialog with the title Error and then the given message.
fn show_confirm_dialog(msg: String, tx: mpsc::Sender<bool>) -> Result<()> {
    let dialog = ConfirmDialog::new()?;

    dialog.set_confirm_message(msg.into());

    let dlg = dialog.as_weak();

    dialog.on_no_pressed({
        let tx = tx.clone();
        let dlg = dlg.clone();
        move || {
            if tx.try_send(false).is_err() {
                log::err_now!("Could not 'cancel' signal from confirmation dialog.")
            };
            dlg.unwrap().window().hide().unwrap();
        }
    });

    dialog.on_yes_pressed(move || {
        if tx.try_send(true).is_err() {
            log::err_now!("Could not 'ok' signal from confirmation dialog.")
        };
        dlg.unwrap().window().hide().unwrap();
    });

    dialog.show()?;
    Ok(())
}

/// Helper function to show a confirm dialog in the UI from an error type.
///
/// Returns:
/// - `true` if 'Ok' was pressed, otherwise `false`
pub async fn confirm_dialog_helper(msg: String) -> bool {
    if let Some(ui) = UI.get() {
        let (tx, mut rx) = mpsc::channel(3);
        ui.upgrade_in_event_loop({
            move |_| {
                if let Err(e_show) = show_confirm_dialog(msg, tx) {
                    log::err_now!("Failed to show confirm dialog: {}", e_show);
                };
            }
        })
        .expect("UI must be alive");

        let answer = rx.recv().await;
        match answer {
            Some(i) => i,
            None => {
                log::err!("Confirm dialog channel receive error").await;
                false
            }
        }
    } else {
        log::err_now!("Cannot get ui in error dialog helper.");
        false
    }
}

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
