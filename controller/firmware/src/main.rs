#![no_std]
#![no_main]

use app::AppTx;
use defmt::info;
use embassy_executor::Spawner;
use embassy_rp::{
    bind_interrupts,
    block::ImageDef,
    gpio::{Input, Level, Output, Pull},
    peripherals::USB,
    usb,
    watchdog::Watchdog,
};
use embassy_time::{Duration, Instant, Ticker};
use embassy_usb::{Config, UsbDevice};
use postcard_rpc::{
    sender_fmt,
    server::{Dispatch, Sender, Server},
};
use static_cell::StaticCell;

bind_interrupts!(pub struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
});

use crate::{
    baking::{Baking, baking_task},
    broadcaster::broadcaster_task,
    flow_meter::FlowMeter,
    valve::{Valve, ValveSelector, valve_task},
    vct::{Vct, vct_task},
};

use {defmt_rtt as _, panic_probe as _};

// postcard-rpc / poststation
mod app;
mod handlers;

// hardware
mod baking;
mod broadcaster;
mod flow_meter;
mod valve;
mod vct;

#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: ImageDef = ImageDef::secure_exe();

// Program metadata for `picotool info`.
// This is needed if you are using picotool to flash the device
#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"RP2350 template"),
    embassy_rp::binary_info::rp_program_description!(c"An example template for the RP2350"),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

fn usb_config(serial: &'static str) -> Config<'static> {
    let mut config = Config::new(0x16c0, 0x27DD);
    config.manufacturer = Some("Reto Trappitsch, EPFL");
    config.product = Some("Cryostorage Controller");
    info!("USB Serial: {}", serial);
    config.serial_number = Some(serial);

    // Required for windows compatibility.
    // https://developer.nordicsemi.com/nRF_Connect_SDK/doc/1.9.1/kconfig/CONFIG_CDC_ACM_IAD.html#help
    config.device_class = 0xEF;
    config.device_sub_class = 0x02;
    config.device_protocol = 0x01;
    config.composite_with_iads = true;

    config
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // SYSTEM INIT
    info!("Start");
    let p = embassy_rp::init(Default::default());

    let unique_id: u64 = embassy_rp::otp::get_chipid().unwrap();
    static SERIAL_STRING: StaticCell<[u8; 16]> = StaticCell::new();
    let mut ser_buf = [b' '; 16];
    // This is a simple number-to-hex formatting
    unique_id
        .to_be_bytes()
        .iter()
        .zip(ser_buf.chunks_exact_mut(2))
        .for_each(|(b, chs)| {
            let mut b = *b;
            for c in chs {
                *c = match b >> 4 {
                    v @ 0..10 => b'0' + v,
                    v @ 10..16 => b'A' + (v - 10),
                    _ => b'X',
                };
                b <<= 4;
            }
        });
    let ser_buf = SERIAL_STRING.init(ser_buf);
    let ser_buf = core::str::from_utf8(ser_buf.as_slice()).unwrap();

    // USB/RPC INIT
    let driver = usb::Driver::new(p.USB, Irqs);
    let pbufs = app::PBUFS.take();
    let config = usb_config(ser_buf);

    // Define outputs that go straight into context
    let light_output = Output::new(p.PIN_19, Level::Low);

    let context = app::Context {
        unique_id,
        light_output,
    };

    let (device, tx_impl, rx_impl) =
        app::STORAGE.init_poststation(driver, config, pbufs.tx_buf.as_mut_slice());
    let dispatcher = app::MyApp::new(context, spawner.into());
    let vkk = dispatcher.min_key_len();
    let mut server: app::AppServer = Server::new(
        tx_impl,
        rx_impl,
        pbufs.rx_buf.as_mut_slice(),
        dispatcher,
        vkk,
    );
    let sender = server.sender();

    // Watchdog
    //
    // - We save the residual baking time in scratch index 0
    let mut watchdog = Watchdog::new(p.WATCHDOG);
    watchdog.start(Duration::from_millis(icd::BROADCAST_INTERVAL_MS * 2));

    // Define my hardware here
    let residual_baking_time = watchdog.get_scratch(0);
    let baking = Baking::new(
        Output::new(p.PIN_20, Level::Low),
        residual_baking_time as u64,
    );
    let flow_meter = FlowMeter::new(Input::new(p.PIN_15, Pull::None));
    let pump_valve = Valve::new(
        Output::new(p.PIN_4, Level::Low), // Control option
        Output::new(p.PIN_5, Level::Low), // Control close
        Input::new(p.PIN_8, Pull::None),  // State open
        Input::new(p.PIN_9, Pull::None),  // State closed
    );
    let transfer_valve = Valve::new(
        Output::new(p.PIN_2, Level::Low), // Control option
        Output::new(p.PIN_3, Level::Low), // Control close
        Input::new(p.PIN_6, Pull::None),  // State open
        Input::new(p.PIN_7, Pull::None),  // State closed
    );
    let vct = Vct::new(
        Output::new(p.PIN_17, Level::Low), // Init NotReady
        Input::new(p.PIN_13, Pull::None),
        Input::new(p.PIN_16, Pull::None),
    );

    // We need to spawn the USB task so that USB messages are handled by
    // embassy-usb
    spawner.must_spawn(usb_task(device));
    spawner.must_spawn(logging_task(sender.clone()));

    // Spawn my tasks
    spawner.must_spawn(baking_task(baking));
    spawner.must_spawn(valve_task(pump_valve, ValveSelector::Pump));
    spawner.must_spawn(valve_task(transfer_valve, ValveSelector::Transfer));
    spawner.must_spawn(vct_task(vct));

    // Broadcaster
    spawner.must_spawn(broadcaster_task(sender, watchdog, flow_meter));

    // Begin running!
    loop {
        // If the host disconnects, we'll return an error here.
        // If this happens, just wait until the host reconnects
        let _ = server.run().await;
    }
}

/// This handles the low level USB management
#[embassy_executor::task]
pub async fn usb_task(mut usb: UsbDevice<'static, app::AppDriver>) {
    usb.run().await;
}

/// This task is a "sign of life" logger
#[embassy_executor::task]
pub async fn logging_task(sender: Sender<AppTx>) {
    let mut ticker = Ticker::every(Duration::from_secs(3));
    let start = Instant::now();
    loop {
        ticker.next().await;
        let _ = sender_fmt!(sender, "Uptime: {:?}", start.elapsed()).await;
    }
}
