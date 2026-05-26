//! This part handles the progess window for the workflows.

use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use slint::{ComponentHandle, Weak};
use tokio::sync::{mpsc, oneshot};

use crate::{
    app::{ProgressWindow, ProgressWindowLogic, get_ui_wait},
    workflows::timer::Timer,
};

/// Actions on how the user might interact with the WorkflowProgress window.
#[derive(Debug, Copy, Clone)]
pub enum WorkflowProgressAction {
    /// Cancel button was pressed.
    Cancel,
    /// Next button was pressed.
    Next,
    /// Ok button was pressed.
    Ok,
}

/// A structure that holds the buttons state.
///
/// The action button will be anbled if the Option is set to a name, otherwise disabled.
/// All other buttons are simple on/off, thus we create the enabled state.
#[derive(Debug, Default, Clone)]
pub struct WorkflowProgressButtons {
    cancel_button_enabled: bool,
    ok_button_enabled: bool,
    next_button_enabled: bool,
}

impl WorkflowProgressButtons {
    /// Create a new WorkflowProgressButtons structure with all buttons disabled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable the cancel button.
    pub fn enable_cancel(mut self) -> Self {
        self.cancel_button_enabled = true;
        self
    }

    /// Enable the ok button.
    pub fn enable_ok(mut self) -> Self {
        self.ok_button_enabled = true;
        self
    }

    /// Enable the next button.
    pub fn enable_next(mut self) -> Self {
        self.next_button_enabled = true;
        self
    }
}

/// The workflow progress display structure.
///
/// Handles all the necessary interactions with the progress window.
pub struct WorkflowProgress {
    /// A weak handle to the progress window itself.
    pw: Weak<ProgressWindow>,
    /// Receiver for Workflow progress actions.
    rx: mpsc::Receiver<WorkflowProgressAction>,
}

impl WorkflowProgress {
    /// Creates a new workflow progress window and displays it.
    ///
    /// This will set the given title and description. However, no further actions will be done.
    ///
    /// Arguments:
    /// - `title`: The title of the workflow progress window.
    /// - `desc`: The description of the process of the progress workflow window.
    /// - `ui`: The main UI that can start this whole thing in its main loop.
    pub async fn new(title: &str, desc: &str) -> Self {
        let (tx, rx) = mpsc::channel(16);
        let ui = get_ui_wait().await;

        let (txos, rxos) = oneshot::channel();
        ui.upgrade_in_event_loop({
            let tx = tx.clone();
            let title = title.into();
            let desc = desc.into();
            move |_| {
                let pw = get_progress_window(title, desc, tx)
                    .expect("ProgressWindow creation must succeed.");
                let _ = txos.send(pw.as_weak());
                pw.show().unwrap();
            }
        })
        .expect("UI must be alive.");
        let pw = rxos.await.expect("UI thread must be still alive");

        Self { pw, rx }
    }

    /// Check for a condition for a given amount of time.
    ///
    /// The only button enabled in this scenario is the cancel button.
    ///
    /// Arguments:
    /// - `dur`: Duration of the time to wait at most.
    /// - `msg`: Message to display.
    /// - `check`: Closure that returns `true`` if the check succeeds and `false` otherwise.
    ///
    /// Return:
    /// - Ok(()) If the check has succeeded.
    /// - Err(): Timer expired and check failed, cancel pressed and check failed, program shut down.
    pub async fn check_for<F>(&mut self, dur: Duration, msg: &str, check: F) -> Result<()>
    where
        F: Fn() -> bool + Send + 'static,
    {
        let mut rx_shutdown = crate::HALT_SENDER.get().unwrap().subscribe();

        self.set_buttons(WorkflowProgressButtons::new().enable_cancel());
        self.set_message(msg);

        let mut tim = Timer::new(dur);
        loop {
            tokio::select! {
                val = tim.remaining_time_once_per_sec() => {
                    match val {
                        Some(d) =>  {
                            if check() {
                                self.set_buttons(WorkflowProgressButtons::default());
                                self.update_timer(None);
                                return Ok(());
                            } else {
                                self.update_timer(Some(&d));
                            }
                        },
                        None => break,
                    }
                },
                _ = self.recv() => {
                    break;
                },
                _ = rx_shutdown.recv() => {
                    self.close();
                    bail!("Program shutdown received");
                },
            }
        }

        // Final chance for the check to succeed.
        self.set_buttons(WorkflowProgressButtons::default());
        self.update_timer(None);
        if check() {
            Ok(())
        } else {
            bail!("Check failed.")
        }
    }

