//! Module to handle saving and loading configuration files.

use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    time::Duration,
};

use anyhow::{Result, anyhow};
use measurements::{Pressure, Temperature};
use serde::{Deserialize, Serialize};

use crate::{
    ARCHIVE_FOLDER, CONFIG_FOLDER,
    controller::ControllerConfig,
    instruments::{
        cryocooler::CryoCoolerConfig, hi_cube::PfeifferHiCubeConf, ion_pump::IonPumpConfig,
        lakeshore_temp::LakeshoreTempConfig, omnicontrol::OmniControlConfig,
    },
    log,
    samples::{Sample, Samples},
};

pub const CONFIG_FNAME: &str = "cryostorage_config.ron";

#[derive(Debug, Serialize, Deserialize)]
pub struct PrgConfig {
    fname: PathBuf,
    admin_pin: String,
    authorizations: Authorizations,
    agilent_ion_pump: IonPumpConfig,
    controller_config: ControllerConfig,
    pfeiffer_hicube: PfeifferHiCubeConf,
    pfeiffer_omnicontrol: OmniControlConfig,
    samples: Samples,
    lakeshore_temperature: LakeshoreTempConfig,
    safety_warnings: SafetyWarnings,
    suntel_cryocooler: CryoCoolerConfig,
}

impl PrgConfig {
    /// Create a new PrgConfig instance with saved or default values.
    ///
    /// The configuration folder is created, if it does not exist, in `main.rs`.
    pub fn try_new() -> Result<Self> {
        let fname = CONFIG_FOLDER
            .get()
            .expect("Config folder is initialized")
            .join(CONFIG_FNAME);

        let mut ret_self = Self {
            fname,
            admin_pin: String::from("1234"),
            authorizations: Authorizations::default(),
            agilent_ion_pump: IonPumpConfig::default(),
            controller_config: ControllerConfig::default(),
            pfeiffer_hicube: PfeifferHiCubeConf::default(),
            pfeiffer_omnicontrol: OmniControlConfig::default(),
            samples: Samples::new(),
            lakeshore_temperature: LakeshoreTempConfig::default(),
            safety_warnings: SafetyWarnings::default(),
            suntel_cryocooler: CryoCoolerConfig::default(),
        };

        if let Err(e) = ret_self.load_from_file() {
            eprintln!("Error loading config file: {e}");
        };

        Ok(ret_self)
    }

    fn load_from_file(&mut self) -> Result<()> {
        if let Ok(content) = fs::read_to_string(&self.fname)
            && let Ok(cont_ron) = ron::de::from_str::<PrgConfig>(&content)
        {
            *self = cont_ron;
            Ok(())
        } else {
            self.save_to_file()?;
            Err(anyhow!(
                "Config file likely invalid or not found, saving a new one with default values."
            ))
        }
    }

