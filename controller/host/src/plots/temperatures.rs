//! Plot the temperature figure and accepts signals that send temperature data points.

use std::f64;

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
        Measurements, PLOT_STYLE, PlotSizePx, TIME_INTERVAL_CLEANUP, TIME_RANGE_TO_KEEP,
        TemperatureDataPoint,
    },
};

pub enum TemperaturePlotCommands {
    AddDataPoint(TemperatureDataPoint),
}

pub struct TemperaturePlot {
    measurements: Measurements,
    plot_size: PlotSizePx,
}

impl TemperaturePlot {
    /// Create a new and empty temperature plot.
    pub fn new(plot_size: PlotSizePx) -> Self {
        let measurements = Measurements::new_temperature();

        Self {
            measurements,
            plot_size,
        }
    }

    /// Make the plot for the temperature display with a logarithmic scenario and set it to the UI.
    fn plot_it(&self) -> Result<()> {
        let ui = match UI.get() {
            Some(ui) => ui,
            None => bail!("Cannot make temperature plot: UI not set."),
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
            let min_3 = view.series_3().fold(f64::INFINITY, f64::min);
            let max_3 = view.series_3().fold(f64::NEG_INFINITY, f64::max);
            (
                min_1.min(min_2).min(min_3) - 15.0,
                max_1.max(max_2).max(max_3) + 15.0,
            )
        };

        let mut chart = ChartBuilder::on(&root)
            .margin(10)
            .set_label_area_size(LabelAreaPosition::Left, 90)
            .set_label_area_size(LabelAreaPosition::Bottom, 60)
            .build_cartesian_2d(min_ts..max_ts, min_y..max_y)?;

        let xlbl = "Time";
        let ylbl = "Temperature (K)";

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
            .y_labels(3)
            .y_label_formatter(&|yval| format!("{:.0}", yval))
            .draw()?;

        // Draw the first series
        chart.draw_series(LineSeries::new(
            view.iter_series_1(),
            &PLOT_STYLE.sample_color,
        ))?;

        // Draw the second series
        chart.draw_series(LineSeries::new(
            view.iter_series_2(),
            &PLOT_STYLE.bridge_color,
        ))?;

        // Draw the third series
        chart.draw_series(LineSeries::new(
            view.iter_series_3(),
            &PLOT_STYLE.cooler_color,
        ))?;

        // Avoid IO failure being ignored silently by manually calling present function
        root.present()?;

        drop(chart);
        drop(root);

        ui.upgrade_in_event_loop(move |ui| {
            let img = slint::Image::from_rgb8(pixel_buffer);
            ui.global::<Logic>().set_temperature_plot(img);
        })
        .expect("UI still exists");

        Ok(())
    }

    /// Make the plot and set it to the UI.
    ///
    /// TODO: Analyze what here actually needs to be done everytime and what can be done at init
    pub fn make_plot(&mut self) {
        if let Err(e) = self.plot_it() {
            log::err_now!("Failed to make temperature plot: {}", e);
        }
    }
}

/// Pressure plot task: Receive temperature data points and update the plot and UI.
pub async fn temperature_plot_task(mut rx: mpsc::Receiver<TemperaturePlotCommands>) {
    let mut plot = TemperaturePlot::new(PlotSizePx {
        width: 800,
        height: 400,
    });

    let mut rx_shutdown = crate::HALT_SENDER.get().expect("Uninitialized").subscribe();

    let mut next_cleanup_time = Instant::now() + TIME_INTERVAL_CLEANUP;
    let mut next_hist_rotation = Instant::now() + crate::LOG_ROTATION_DURATION;

    loop {
        tokio::select! {
            Some(cmd) = rx.recv() => {
                match cmd {
                    TemperaturePlotCommands::AddDataPoint(dp) => {
                        // cleanup first?
                        if Instant::now() >= next_cleanup_time {
                            plot.measurements.retain(TIME_RANGE_TO_KEEP);
                            next_cleanup_time = Instant::now() + TIME_INTERVAL_CLEANUP;
                        }

                        plot.measurements.push_temperature(dp);
                        plot.make_plot();
                    }
                }
            }
            _ = sleep_until(next_hist_rotation) => {
                plot.measurements.rotate_history_file();
                crate::log::info!("Rotated {} file", super::HISTORY_TEMPERATURE_FNAME).await;
                next_hist_rotation = Instant::now() + crate::LOG_ROTATION_DURATION;
            }
            _ = rx_shutdown.recv() => {
                    break;
            }
        }
    }
}

/// Get the command sender for the temperature plot.
fn get_temperature_plot_command_sender() -> mpsc::Sender<TemperaturePlotCommands> {
    crate::plots::PLOT_TEMPERATURE_SENDER
        .get()
        .expect("Uninitialized")
        .clone()
}

/// Convenience function to send a temperature plot command without awaiting.
///
/// If an error occurs, this error is logged. Otherwise, the program will continue
/// as normal.
pub fn send_temperature_plot_cmd_now(cmd: TemperaturePlotCommands) {
    let sender = get_temperature_plot_command_sender();
    if let Err(e) = sender.try_send(cmd) {
        log::err_now!("Failed to send temperature plot command now: {}", e);
    }
}