    /// Close the progress window.
    pub fn close(&mut self) {
        self.pw
            .upgrade_in_event_loop(move |pw| {
                pw.hide().expect("PrgWin UI must be alive.");
            })
            .expect("PrgWin UI must be alive.");
    }

    /// Gives a final state message, makes the `Ok` button available, and waits for it to close.
    ///
    /// Arguments:
    /// - `msg`: The message to display.
    /// - `is_error`: Boolean to indicate if this is an error exit (`true`). In this case, the
    ///   message will be displayed in red.
    pub async fn finalize(&mut self, msg: &str, is_error: bool) {
        self.set_buttons(WorkflowProgressButtons::new().enable_ok());

        self.pw
            .upgrade_in_event_loop({
                let msg = msg.into();
                move |pw| {
                    pw.global::<ProgressWindowLogic>().set_message(msg);
                    pw.global::<ProgressWindowLogic>().set_error_mode(is_error);
                }
            })
            .expect("PrgWin UI is alive.");

        let _ = self.recv().await; // only one message is possible...

        self.close();
    }

    /// Present a notification and wait for the user to press next.
    ///
    /// Once next is received, set the buttons back to no buttons at all.
    ///
    /// Arguments:
    /// - `msg`: Message to be displayed.
    /// - `next_only`: If `true`, only the "Next" button will be active and cancelling will not be possible.
    ///
    /// Returns:
    /// - Ok(()): User pressed next.
    /// - Err(): User pressed cancel.
    pub async fn notify(&mut self, msg: &str, next_only: bool) -> Result<()> {
        let btns = if next_only {
            WorkflowProgressButtons::new().enable_next()
        } else {
            WorkflowProgressButtons::new().enable_cancel().enable_next()
        };
        self.set_buttons(btns);

        self.pw
            .upgrade_in_event_loop({
                let msg = msg.into();
                move |pw| {
                    pw.global::<ProgressWindowLogic>().set_message(msg);
                }
            })
            .expect("PrgWin UI is alive.");

        let res = match self.recv().await {
            WorkflowProgressAction::Cancel => Err(anyhow!("Cancel was pressed.")),
            WorkflowProgressAction::Next => Ok(()),
            _ => unreachable!(
                "Bug in notification: This should not be possible if the right buttons are displayed."
            ),
        };

        self.set_buttons(WorkflowProgressButtons::default());
        res
    }

    /// Wait for a timer to expire or for the user to press cancel.
    ///
    /// Arguments:
    /// - `dur`: Duration of the timer.
    /// - `msg`: The message that will be displayed during this timer.
    ///
    /// Return:
    /// - Ok(()) if the timer has expired.
    /// - Err() if the user has pressed cancel or the program shut down.
    pub async fn wait_for(&mut self, dur: Duration, msg: &str) -> Result<()> {
        let mut rx_shutdown = crate::HALT_SENDER.get().unwrap().subscribe();

        self.set_buttons(WorkflowProgressButtons::new().enable_cancel());
        self.set_message(msg);

        let mut tim = Timer::new(dur);
        loop {
            tokio::select! {
                val = tim.remaining_time_once_per_sec() => {
                    match val {
                        Some(d) => self.update_timer(Some(&d)),
                        None => {
                            self.set_buttons(WorkflowProgressButtons::default());
                            self.update_timer(None);
                            return Ok(());
                        },
                    }
                },
                _ = self.recv() => {
                    self.set_buttons(WorkflowProgressButtons::default());
                    bail!("Cancel was pressed before the timer expired.")
                },
                _ = rx_shutdown.recv() => {
                    self.close();
                    bail!("Program shutdown received");
                }
            }
        }
    }

