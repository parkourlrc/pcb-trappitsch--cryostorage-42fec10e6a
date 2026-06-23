use std::{
    env, fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::{OnceCell, broadcast, mpsc};

mod app;
mod connections;
mod controller;
mod dialog;
mod instruments;
mod log;
mod plots;
mod prg_config;
mod rotation;
mod safety;
mod samples;
mod status;
mod workflows;

use crate::{
    controller::start_controller_tasks,
    instruments::{hi_cube::pfeiffer_hicube_task, instruments_task},
    plots::{pressure_plot_task, temperature_plot_task},
    prg_config::PrgConfig,
    status::InstrumentStatus,
    workflows::{PROGRESS_WINDOW, WORKFLOW_COMMAND_SENDER, workflow_task},
};

pub static CONFIG_FOLDER: OnceCell<PathBuf> = OnceCell::const_new(); // config files, logs
pub static ARCHIVE_FOLDER: OnceCell<PathBuf> = OnceCell::const_new(); // archive of config files, logs
pub const LOG_ROTATION_DURATION: Duration = Duration::from_secs(7 * 24 * 3600); // Duration between log rot.

pub const LOG_LEVEL_DISPLAY: log::Level = log::Level::Warning;

pub static HALT_SENDER: OnceCell<broadcast::Sender<()>> = OnceCell::const_new();

pub static INST_STATUS: OnceCell<Arc<Mutex<InstrumentStatus>>> = OnceCell::const_new();
pub static PRG_CONF: OnceCell<Arc<Mutex<PrgConfig>>> = OnceCell::const_new();

#[tokio::main]
async fn main() {
    // first let's create the config and archive folders if they don't exist already.
    init_config_folders();

    // config
    let prg_conf = Arc::new(Mutex::new(prg_config::PrgConfig::try_new().unwrap()));
    PRG_CONF.set(Arc::clone(&prg_conf)).expect("Uninitialized");

    // status of instrument
    let inst_status = Arc::new(Mutex::new(InstrumentStatus::new()));
    INST_STATUS
        .set(Arc::clone(&inst_status))
        .expect("Unititialized");

    // Shutdown signal and receiver for tasks
    let (tx_halt, _) = broadcast::channel(1);
    HALT_SENDER.set(tx_halt.clone()).expect("Uninitialized");

    // LogHandler
    let (tx_log, rx_log) = mpsc::channel(128);
    let log_handler = log::LogHandler::new(rx_log);
    log::LOG_SENDER.set(tx_log).expect("Uninitialized");

    let log_handler_listen = tokio::spawn(log::log_handler_task(log_handler));

    // Initialize PROGRESS_WINDOW
    PROGRESS_WINDOW.set(Mutex::new(None)).ok();

    // Pressure plotting task
    let (tx_p_plot, rx_p_plot) = mpsc::channel(32);
    plots::PLOT_PRESSURE_SENDER
        .set(tx_p_plot.clone())
        .expect("Uninitialized");
    let p_plot_task = tokio::spawn(pressure_plot_task(rx_p_plot));

    // Temperature plotting task
    let (tx_t_plot, rx_t_plot) = mpsc::channel(32);
    plots::PLOT_TEMPERATURE_SENDER
        .set(tx_t_plot.clone())
        .expect("Uninitialized");
    let t_plot_task = tokio::spawn(temperature_plot_task(rx_t_plot));

    // comms for controller task
    let (tx_ctrl, rx_ctrl) = mpsc::channel(32);
    controller::CONTROLLER_COMMAND_SENDER
        .set(tx_ctrl.clone())
        .expect("Uninitialized");

    // controller
    let (cntrl_tsk, cntrl_bc_listen) = start_controller_tasks(rx_ctrl).await;

    // instruments monitoring task
    let (tx_instr, rx_instr) = mpsc::channel(32);
    instruments::INSTRUMENT_COMMAND_SENDER
        .set(tx_instr.clone())
        .expect("Uninitialized");
    let instr_tsk = tokio::spawn(instruments_task(rx_instr));

    // HiCube task
    let (tx_hicube, rx_hicube) = mpsc::channel(32);
    instruments::HICUBE_COMMAND_SENDER
        .set(tx_hicube.clone())
        .expect("Uninitialized");
    let hicube_task = tokio::spawn(pfeiffer_hicube_task(rx_hicube));

    // initialize the workflow channel and spawn the task.
    let (tx_wf, rx_wf) = mpsc::channel(32);
    WORKFLOW_COMMAND_SENDER
        .set(tx_wf.clone())
        .expect("Uninitialized");
    let _wf_taks = tokio::spawn(workflow_task(rx_wf));

    log::info!(
        "Started cryostorage_host: Build Info - {}",
        env!("BUILD_INFO")
    )
    .await;

    // start the app
    match app::app_main() {
        Ok(_) => {
            tx_halt.send(()).unwrap();
            let _ = tokio::join!(
                cntrl_tsk,
                cntrl_bc_listen,
                hicube_task,
                instr_tsk,
                log_handler_listen,
                p_plot_task,
                t_plot_task
            );
            println!("App exited normally")
        }
        Err(e) => eprintln!("App exited with error: {}", e),
    }
}

/// Set the config and archive folder consts, create the directories.
fn init_config_folders() {
    let conf_folder_pth = env::home_dir()
        .expect("Home directory must be known")
        .join(".cryostorage");
    fs::create_dir_all(&conf_folder_pth).expect("Could not create config folder");
    CONFIG_FOLDER
        .set(conf_folder_pth.clone())
        .expect("Uninitialized");

    let archive_folder_pth = conf_folder_pth.join("archive");
    fs::create_dir_all(&archive_folder_pth).expect("Could not create archive folder");
    ARCHIVE_FOLDER
        .set(archive_folder_pth)
        .expect("Uninitialized");
}

/// Wait for the program configuration to be available and then get it.
pub async fn get_inst_status_wait() -> Arc<Mutex<InstrumentStatus>> {
    loop {
        if let Some(ist) = INST_STATUS.get() {
            return Arc::clone(ist);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Wait for the program configuration to be available and then get it.
pub async fn get_prg_conf_wait() -> Arc<Mutex<PrgConfig>> {
    loop {
        if let Some(prg_conf) = PRG_CONF.get() {
            return Arc::clone(prg_conf);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
