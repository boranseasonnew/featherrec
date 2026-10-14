#![windows_subsystem = "windows"]

use eframe::egui::{self, Color32, RichText};
use recorder_core::{EncoderPolicy, Metrics, Preset, RecordingReport, Settings};
use recording_windows::{HardwareInfo, SharedMetrics};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const ACCENT: Color32 = Color32::from_rgb(226, 74, 87);

fn portable_root() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

struct Job {
    stop: SyncSender<()>,
    result: Receiver<Result<RecordingReport, String>>,
    worker: Option<JoinHandle<()>>,
}

impl Drop for Job {
    fn drop(&mut self) {
        let _ = self.stop.try_send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct FeatherRec {
    settings: Settings,
    hardware: Option<HardwareInfo>,
    inspection: Receiver<Result<HardwareInfo, String>>,
    metrics: SharedMetrics,
    job: Option<Job>,
    status: String,
    stopping: bool,
    closing: bool,
    last_file: Option<PathBuf>,
    settings_page: bool,
    screenshot: Option<PathBuf>,
    smoke_seconds: Option<u64>,
    smoke_started: Option<Instant>,
}

impl FeatherRec {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = cc.egui_ctx.clone();
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(15, 16, 19);
        visuals.window_fill = Color32::from_rgb(25, 27, 32);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(34, 37, 43);
        visuals.selection.bg_fill = Color32::from_rgb(99, 47, 56);
        visuals.override_text_color = Some(Color32::from_rgb(225, 227, 233));
        ctx.set_visuals(visuals);
        ctx.style_mut_of(egui::Theme::Dark, |style| {
            style.spacing.item_spacing = egui::vec2(12.0, 12.0);
            style.spacing.button_padding = egui::vec2(14.0, 9.0);
        });
        let (tx, inspection) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let _ = tx.send(recording_windows::inspect().map_err(|e| format!("{e:#}")));
            ctx.request_repaint();
        });
        let config = portable_root().join("featherrec-settings.json");
        let (settings, status) = match std::fs::read(&config) {
            Ok(bytes) => match serde_json::from_slice::<Settings>(&bytes) {
                Ok(settings) if settings.validate().is_ok() => {
                    (settings, "Checking displays and encoders…".into())
                }
                _ => (
                    Settings {
                        output_dir: portable_root().join("recordings"),
                        ..Settings::default()
                    },
                    "Saved settings are invalid; defaults restored.".into(),
                ),
            },
            Err(_) => (
                Settings {
                    output_dir: portable_root().join("recordings"),
                    ..Settings::default()
                },
                "Checking displays and encoders…".into(),
            ),
        };
        let args: Vec<String> = std::env::args().collect();
        let screenshot = args
            .windows(2)
            .find(|p| p[0] == "--screenshot")
            .map(|p| PathBuf::from(&p[1]));
        let smoke_seconds = args
            .windows(2)
            .find(|p| p[0] == "--smoke-test")
            .and_then(|p| p[1].parse::<u64>().ok())
            .map(|s| s.clamp(1, 120));
        Self {
            settings,
            hardware: None,
            inspection,
            metrics: Arc::new(Mutex::new(Metrics::default())),
            job: None,
            status,
            stopping: false,
            closing: false,
            last_file: None,
            settings_page: false,
            screenshot,
            smoke_seconds,
            smoke_started: None,
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        if let Err(error) = self.settings.validate() {
            self.status = error.to_string();
            return;
        }
        self.metrics = Arc::new(Mutex::new(Metrics::default()));
        let metrics = self.metrics.clone();
        let settings = self.settings.clone();
        let ctx = ctx.clone();
        let (stop, commands) = mpsc::sync_channel(1);
        let (result_sender, result) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let result = (|| {
                let mut recording = recording_windows::start(settings, metrics)?;
                ctx.request_repaint();
                let mut last_repaint = Instant::now();
                loop {
                    match commands.recv_timeout(Duration::from_millis(250)) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    recording.poll();
                    if recording.is_finished() {
                        break;
                    }
                    if last_repaint.elapsed() >= Duration::from_secs(1) {
                        ctx.request_repaint();
                        last_repaint = Instant::now();
                    }
                }
                recording.finish()
            })()
            .map_err(|e: anyhow::Error| format!("{e:#}"));
            let _ = result_sender.send(result);
            ctx.request_repaint();
        });
        self.job = Some(Job {
            stop,
            result,
            worker: Some(worker),
        });
        self.status = "Starting capture and checking the encoder…".into();
        self.stopping = false;
    }

    fn stop(&mut self) {
        if let Some(job) = &self.job {
            let _ = job.stop.try_send(());
            self.stopping = true;
            self.status = "Finalizing and verifying the MP4…".into();
        }
    }

    fn open_folder(&mut self) {
        let folder = self
            .last_file
            .as_ref()
            .and_then(|p| p.parent())
            .unwrap_or(&self.settings.output_dir);
        if let Err(error) = std::fs::create_dir_all(folder).and_then(|_| {
            std::process::Command::new("explorer.exe")
                .arg(folder)
                .spawn()
                .map(|_| ())
        }) {
            self.status = format!("Could not open folder: {error}");
        }
    }
}