    fn save_to_file(&self) -> Result<()> {
        // backup the previous config file with same name and timestamp if it exists
        if self.fname.exists() {
            let archive_folder = ARCHIVE_FOLDER.get().expect("Archive folder is initialized");

            let timestamp = chrono::Utc::now().format("%Y-%m-%d-%H:%M:%S");

            let stem = self
                .fname
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("cryostorage_config");
            let ext = self
                .fname
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("ron");

            let backup_fname = self
                .fname
                .parent()
                .expect("Config file has a parent folder")
                .join(archive_folder)
                .join(format!("{timestamp}_{stem}.{ext}"));

            if let Err(e) = fs::copy(&self.fname, backup_fname) {
                log::err_now!("Failed to backup config file: {}", e);
            };
        };

        let content = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())?;
        let mut f = File::options()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&self.fname)?;
        writeln!(&mut f, "{}", content)?;
        Ok(())
    }

    /// Get a clone of the admin pin.
    pub fn get_admin_pin(&self) -> String {
        self.admin_pin.clone()
    }

    /// Get a clone of the authorizations.
    pub fn get_authorizations(&self) -> Authorizations {
        self.authorizations.clone()
    }

    /// Get a clone of the controller configuration.
    pub fn get_controller_config(&self) -> ControllerConfig {
        self.controller_config.clone()
    }

    /// Get a clone of the cryocooler configuration.
    pub fn get_cryocooler_config(&self) -> CryoCoolerConfig {
        self.suntel_cryocooler.clone()
    }

    /// Get a clone of the ion pump configuration.
    pub fn get_ion_pump_config(&self) -> IonPumpConfig {
        self.agilent_ion_pump.clone()
    }

    /// Get the max safe sample temperature.
    pub fn get_max_safe_sample_temperature(&self) -> Temperature {
        self.safety_warnings.max_safe_sample_temperature
    }

    /// Get a clone of the Omnicontrol configuration.
    pub fn get_omnicontrol_config(&self) -> OmniControlConfig {
        self.pfeiffer_omnicontrol.clone()
    }

    /// Get a clone of the lakeshore temperature configuration.
    pub fn get_lakeshore_temp_config(&self) -> LakeshoreTempConfig {
        self.lakeshore_temperature.clone()
    }

    /// Get a clone of the HiCube configuration.
    pub fn get_pfeiffer_hicube_config(&self) -> PfeifferHiCubeConf {
        self.pfeiffer_hicube.clone()
    }

    /// Execute a swipe action on the sample
    pub fn execute_swipe_action(
        &mut self,
        pos1: &str,
        dx: f32,
        dy: f32,
    ) -> Option<[(String, Sample); 2]> {
        let res = self.samples.execute_swipe_swap(pos1, dx, dy);
        if self.save_to_file().is_err() {
            eprintln!("Couldn't save file. should not happen...");
        };
        res
    }

    /// Get a clone of the samples.
    pub fn get_samples(&self) -> Samples {
        self.samples.clone()
    }

    /// Check if the samples are empty.
    pub fn is_samples_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Update the samples, save to file, and return index of updated entry.
    pub fn update_sample(&mut self, pos: &str, value: &str) -> Result<Sample> {
        let res = self.samples.update_sample(pos, value);
        self.save_to_file()?;
        res
    }
}

/// As structure to provide certain authorizations and limits for the safe operation of the system.
///
/// These authorizations are used to ensure safe operations and to prevent damage to the system and the
/// chamber. They are checked against with current values to allow or disallow certain operations.
///
/// We add some `__doc_xxx` fields to the struct to be able to display some documentation on
/// certain variables. These are not used for the program and are solely for providing explanations
/// in the configuration file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Authorizations {
    pub baking: BakingAuthorization,
    pub cryo_cooler: CryoCoolerAuthorization,
    pub open_valve: OpenValveAuthorization,
    pub vent_chamber: VentChamberAuthorization,
    pub pump_chamber: PumpChamberAuthorization,
    pub equalize_pressures: EqualizePressureAuthorization,
}

impl Default for Authorizations {
    fn default() -> Self {
        Self {
            baking: BakingAuthorization::default(),
            cryo_cooler: CryoCoolerAuthorization::default(),
            open_valve: OpenValveAuthorization::default(),
            vent_chamber: VentChamberAuthorization::default(),
            pump_chamber: PumpChamberAuthorization::default(),
            equalize_pressures: EqualizePressureAuthorization::default(),
        }
    }
}

/// Authorization limits for baking the chamber.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BakingAuthorization {
    __doc_max_chamber_pressure_mbar: String,
    pub max_chamber_pressure_mbar: f64,
}

impl BakingAuthorization {
    pub fn default() -> Self {
        Self {
            __doc_max_chamber_pressure_mbar: String::from(
                "Maximum chamber pressure allowed to start baking.",
            ),
            max_chamber_pressure_mbar: 0.00001,
        }
    }
}

/// Authorization limits for the cryocooler.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CryoCoolerAuthorization {
    __doc_max_pressure_on: String,
    pub max_pressure_on_mbar: f64,
}

impl CryoCoolerAuthorization {
    pub fn default() -> Self {
        Self {
            __doc_max_pressure_on: String::from(
                "Authorization to turn on the cryocooler is given if the pressure in the chamber is below this limit.",
            ),
            max_pressure_on_mbar: 0.00001,
        }
    }
}

