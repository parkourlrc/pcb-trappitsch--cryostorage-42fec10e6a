//! This part handles the progess window for the workflows.

use anyhow::Result;
use slint::{ComponentHandle, Weak};
use tokio::sync::{mpsc, oneshot, watch};

use crate::{
    app::{AppWindow, ProgressWindow, ProgressWindowLogic},
    log,
};

/// Commands that can be sent to an active progress window.
pub enum ProgressWindowCommands {
    /// Set to true if the button should be enabled, otherwise false.
    EnableButton(bool),
    /// Set the message as an error message (in red).
    EnableErrorMode(bool),
    /// Set the text of the button to the following. Defaults to `Cancel`.
    SetButtonText(String),
    /// Set a new message.
    SetMessage(String),
    /// Update the timer. Set this to "" if the timer should be removed.
    UpdateTimer(String),
}

/// Create a new progress window and spawn out the update task.
pub fn new_progress_window(
    rx: mpsc::Receiver<ProgressWindowCommands>,
    title: &str,
    description: &str,
    tx_pw: oneshot::Sender<Weak<ProgressWindow>>,
    tx_button_press: watch::Sender<()>,
) -> Result<()> {
    let pgrw = ProgressWindow::new()?;

    pgrw.global::<ProgressWindowLogic>().set_title(title.into());
    pgrw.global::<ProgressWindowLogic>()
        .set_description(description.into());

    let pw = pgrw.as_weak();
    slint::spawn_local(async_compat::Compat::new(progress_window_task(
        rx,
        pw,
        tx_button_press,
    )))?;
    let _ = tx_pw.send(pgrw.as_weak());

    pgrw.show()?;
    Ok(())
}

/// An async task to communicate with the progress window.
async fn progress_window_task(
    mut rx: mpsc::Receiver<ProgressWindowCommands>,
    pw: Weak<ProgressWindow>,
    tx_button_press: watch::Sender<()>,
) {
    let pw = pw.unwrap();
    pw.global::<ProgressWindowLogic>().on_button_pressed({
        move || {
            if tx_button_press.send(()).is_err() {
                eprintln!("A ProgressWindow tried to send a button press to a closed channel. This should never happen!");
            };
        }
    });

    let mut rx_shutdown = crate::HALT_SENDER.get().unwrap().subscribe();

    loop {
        tokio::select! {
            Some(msg) = rx.recv() => {
                use ProgressWindowCommands::*;
                match msg {
                    EnableButton(en) => {
                        pw.global::<ProgressWindowLogic>().set_button_enabled(en);
                    },
                    EnableErrorMode(en) => {
                        pw.global::<ProgressWindowLogic>().set_error_mode(en);
                    },
                    SetButtonText(txt) => {
                        pw.global::<ProgressWindowLogic>().set_button_text(txt.into());
                    },
                    SetMessage(msg) => {
                        pw.global::<ProgressWindowLogic>().set_message(msg.into());
                    },
                    UpdateTimer(tmr) => {
                        pw.global::<ProgressWindowLogic>().set_timer(tmr.into());
                    }
                }
            }
            _ = rx_shutdown.recv() => {
                break;
            }
        }
    }
}

/// Shortcut to send a message to a progress window.
pub async fn prg_msg(tx: &mpsc::Sender<ProgressWindowCommands>, msg: &str) {
    prg_cmd(tx, ProgressWindowCommands::SetMessage(msg.into())).await;
}

/// Shortcut to send something to the progress window.
pub async fn prg_cmd(tx: &mpsc::Sender<ProgressWindowCommands>, cmd: ProgressWindowCommands) {
    if let Err(e) = tx.send(cmd).await {
        log::err!("Sending command to progress window failed: {}", e).await;
    };
}

/// Display an error message to the user, activate the `Close` button and wait for a button press.
pub async fn disp_error_and_close(
    tx: &mpsc::Sender<ProgressWindowCommands>,
    err_msg: &str,
    mut rx_btn: watch::Receiver<()>,
) {
    rx_btn.mark_unchanged();
    prg_msg(tx, err_msg).await;
    prg_cmd(tx, ProgressWindowCommands::SetButtonText("Close".into())).await;
    prg_cmd(tx, ProgressWindowCommands::EnableButton(true)).await;
    prg_cmd(tx, ProgressWindowCommands::EnableErrorMode(true)).await;
    let _ = rx_btn.changed().await;
}

/// Close the progress window from the UI main loop.
pub fn close_prg_window(ui: Weak<AppWindow>, pw: Weak<ProgressWindow>) {
    ui.upgrade_in_event_loop(move |_| {
        let _ = pw.unwrap().hide();
    })
    .expect("UI must be alive");
}

/// Display a notification to fill N2 balloon and wait until the "Next" button was pressed.
pub async fn n2_notification(tx_window_cmd: &mpsc::Sender<ProgressWindowCommands>, rx_button_press: &mut watch::Receiver<()>) {
    prg_msg(
        tx_window_cmd,
        "Ensure that there is nitrogen in the balloon and press next.",
    )
    .await;
    prg_cmd(
        tx_window_cmd,
        ProgressWindowCommands::SetButtonText("Next".into()),
    )
    .await;
    prg_cmd(tx_window_cmd, ProgressWindowCommands::EnableButton(true)).await;
    let _ = rx_button_press.changed().await;
}