    /// Receive the next command.
    ///
    /// If the channel is closed for some reason, this will panic.
    async fn recv(&mut self) -> WorkflowProgressAction {
        self.rx.recv().await.expect("Channel should not be closed")
    }

    /// Add a new set of buttons to the interface.
    fn set_buttons(&self, btns: WorkflowProgressButtons) {
        self.pw
            .upgrade_in_event_loop(move |pw| {
                pw.global::<ProgressWindowLogic>()
                    .set_cancel_button_enabled(btns.cancel_button_enabled);
                pw.global::<ProgressWindowLogic>()
                    .set_ok_button_enabled(btns.ok_button_enabled);
                pw.global::<ProgressWindowLogic>()
                    .set_next_button_enabled(btns.next_button_enabled);
            })
            .unwrap();
    }

    /// Set a new message to the display.
    fn set_message(&self, msg: &str) {
        self.pw
            .upgrade_in_event_loop({
                let msg = msg.into();
                move |pw| {
                    pw.global::<ProgressWindowLogic>().set_message(msg);
                }
            })
            .expect("PrgWin UI should be alive");
    }

    /// Show the vaccum plot (`true`) or hide it (`false`)
    pub fn show_vacuum_plot(&self, show_it: bool) {
        self.pw
            .upgrade_in_event_loop({
                move |pw| {
                    pw.global::<ProgressWindowLogic>()
                        .set_show_vacuum_plot(show_it);
                }
            })
            .expect("PrgWin UI should be alive");
    }

    /// Update the timer in the UI from reference to a duration.
    fn update_timer(&self, dur: Option<&Duration>) {
        let s = if let Some(d) = dur {
            let mut sec = d.as_secs();

            // add a second for rounding if necessary.
            if d.subsec_millis() >= 500 {
                sec += 1;
            }

            let hrs = sec / 3_600;
            sec -= hrs * 3_600;
            let min = sec / 60;
            sec -= min * 60;

            format!("{:02}:{:02}:{:02}", hrs, min, sec).into()
        } else {
            "".into()
        };

        self.pw
            .upgrade_in_event_loop({
                move |pw| {
                    pw.global::<ProgressWindowLogic>().set_timer(s);
                }
            })
            .expect("PrgWin UI must be alive.");
    }
}

/// Set up a new progress window with a given title and description.
fn get_progress_window(
    title: slint::SharedString,
    desc: slint::SharedString,
    tx: mpsc::Sender<WorkflowProgressAction>,
) -> Result<ProgressWindow> {
    let pw = ProgressWindow::new()?;
    pw.global::<ProgressWindowLogic>().set_title(title);
    pw.global::<ProgressWindowLogic>().set_description(desc);

    pw.global::<ProgressWindowLogic>()
        .on_cancel_button_pressed({
            let tx = tx.clone();
            move || {
                if let Err(e) = tx.try_send(WorkflowProgressAction::Cancel) {
                    eprintln!("Progress Window cancel button press did not succeed: {e}.");
                };
            }
        });
    pw.global::<ProgressWindowLogic>().on_ok_button_pressed({
        let tx = tx.clone();
        move || {
            if let Err(e) = tx.try_send(WorkflowProgressAction::Ok) {
                eprintln!("Progress Window ok button press did not succeed: {e}.");
            };
        }
    });
    pw.global::<ProgressWindowLogic>().on_next_button_pressed({
        let tx = tx.clone();
        move || {
            if let Err(e) = tx.try_send(WorkflowProgressAction::Next) {
                eprintln!("Progress Window next button press did not succeed: {e}.");
            };
        }
    });
    Ok(pw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_workflow_progress_buttons() {
        let btns = WorkflowProgressButtons::default();
        assert!(!btns.cancel_button_enabled);
        assert!(!btns.ok_button_enabled);
        assert!(!btns.next_button_enabled);
    }
}
