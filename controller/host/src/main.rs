use std::{
    env, fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::{OnceCell, broadcast, mpsc, oneshot};

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
    status::InstrumentStatus,
};

pub static CONFIG_FOLDER: OnceCell<PathBuf> = OnceCell::const_new(); // config files, logs
pub static ARCHIVE_FOLDER: OnceCell<PathBuf> = OnceCell::const_new(); // archive of config files, logs
pub const LOG_ROTATION_DURATION: Duration = Duration::from_secs(7 * 24 * 3600); // Duration between log rot.

pub const LOG_LEVEL_DISPLAY: log::Level = log::Level::Warning;

pub static HALT_SENDER: OnceCell<broadcast::Sender<()>> = OnceCell::const_new();

#[tokio::main]
async fn main() {
    // first let's create the config and archive folders if they don't exist already.
    init_config_folders();

    // config
    let conf = Arc::new(Mutex::new(prg_config::PrgConfig::try_new().unwrap()));

    // status of instrument
    let inst_status = Arc::new(Mutex::new(InstrumentStatus::new()));

    // Shutdown signal and receiver for tasks
    let (tx_halt, _) = broadcast::channel(1);
    HALT_SENDER.set(tx_halt.clone()).expect("Uninitialized");

    // LogHandler
    let (tx_log, rx_log) = mpsc::channel(128);
    let (tx_ui_set, rx_ui_set) = oneshot::channel();
    let log_handler = log::LogHandler::new(rx_log);
    log::LOG_SENDER.set(tx_log).expect("Uninitialized");

    let log_handler_listen = tokio::spawn(log::log_handler_task(log_handler, rx_ui_set));

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
    let controller_config = conf
        .lock()
        .expect("Locking config must work")
        .get_controller_config();
    let (cntrl_tsk, cntrl_bc_listen) =
        start_controller_tasks(controller_config, Arc::clone(&inst_status), rx_ctrl).await;

    // instruments monitoring task
    let (tx_instr, rx_instr) = mpsc::channel(32);
    instruments::INSTRUMENT_COMMAND_SENDER
        .set(tx_instr.clone())
        .expect("Uninitialized");
    let instr_tsk = tokio::spawn(instruments_task(
        Arc::clone(&conf),
        Arc::clone(&inst_status),
        rx_instr,
    ));

    // HiCube task
    let (tx_hicube, rx_hicube) = mpsc::channel(32);
    instruments::HICUBE_COMMAND_SENDER
        .set(tx_hicube.clone())
        .expect("Uninitialized");
    let hicube_conf = conf
        .lock()
        .expect("Locking config must work")
        .get_pfeiffer_hicube_config();
    let hicube_task = tokio::spawn(pfeiffer_hicube_task(
        hicube_conf,
        Arc::clone(&inst_status),
        rx_hicube,
    ));

    log::info!(
        "Started cryostorage_host: Build Info - {}",
        env!("BUILD_INFO")
    )
    .await;

    // start the app
    match app::app_main(Arc::clone(&conf), Arc::clone(&inst_status), tx_ui_set) {
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
