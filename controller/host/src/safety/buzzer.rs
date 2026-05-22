//! Implement the buzzer and the static, lazy loading of the buzzer.

use std::{fs, sync::OnceLock};

use crate::log;

static BUZZER: OnceLock<Buzzer> = OnceLock::new();
const BUZZER_FNAME: &str = "/sys/class/leds/usr-buzzer/brightness";

/// Function to get the buzzer or intialize it if it is not there yet.
pub fn buzzer() -> &'static Buzzer {
    BUZZER.get_or_init(Buzzer::new)
}

/// The buzzer structure to toggle the system buzzer.
///
/// Safety:
/// Note: We can technically access the buzzer quasi-simultaneously. This should never really
/// happen as we only turn it on and then turn it off when the user interacts with it.
/// The worst thing that can happen: We overwrite a state right away.
#[derive(Debug)]
pub struct Buzzer {
    _private: (),
}

impl Buzzer {
    /// A non-public way to create a new Buzzer so we can only create it here!
    fn new() -> Self {
        Buzzer { _private: () }
    }
    /// Turn the buzzer on.
    pub fn turn_off(&self) {
        self.set_state(false);
    }

    /// Turn the buzzer off.
    pub fn turn_on(&self) {
        self.set_state(true);
    }

    /// Set the state of the buzzer by writen to the file.
    ///
    /// If an error in file handling occurs, an error is logged.
    fn set_state(&self, state: bool) {
        let out = match state {
            true => "1",
            false => "0",
        };

        if let Err(e) = fs::write(BUZZER_FNAME, out) {
            log::err_now!("Could not write to buzzer {}.", e);
        }
    }
}