impl eframe::App for FeatherRec {
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let screenshots: Vec<_> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
                .collect()
        });
        if let Some(image) = screenshots.first()
            && let Some(path) = self.screenshot.take()
        {
            let pixels: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Err(error) = image::save_buffer(
                path,
                &pixels,
                image.width() as u32,
                image.height() as u32,
                image::ColorType::Rgba8,
            ) {
                self.status = format!("Could not save UI screenshot: {error}");
            }
            if self.smoke_seconds.is_none() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if let Ok(result) = self.inspection.try_recv() {
            match result {
                Ok(info) => {
                    self.status = if info.displays.is_empty() {
                        "No connected display found.".into()
                    } else {
                        "Ready. Your recordings stay on this computer.".into()
                    };
                    self.hardware = Some(info);
                }
                Err(error) => self.status = error,
            }
        }
        let completed = self.job.as_ref().and_then(|j| match j.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("Recording worker exited unexpectedly.".into()))
            }
            Err(mpsc::TryRecvError::Empty) => None,
        });
        if let Some(result) = completed {
            self.job.take();
            self.stopping = false;
            let successful = result.as_ref().is_ok_and(|report| report.finalized);
            match result {
                Ok(report) => {
                    *self.metrics.lock().unwrap() = report.metrics.clone();
                    self.status = if report.finalized {
                        format!(
                            "Saved and verified: {}",
                            report
                                .file
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                        )
                    } else {
                        format!("Recording failed: {}", report.validation)
                    };
                    self.last_file = Some(report.file);
                }
                Err(error) => self.status = error,
            }
            if self.closing && successful {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if self.closing {
                // Keep the error visible if closing could not produce a valid MP4.
                self.closing = false;
            }
            if self.smoke_seconds.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if self.hardware.is_some() {
            if self.smoke_seconds.is_some() && self.smoke_started.is_none() {
                self.settings.output_dir = portable_root().join("validation");
                self.smoke_started = Some(Instant::now());
                self.start(ctx);
            }
            if let (Some(seconds), Some(started)) = (self.smoke_seconds, self.smoke_started)
                && started.elapsed() >= Duration::from_secs(seconds)
                && !self.stopping
            {
                // Exercise the same close-during-recording path used by the title bar.
                self.closing = true;
                self.stop();
            }
            if self.screenshot.is_some() && screenshots.is_empty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.job.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.closing = true;
            self.stop();
        }
        if self.job.is_some() {
            ctx.request_repaint_after(Duration::from_secs(1));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().fill(Color32::from_rgb(15, 16, 19)).inner_margin(28).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(36.0, 40.0), egui::Sense::hover());
                let origin = rect.left_top();
                let points = [[6.0, 32.0], [6.0, 23.0], [11.0, 13.0], [20.0, 6.0], [34.0, 5.0], [32.0, 19.0], [24.0, 29.0], [14.0, 33.0]].map(|[x,y]| origin + egui::vec2(x,y)).to_vec();
                ui.painter().add(egui::Shape::convex_polygon(points, ACCENT, egui::Stroke::NONE));
                ui.painter().line_segment([origin + egui::vec2(4.0, 37.0), origin + egui::vec2(26.0, 13.0)], egui::Stroke::new(2.0, Color32::WHITE));
                ui.vertical(|ui| { ui.heading("FeatherRec"); ui.label(RichText::new("A little lighter. A lot more focused.").color(Color32::GRAY)); });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { if ui.selectable_label(self.settings_page, "Settings").clicked() { self.settings_page = !self.settings_page; } });
            });
            ui.add_space(16.0);
            egui::Frame::new().fill(Color32::from_rgb(24, 26, 31)).corner_radius(12).inner_margin(20).show(ui, |ui| {
                let m = self.metrics.lock().unwrap().clone();
                ui.horizontal(|ui| {
                    let label = if self.stopping { "FINALIZING" } else if self.job.is_some() && !m.encoder_name.is_empty() { "RECORDING" } else { "DISPLAY RECORDING" };
                    let (indicator, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
                    ui.painter().circle_filled(indicator.center(), 4.0, if self.job.is_some() { ACCENT } else { Color32::GRAY });
                    ui.strong(label);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let seconds = m.elapsed_seconds as u64;
                        ui.label(RichText::new(format!("{:02}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60)).monospace().size(28.0));
                    });
                });
                ui.add_space(8.0);
                ui.label(&self.status);
                if self.job.is_some() || m.submitted > 0 {
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("{} frames submitted", m.submitted));
                        ui.label(format!("{:.1} submitted FPS", m.submitted as f64 / m.elapsed_seconds.max(0.001)));
                        ui.label(format!("{} dropped", m.dropped));
                        ui.label(format!("{:.0} MB RAM", m.working_set_bytes as f64 / 1_048_576.0));
                        ui.label(format!("{:.1}% CPU", m.process_cpu_percent));
                    });
                    if let Some(warning) = &m.warning { ui.colored_label(Color32::from_rgb(232, 183, 100), warning); }
                    if let Some(error) = &m.error { ui.colored_label(ACCENT, error); }
                    ui.small(&m.encoder_name);
                    if self.job.is_some() && ui.add_enabled(!self.stopping, egui::Button::new("Stop recording").fill(ACCENT)).clicked() { self.stop(); }
                }
            });
            ui.add_space(12.0);
            ui.add_enabled_ui(self.job.is_none(), |ui| {
                if self.settings_page {
                    ui.heading("Preferences & diagnostics");
                    ui.checkbox(&mut self.settings.cursor, "Include mouse cursor");
                    ui.label("No preview, analytics, accounts, or network connection required.");
                    if let Some(info) = &self.hardware {
                        ui.label(format!("{} logical processors", info.logical_processors));
                        ui.label("Registered hardware H.264 encoders:");
                        for name in &info.h264_hardware_encoders { ui.small(name); }
                        if info.h264_hardware_encoders.is_empty() { ui.colored_label(ACCENT, "No hardware H.264 encoder registered. Software may use considerably more CPU."); }
                    }
                    if ui.button("Save preferences").clicked() {
                        self.status = match serde_json::to_vec_pretty(&self.settings).map_err(std::io::Error::other).and_then(|bytes| std::fs::write(portable_root().join("featherrec-settings.json"), bytes)) {
                            Ok(()) => "Preferences saved beside the portable application.".into(),
                            Err(error) => format!("Could not save preferences: {error}"),
                        };
                    }
                    if ui.button("Back to recorder").clicked() { self.settings_page = false; }
                } else {
                    ui.label(RichText::new("RECORDING PRESET").small().color(Color32::GRAY));
                    ui.horizontal(|ui| {
                        for (preset, name) in [(Preset::Potato, "Potato"), (Preset::Balanced, "Balanced"), (Preset::Quality, "Quality")] {
                            if ui.selectable_label(self.settings.preset == preset, name).clicked() { self.settings.apply_preset(preset); }
                        }
                    });
                    egui::Grid::new("recording-settings").num_columns(2).spacing([24.0, 14.0]).show(ui, |ui| {
                        ui.label("Capture source"); ui.label("Full monitor · Windows Graphics Capture"); ui.end_row();
                        ui.label("Monitor");
                        egui::ComboBox::from_id_salt("monitor").width(330.0).selected_text(self.hardware.as_ref().and_then(|h| h.displays.iter().find(|d| d.index == self.settings.monitor)).map_or_else(|| "Select a display".into(), |d| format!("{} · {}×{} · {} Hz", d.name, d.width, d.height, d.refresh_hz))).show_ui(ui, |ui| {
                            if let Some(info) = &self.hardware { for d in &info.displays { ui.selectable_value(&mut self.settings.monitor, d.index, format!("{} · {}×{}", d.name, d.width, d.height)); } }
                        }); ui.end_row();
                        ui.label("Resolution / FPS");
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("resolution").selected_text(format!("{}p", self.settings.height)).show_ui(ui, |ui| {
                                for (w, h) in [(1280, 720), (1920, 1080)] {
                                    if ui.selectable_label(self.settings.height == h, format!("{h}p")).clicked() { (self.settings.width, self.settings.height) = (w, h); }
                                }
                            });
                            egui::ComboBox::from_id_salt("fps").selected_text(format!("{} FPS", self.settings.fps)).show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.settings.fps, 30, "30 FPS"); ui.selectable_value(&mut self.settings.fps, 60, "60 FPS");
                            });
                        }); ui.end_row();
                        ui.label("Encoder");
                        egui::ComboBox::from_id_salt("encoder").selected_text(match self.settings.encoder { EncoderPolicy::Hardware => "Verified hardware H.264", EncoderPolicy::AllowSoftware => "Allow software (higher CPU)" }).show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.settings.encoder, EncoderPolicy::Hardware, "Verified hardware H.264");
                            ui.selectable_value(&mut self.settings.encoder, EncoderPolicy::AllowSoftware, "Allow software (higher CPU)");
                        }); ui.end_row();
                        ui.label("Bitrate"); ui.add(egui::Slider::new(&mut self.settings.bitrate, 1_000_000..=50_000_000).step_by(500_000.0).custom_formatter(|v, _| format!("{:.1} Mbps", v / 1_000_000.0))); ui.end_row();
                        ui.label("Audio"); ui.horizontal(|ui| {
                            let mut off = false;
                            ui.add_enabled(false, egui::Checkbox::new(&mut off, "System"));
                            ui.add_enabled(false, egui::Checkbox::new(&mut off, "Microphone"));
                            ui.small("Video-only MVP");
                        }); ui.end_row();
                        ui.label("Output folder");
                        ui.horizontal(|ui| {
                            ui.label(self.settings.output_dir.file_name().unwrap_or(self.settings.output_dir.as_os_str()).to_string_lossy()).on_hover_text(self.settings.output_dir.display().to_string());
                            if ui.button("Choose…").clicked() && let Some(folder) = rfd::FileDialog::new().pick_folder() { self.settings.output_dir = folder; }
                        }); ui.end_row();
                    });
                    ui.small("30 / 60 FPS is a target ceiling. Actual delivery depends on content and hardware.");
                }
            });
            ui.add_space(16.0);
            ui.separator();
            ui.horizontal(|ui| {
                if self.job.is_some() {
                    if ui.add_enabled(!self.stopping, egui::Button::new(RichText::new("Stop recording").size(16.0)).fill(ACCENT)).clicked() { self.stop(); }
                } else if ui.add_enabled(self.hardware.as_ref().is_some_and(|h| h.displays.iter().any(|d| d.index == self.settings.monitor)), egui::Button::new(RichText::new("Start recording").size(16.0)).fill(ACCENT)).clicked() { self.start(&ctx); }
                if ui.button("Open recordings").clicked() { self.open_folder(); }
            });
            ui.add_space(8.0);
            ui.small("Local by design. No preview. No distractions.   ·   v0.1.0 experimental");
        });
        });
    }
}

fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([760.0, 760.0])
            .with_min_inner_size([700.0, 720.0])
            .with_title("FeatherRec"),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    if let Err(error) = eframe::run_native(
        "FeatherRec",
        options,
        Box::new(|cc| Ok(Box::new(FeatherRec::new(cc)))),
    ) {
        let _ = rfd::MessageDialog::new()
            .set_title("FeatherRec could not start")
            .set_description(error.to_string())
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
}
