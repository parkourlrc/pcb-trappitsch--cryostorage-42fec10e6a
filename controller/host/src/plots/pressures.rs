//! Plot the pressure figure and accepts signals that send pressure data points.

use anyhow::{Result, anyhow, bail};
use plotters::prelude::*;
use slint::ComponentHandle;
use tokio::{
    sync::mpsc,
    time::{Instant, sleep_until},
};

use crate::{
    app::{Logic, UI},
    log,
    plots::{
        Measurements, PLOT_STYLE, PlotSizePx, PressureDataPoint, TIME_INTERVAL_CLEANUP,
        TIME_RANGE_TO_KEEP,
    },
};

pub enum PressurePlotCommands {
    AddDataPoint(PressureDataPoint),
}

pub struct PressurePlot {
    measurements: Measurements,
    plot_size: PlotSizePx,
}

impl PressurePlot {
    /// Create a new and empty pressure plot.
    pub fn new(plot_size: PlotSizePx) -> Self {
        let measurements = Measurements::new_pressure();

        Self {
            measurements,
            plot_size,
        }
    }

    /// Make the plot for the pressure display with a logarithmic scenario and set it to the UI.
    fn plot_it(&self) -> Result<()> {
        let ui = match UI.get() {
            Some(ui) => ui,
            None => bail!("Cannot make pressure plot: UI not set."),
        };

        let mut pixel_buffer =
            slint::SharedPixelBuffer::new(self.plot_size.width, self.plot_size.height);
        let size = (pixel_buffer.width(), pixel_buffer.height());

        let backend = BitMapBackend::with_buffer(pixel_buffer.make_mut_bytes(), size);
        let root = backend.into_drawing_area();
        root.fill(&PLOT_STYLE.bg_color)?;

        let view = self.measurements.last_timerange_view(TIME_RANGE_TO_KEEP);

        // Bounds for the view we want
        let (min_ts, max_ts) = (
            view.timestamps()
                .next()
                .ok_or_else(|| anyhow!("Not enough data yet to plot."))?,
            view.timestamps()
                .last()
                .ok_or_else(|| anyhow!("Not enough data yet to plot."))?,
        );
        let (min_y, max_y) = {
            let min_1 = view.series_1().fold(f64::INFINITY, f64::min);
            let max_1 = view.series_1().fold(f64::NEG_INFINITY, f64::max);
            let min_2 = view.series_2().fold(f64::INFINITY, f64::min);
            let max_2 = view.series_2().fold(f64::NEG_INFINITY, f64::max);
            let (min_exp, max_exp) = (
                min_1.min(min_2).log10().floor(),
                max_1.max(max_2).log10().ceil(),
            );
            (10_f64.powf(min_exp), 10_f64.powf(max_exp))
        };

        let mut chart = ChartBuilder::on(&root)
            .margin(10)
            .set_label_area_size(LabelAreaPosition::Left, 90)
            .set_label_area_size(LabelAreaPosition::Bottom, 60)
            .build_cartesian_2d(min_ts..max_ts, (min_y..max_y).log_scale())?;

        let xlbl = "Time";
        let ylbl = "Pressure (mbar)";

        chart
            .configure_mesh()
            .light_line_style(PLOT_STYLE.mesh_minor_color)
            .bold_line_style(PLOT_STYLE.mesh_major_color)
            .label_style(
                (PLOT_STYLE.font, 24)
                    .into_font()
                    .color(&PLOT_STYLE.fg_color),
            )
            .axis_style(PLOT_STYLE.fg_color)
            .x_desc(xlbl)
            .x_labels(6)
            .x_label_formatter(&|xval| xval.format("%H:%M").to_string())
            .max_light_lines(4)
            .y_desc(ylbl)
            .y_labels(5)
            .y_label_formatter(&|yval| format!("{:.0e}", yval))
            .draw()?;

        // Draw the chamber
        chart.draw_series(LineSeries::new(
            view.iter_series_1(),
            &PLOT_STYLE.chamber_color,
        ))?;

        // Draw the second series
        chart.draw_series(LineSeries::new(
            view.iter_series_2(),
            &PLOT_STYLE.transfer_color,
        ))?;

        // Avoid IO failure being ignored silently by manually calling present function
        root.present()?;

        drop(chart);
        drop(root);

        ui.upgrade_in_event_loop(move |ui| {
            let img = slint::Image::from_rgb8(pixel_buffer);
            ui.global::<Logic>().set_vacuum_plot(img);
        })
        .expect("UI still exists");

        Ok(())
    }

    /// Make the plot and set it to the UI.
    pub fn make_plot(&mut self) {
        if let Err(e) = self.plot_it() {
            log::err_now!("Failed to make pressure plot: {}", e);
        }
    }
}

/// Pressure plot task: Receive pressure data points and update the plot and UI.
pub async fn pressure_plot_task(mut rx: mpsc::Receiver<PressurePlotCommands>) {
    let mut plot = PressurePlot::new(PlotSizePx {
        width: 800,
        height: 400,
    });

    let mut rx_shutdown = crate::HALT_SENDER.get().expect("Uninitialized").subscribe();

    let mut next_plot_cleanup = Instant::now() + TIME_INTERVAL_CLEANUP;
    let mut next_hist_rotation = Instant::now() + crate::LOG_ROTATION_DURATION;

    loop {
        tokio::select! {
            Some(cmd) = rx.recv() => {
                match cmd {
                    PressurePlotCommands::AddDataPoint(dp) => {
                        // cleanup first?
                        if Instant::now() >= next_plot_cleanup {
                            plot.measurements.retain(TIME_RANGE_TO_KEEP);
                            next_plot_cleanup = Instant::now() + TIME_INTERVAL_CLEANUP;
                        }

                        plot.measurements.push_pressure(dp);
                        plot.make_plot();
                    }
                }
            }
            _ = sleep_until(next_hist_rotation) => {
                plot.measurements.rotate_history_file();
                crate::log::info!("Rotated {} file", super::HISTORY_PRESSURE_FNAME).await;
                next_hist_rotation = Instant::now() + crate::LOG_ROTATION_DURATION;
            }
            _ = rx_shutdown.recv() => {
                    break;
            }
        }
    }
}

/// Get the command sender for the pressure plot.
fn get_pressur_plot_command_sender() -> mpsc::Sender<PressurePlotCommands> {
    crate::plots::PLOT_PRESSURE_SENDER
        .get()
        .expect("Uninitialized")
        .clone()
}

/// Convenience function to send a pressure plot command without awaiting.
///
/// If an error occurs, this error is logged. Otherwise, the program will continue
/// as normal.
pub fn send_pressure_plot_cmd_now(cmd: PressurePlotCommands) {
    let sender = get_pressur_plot_command_sender();
    if let Err(e) = sender.try_send(cmd) {
        log::err_now!("Failed to send pressure plot command now: {}", e);
    }
}
