//! Add a simple logging utility that logs messages to the GUI.
//!
//! These messages should also ultimately be saved to a log file whenever a new message shows up.

use crate::app::get_ui_wait;

use tokio::{
    sync::{OnceCell, mpsc},
    time::{Instant, sleep_until},
};

mod logger;

pub(crate) use logger::{Level, LogHandler, LogMessage};

pub static LOG_SENDER: OnceCell<mpsc::Sender<LogMessage>> = OnceCell::const_new();

pub const LOG_FNAME: &str = "cryostorage.log";

/// Async handler that runs the LogHandler.
///
/// Shuts down when a shutdown signal is received.
pub async fn log_handler_task(mut log_handler: LogHandler) {
    // wait till UI is set.
    let _ = get_ui_wait().await;

    let mut next_rotation = Instant::now() + crate::LOG_ROTATION_DURATION;
    let mut rx_shutdown = crate::HALT_SENDER.get().unwrap().subscribe();
    loop {
        tokio::select! {
            message = log_handler.recv() => {
                if let Some(msg) = message {
                    log_handler.add_new_message(msg);
                }
            }
            _ = sleep_until(next_rotation) => {
                log_handler.rotate_log_file();
                crate::log::info!("Rotated {} file", LOG_FNAME).await;
                next_rotation = Instant::now() + crate::LOG_ROTATION_DURATION;
            }
            _ = rx_shutdown.recv() => {
                break;
            }
        }
    }
}

/// Get a clone of the log sender.
fn get_log_sender() -> mpsc::Sender<LogMessage> {
    LOG_SENDER
        .get()
        .expect("Log sender must be initialized")
        .clone()
}

/// Convenience function to await sending a log message.
///
/// If an error occurs, this error is printed to stderr. Otherwise, the program will continue as
/// normal.
pub async fn send_log_message(msg: LogMessage) {
    let ls = get_log_sender();
    if let Err(e) = ls.send(msg).await {
        eprintln!("Could not send log message: {}", e);
    }
}

/// Convenience function to send a log message without awaiting.
///
/// If an error occurs, this error is printed to stderr. Otherwise, the program will continue as
/// normal.
pub fn send_log_message_now(msg: LogMessage) {
    let ls = get_log_sender();
    if let Err(e) = ls.try_send(msg) {
        eprintln!("Could not send log message now: {}", e);
    }
}

/// Macro to send a info log message with awaiting.
///
/// Takes the same arguments as `format!` if more than just a &str is supplied.
#[allow(unused)]
macro_rules! info {
    ($fmt:expr) => {
        crate::log::send_log_message(crate::log::LogMessage::new_info($fmt))
    };
    ($fmt:expr, $($arg:tt)*) => {
        {
            let msg = format!($fmt, $($arg)*);
            crate::log::send_log_message(crate::log::LogMessage::new_info(&msg))
        }
    };
}

/// Macro to send a warning log message with awaiting.
///
/// Takes the same arguments as `format!` if more than just a &str is supplied.
#[allow(unused)]
macro_rules! warning {
    ($fmt:expr) => {
        crate::log::send_log_message(crate::log::LogMessage::new_warning($fmt))
    };
    ($fmt:expr, $($arg:tt)*) => {
        {
            let msg = format!($fmt, $($arg)*);
            crate::log::send_log_message(crate::log::LogMessage::new_warning(&msg))
        }
    };
}

/// Macro to send a warning log message with awaiting.
///
/// Takes the same arguments as `format!` if more than just a &str is supplied.
#[allow(unused)]
macro_rules! err {
    ($fmt:expr) => {
        crate::log::send_log_message(crate::log::LogMessage::new_error($fmt))
    };
    ($fmt:expr, $($arg:tt)*) => {
        {
            let msg = format!($fmt, $($arg)*);
            crate::log::send_log_message(crate::log::LogMessage::new_error(&msg))
        }
    };
}

/// Macro to send an info message right now.
///
/// Takes the same arguments as `format!` if more than just a &str is supplied.
#[allow(unused)]
macro_rules! info_now {
    ($fmt:expr) => {
        crate::log::send_log_message_now(crate::log::LogMessage::new_info($fmt))
    };
    ($fmt:expr, $($arg:tt)*) => {
        {
            let msg = format!($fmt, $($arg)*);
            crate::log::send_log_message_now(crate::log::LogMessage::new_info(&msg))
        }
    };
}
/// Macro to send a warning message right now.
///
/// Takes the same arguments as `format!` if more than just a &str is supplied.
#[allow(unused)]
macro_rules! warning_now {
    ($fmt:expr) => {
        crate::log::send_log_message_now(crate::log::LogMessage::new_warning($fmt))
    };
    ($fmt:expr, $($arg:tt)*) => {
        {
            let msg = format!($fmt, $($arg)*);
            crate::log::send_log_message_now(crate::log::LogMessage::new_warning(&msg))
        }
    };
}
/// Macro to send an error message right now.
///
/// Takes the same arguments as `format!` if more than just a &str is supplied.
#[allow(unused)]
macro_rules! err_now {
    ($fmt:expr) => {
        crate::log::send_log_message_now(crate::log::LogMessage::new_error($fmt))
    };
    ($fmt:expr, $($arg:tt)*) => {
        {
            let msg = format!($fmt, $($arg)*);
            crate::log::send_log_message_now(crate::log::LogMessage::new_error(&msg))
        }
    };
}

#[allow(unused_imports)]
pub(crate) use {err, err_now, info, info_now, warning, warning_now};

#[cfg(test)]
mod tests {
    use super::*;

    /// Convince myself how Ordering in enums work.
    #[test]
    fn log_level_ordering() {
        let info = Level::Info;
        let warning = Level::Warning;
        let error = Level::Error;

        assert!(info == Level::Info);
        assert!(warning == Level::Warning);
        assert!(error == Level::Error);

        assert!(info < warning);
        assert!(warning < error);
        assert!(info < error);
    }
}
