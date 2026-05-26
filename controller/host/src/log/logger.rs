//! The logger logic itself.

use std::{fmt::Display, fs::OpenOptions, io::Write, path::PathBuf};

use chrono::{DateTime, Local};
use slint::{ComponentHandle, Model};
use tokio::sync::mpsc;

use crate::{
    CONFIG_FOLDER, LOG_LEVEL_DISPLAY,
    app::{Logic, UI},
    log,
    rotation::rotate,
};

/// The severity level of a log message.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    // Informational message, the most verbose.
    Info,
    // Warning level: no error occurred but something is off.
    Warning,
    // An error occurred. An unwrap would have panicked here!
    Error,
}

impl Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Level::Info => write!(f, "INFO"),
            Level::Warning => write!(f, "WARN"),
            Level::Error => write!(f, "ERR "),
        }
    }
}

/// A log message.
#[derive(Debug, Clone)]
pub struct LogMessage {
    timestamp: DateTime<Local>,
    pub level: Level,
    message: String,
}

impl LogMessage {
    /// Create a new log message.
    fn new(level: Level, message: &str) -> Self {
        Self {
            timestamp: Local::now(),
            level,
            message: message.to_string(),
        }
    }

    /// Create a new info log message.
    pub fn new_info(message: &str) -> Self {
        Self::new(Level::Info, message)
    }

    /// Create a new warning log message.
    pub fn new_warning(message: &str) -> Self {
        Self::new(Level::Warning, message)
    }

    /// Create a new error log message.
    pub fn new_error(message: &str) -> Self {
        Self::new(Level::Error, message)
    }
}

impl Display for LogMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}: {}] {}",
            self.timestamp.format("%Y-%m-%d %H:%M:%S"),
            self.level,
            self.message
        )
    }
}

/// A handler for log messages.
pub struct LogHandler {
    /// The log message receiver.
    rx: mpsc::Receiver<LogMessage>,
    /// File name of log file, will be stored in CONFIG_FOLDER.
    log_file: PathBuf,
}

impl LogHandler {
    /// Create a new LogHandler.
    pub fn new(rx: mpsc::Receiver<LogMessage>) -> Self {
        let log_file = CONFIG_FOLDER
            .get()
            .expect("Config folder is initialized")
            .join(super::LOG_FNAME);

        Self { rx, log_file }
    }

    /// Add a new log message to the handler and process it.
    pub fn add_new_message(&mut self, msg: LogMessage) {
        // Save the message to the file.
        match OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.log_file)
        {
            Ok(mut fout) => {
                if writeln!(&mut fout, "{}", msg).is_err() {
                    eprintln!("Could not write to log file {:?}", self.log_file);
                }
            }
            Err(e) => {
                eprintln!(
                    "Could not open log file {:?} for writing: {}",
                    self.log_file, e
                );
            }
        }

        // Save message to buffer if required.
        if msg.level >= LOG_LEVEL_DISPLAY {
            self.update_ui(msg);
        }
    }

    pub async fn recv(&mut self) -> Option<LogMessage> {
        self.rx.recv().await
    }

    /// Rotate the log file.
    ///
    /// This is intentionally blocking in order to avoid writing to a log file while it is being
    /// rotated.
    pub fn rotate_log_file(&self) {
        if let Err(e) = rotate(super::LOG_FNAME) {
            log::warning_now!("Failed to rotate {} file: {}", super::LOG_FNAME, e);
        };
    }

    /// Update the UI, if it is already set. Otherwise just do nothing.
    pub fn update_ui(&self, msg: LogMessage) {
        if let Some(ui) = UI.get() {
            ui.upgrade_in_event_loop(move |ui| {
                let model = ui.global::<Logic>().get_log_messages();
                let model = model
                    .as_any()
                    .downcast_ref::<slint::VecModel<slint::SharedString>>()
                    .unwrap();
                model.insert(0, slint::SharedString::from(msg.to_string()));
            })
            .unwrap();
        }
    }
}