/// Authorization limits for opening valves.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenValveAuthorization {
    __doc_valve_ratio: String,
    pub valve_ratio_range: SafeRangeLimits,
    __doc_low_pressure_limit: String,
    pub low_pressure_limit_mbar: f64,
}

impl Default for OpenValveAuthorization {
    fn default() -> Self {
        Self {
            __doc_valve_ratio: String::from(
                "Authorization given if: lower_limit < Gauge1/Gauge2 < upper_limit.",
            ),
            valve_ratio_range: SafeRangeLimits {
                lower_limit: 0.001,
                upper_limit: 100.0,
            },
            __doc_low_pressure_limit: String::from(
                "Authorization give independent of range if both gauges show pressure below the low_pressure_limit.",
            ),
            low_pressure_limit_mbar: 0.00001,
        }
    }
}

/// Authorization limits for venting the chamber.
///
/// For timer numbering, see the flowchart in the docs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VentChamberAuthorization {
    __doc_timer_1: String,
    pub timer_1: Duration,
    __doc_timer_2: String,
    pub timer_2: Duration,
    __doc_timer_3: String,
    pub timer_3: Duration,
    __doc_sample_t_min: String,
    pub sample_t_min: Temperature,
}

impl Default for VentChamberAuthorization {
    fn default() -> Self {
        Self {
            __doc_timer_1: String::from(
                "Duration to pump transfer system if its pressure is too high compared to the chamber to open the valves.",
            ),
            timer_1: Duration::from_mins(20),
            __doc_timer_2: String::from(
                "Duration to vent transfer system if its pressure is too low compared to the chamber to open the valves.",
            ),
            timer_2: Duration::from_mins(15),
            __doc_timer_3: String::from(
                "Duration how long the vent valve should be open to vent the chamber.",
            ),
            timer_3: Duration::from_mins(25),
            __doc_sample_t_min: String::from(
                "Authorization is given when sample temperature is above this temperature",
            ),
            sample_t_min: Temperature::from_kelvin(280.),
        }
    }
}

/// Authorization limits for when pumping the chamber.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PumpChamberAuthorization {
    __doc_timer_primary_pump: String,
    pub timer_primary_pump: Duration,
    __doc_pressure_limit_primary_pump: String,
    pub pressure_limit_primary_pump: Pressure,
    __doc_timer_ion_pump: String,
    pub timer_ion_pump: Duration,
}

impl Default for PumpChamberAuthorization {
    fn default() -> Self {
        Self {
            __doc_timer_primary_pump: String::from(
                "Duration to wait for pressure to reach the all okay level.",
            ),
            timer_primary_pump: Duration::from_mins(40),
            __doc_pressure_limit_primary_pump: String::from(
                "Pressure that needs to be reached within `timer_primary_pump` time.",
            ),
            pressure_limit_primary_pump: Pressure::from_millibars(1.0e-5),
            __doc_timer_ion_pump: String::from(
                "Duration to wait after primary pump limit is reached before starting ion pump.",
            ),
            timer_ion_pump: Duration::from_hours(2),
        }
    }
}

/// Authorizations specific for the equalize pressure workflows
///
/// These authorizations are the ones that are specific to the workflows to equalize the pressures
/// in the transfer and main chamber that are necessary if the valve opening authority cannot be
/// granted.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EqualizePressureAuthorization {
    __doc_timer: String,
    pub timer: Duration,
}

impl Default for EqualizePressureAuthorization {
    fn default() -> Self {
        Self {
            __doc_timer: String::from(
                "Duration to pump/vent before pressure equalization is aborted.",
            ),
            timer: Duration::from_mins(20),
        }
    }
}

/// Limits for ensuring defining safe ranges.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SafeRangeLimits {
    pub lower_limit: f64,
    pub upper_limit: f64,
}

/// A structure to hold software implemented warning limits.
///
/// Depending on how the chamber is running, these warning limits might trigger a buzzer and warning
/// in the system when they are under or overstepped.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SafetyWarnings {
    pub max_safe_sample_temperature: Temperature,
}

impl Default for SafetyWarnings {
    fn default() -> Self {
        Self {
            max_safe_sample_temperature: Temperature::from_kelvin(130.),
        }
    }
}
