//! //! Heater configuration new types.
//!
//! use instrumentrs::InstrumentError;
//! use measurements::Current;
//!
//! /// Heater setup configuration.
//! #[derive(Debug, Clone)]
//! pub struct HeaterConfig {
//!     /// Heater output channel.
//!     output: u8,
//!     /// Resistance to set for the heater output.
//!     resistance: HeaterResistance,
//!     /// Maximum current to set for this heater output.
//!     max_current: HeaterMaxCurrent,
//!     /// Display unit for this heater.
//!     display: HeaterDisplay,
//! }
//!
//! impl HeaterConfig {
//!     /// Try to create a new HeaterConfig type.
//!     ///
//!     /// Arguments:
//!     /// - `resistance`: Resistance to set for the heater output.
//!     /// - `max_current`: Maximum current to set for the heater output.
//!     /// - `display`: Display unit for this heater.
//!     ///
//!     /// Returns:
//!     /// - Error if user value for max current was out of range.
//!     pub fn try_new(
//!         output: u8,
//!         resistance: HeaterResistance,
//!         max_current: HeaterMaxCurrent,
//!         display: HeaterDisplay,
//!     ) -> Result<Self, InstrumentError> {
//!         if !(0..=1).contains(&output) {
//!             return Err(InstrumentError::IntValueOutOfRange {
//!                 value: output as i64,
//!                 min: 0,
//!                 max: 1,
//!             });
//!         }
//!         if let HeaterMaxCurrent::User(val) = max_current {
//!             let val_amps = val.as_amperes();
//!             if !(0.0..=2.0).contains(&val_amps) {
//!                 return Err(InstrumentError::FloatValueOutOfRange {
//!                     value: val_amps,
//!                     min: 0.0,
//!                     max: 2.0,
//!                 });
//!             }
//!         }
//!
//!         Ok(Self {
//!             output,
//!             resistance,
//!             max_current,
//!             display,
//!         })
//!     }
//!
//!     /// Format the heater config as part of the command.
//!     ///
//!     /// Returns:
//!     /// "n,n,+n.nnn,n" where the n's are as following:
//!     /// - Heater resistance.
//!     /// - Max current setting.
//!     /// - User defined max current (three sig figs) if previous is 0.
//!     /// - Display setting.
//!     fn to_cmd(&self) -> String {
//!         let user_current = match self.max_current {
//!             HeaterMaxCurrent::User(val) => val.as_amperes(),
//!             _ => 0.0,
//!         };
//!         format!(
//!             "{},{},{},+{:.3},{}",
//!             self.output,
//!             u8::from(self.resistance),
//!             u8::from(self.max_current),
//!             user_current,
//!
//!         )
//!     }
//!
//!     /// Parse the result and return a `HeaterConfig`.
//!     pub fn try_from_result(output: u8, res: &str) -> Result<Self, InstrumentError> {
//!         // FIXME: Continue here
//!     }
//! }
//!
//! /// Heater resistance.
//! ///
//! /// Options are 25 Ohm or 50 Ohm.
//! #[derive(Debug, Copy, Clone)]
//! pub enum HeaterResistance {
//!     /// Heater resistance of 25 Ohm.
//!     R25Ohm,
//!     /// Heater resistance of 50 Ohm.
//!     R50Ohm,
//! }
//!
//! impl From<HeaterResistance> for u8 {
//!     fn from(value: HeaterResistance) -> Self {
//!         match value {
//!             HeaterResistance::R25Ohm => 1,
//!             HeaterResistance::R50Ohm => 2,
//!         }
//!     }
//! }
//!
//! /// Maximum heater current.
//! ///
//! /// Max current will be limited to 1.414 A on output 2 if the heater resistance is set to 25 ),
//! /// and will be limited to 1 A on both outputs 1 and 2 if the heater resistance is set to 50 ).
//! #[derive(Debug, Copy, Clone)]
//! pub enum HeaterMaxCurrent {
//!     /// User defined heater current.
//!     User(Current),
//!     /// 0.707 A
//!     I0p707A,
//!     /// 1 A
//!     I1,
//!     /// 1.141 A
//!     I1p141,
//!     /// 2 A
//!     I2,
//! }
//!
//! impl From<HeaterMaxCurrent> for u8 {
//!     fn from(value: HeaterMaxCurrent) -> Self {
//!         match value {
//!             HeaterMaxCurrent::User(_) => 0,
//!             HeaterMaxCurrent::I0p707A => 1,
//!             HeaterMaxCurrent::I1 => 2,
//!             HeaterMaxCurrent::I1p141 => 3,
//!             HeaterMaxCurrent::I2 => 4,
//!         }
//!     }
//! }
//!
//! /// Heater display.
//! ///
//! /// Specifies whether the heater display shows output in current or power.
//! #[derive(Debug, Copy, Clone)]
//! pub enum HeaterDisplay {
//!     /// Show output current.
//!     Current,
//!     /// Show output power.
//!     Power,
//! }
//!
//! impl From<HeaterDisplay> for &str {
//!     fn from(value: HeaterDisplay) -> Self {
//!         match value {
//!             HeaterDisplay::Current => "1",
//!             HeaterDisplay::Power => "2",
//!         }
//!     }
//! }
