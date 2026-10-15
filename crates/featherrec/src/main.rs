#![windows_subsystem = "windows"]

mod hotkeys;
mod preferences;
mod storage;
mod ui;

use eframe::egui;
use preferences::{HotkeyBindings, Preferences};
use recorder_core::{Metrics, RecordingReport, Settings};
use recording_windows::{AudioControls, HardwareInfo, SharedMetrics};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

fn portable_root() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

struct Job {
    stop: SyncSender<()>,
    result: Receiver<Result<Option<RecordingReport>, String>>,
    worker: Option<JoinHandle<()>>,
    phase: Arc<AtomicU8>,
    countdown_until: Instant,
    audio: Arc<AudioControls>,
}
impl Drop for Job {
    fn drop(&mut self) {
        let _ = self.stop.try_send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Recorder,
    Settings,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Recording,
    Audio,
    Shortcuts,
    Advanced,
}
struct PendingBindings {
    result: Receiver<Result<(), String>>,
    bindings: HotkeyBindings,
    save: Option<Preferences>,
}

struct FeatherRec {
    prefs: Preferences,
    saved: Preferences,
    settings_file: PathBuf,
    hardware: Option<HardwareInfo>,
    inspection: Receiver<Result<HardwareInfo, String>>,
    metrics: SharedMetrics,
    audio_preview: Option<recording_windows::AudioPreview>,
    preview_settings: Option<Settings>,
    job: Option<Job>,
    status: String,
    notice: Option<(String, bool)>,
    stopping: bool,
    closing: bool,
    last_file: Option<PathBuf>,
    page: Page,
    settings_tab: SettingsTab,
    advanced_unlocked: bool,
    window_filter: String,
    hotkeys: Option<hotkeys::Hotkeys>,
    active_bindings: HotkeyBindings,
    pending_bindings: Option<PendingBindings>,
    shortcut_status: String,
    listening: Option<usize>,
    listener_ready: Option<Receiver<Result<(), String>>>,
    screenshot: Option<PathBuf>,
    smoke_seconds: Option<u64>,
    smoke_started: Option<Instant>,
}

impl FeatherRec {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        ui::install_theme(&cc.egui_ctx);
        let args: Vec<String> = std::env::args().collect();
        let mut notice = None;
        let settings_file = args
            .windows(2)
            .find(|p| p[0] == "--settings-file")
            .map(|p| PathBuf::from(&p[1]))
            .unwrap_or_else(|| {
                storage::settings_path()
                    .unwrap_or_else(|_| portable_root().join("featherrec-settings.json"))
            });
        let default = || {
            Preferences::new(Settings {
                output_dir: std::env::var_os("USERPROFILE")
                    .map(PathBuf::from)
                    .unwrap_or_else(portable_root)
                    .join("Videos/FeatherRec"),
                ..Settings::default()
            })
        };
        let prefs = match storage::load(
            &settings_file,
            &portable_root().join("featherrec-settings.json"),
        ) {
            Ok(Some(prefs)) => prefs,
            Ok(None) => default(),
            Err(error) => {
                notice = Some((
                    format!(
                        "Ayarlar okunamadı: {error:#}. Güvenli varsayılanlar yüklendi; eski dosya korundu."
                    ),
                    true,
                ));
                default()
            }
        };
        let (tx, inspection) = mpsc::sync_channel(1);
        let ctx = cc.egui_ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(recording_windows::inspect().map_err(|e| format!("{e:#}")));
            ctx.request_repaint();
        });
        let hotkeys = match hotkeys::Hotkeys::new(cc.egui_ctx.clone()) {
            Ok(service) => Some(service),
            Err(error) => {
                notice = Some((format!("Genel kısayollar başlatılamadı: {error:#}"), true));
                None
            }
        };
        let screenshot = args
            .windows(2)
            .find(|p| p[0] == "--screenshot")
            .map(|p| PathBuf::from(&p[1]));
        let smoke_seconds = args
            .windows(2)
            .find(|p| p[0] == "--smoke-test")
            .and_then(|p| p[1].parse::<u64>().ok())
            .map(|s| s.clamp(1, 120));
        let settings_tab = match args
            .windows(2)
            .find(|p| p[0] == "--settings-tab")
            .map(|p| p[1].as_str())
        {
            Some("audio") => SettingsTab::Audio,
            Some("shortcuts") => SettingsTab::Shortcuts,
            Some("advanced") => SettingsTab::Advanced,
            _ => SettingsTab::General,
        };
        let settings_page = args.iter().any(|p| p == "--settings-tab");
        let mut app = Self {
            saved: prefs.clone(),
            settings_file,
            prefs,
            hardware: None,
            inspection,
            metrics: Arc::new(Mutex::new(Metrics::default())),
            job: None,
            status: "Donanım ve kaynaklar kontrol ediliyor…".into(),
            notice,
            stopping: false,
            closing: false,
            last_file: None,
            page: if settings_page {
                Page::Settings
            } else {
                Page::Recorder
            },
            settings_tab,
            advanced_unlocked: false,
            window_filter: String::new(),
            audio_preview: None,
            preview_settings: None,
            hotkeys,
            active_bindings: HotkeyBindings {
                enabled: false,
                ..HotkeyBindings::default()
            },
            pending_bindings: None,
            shortcut_status: "Kısayollar hazırlanıyor…".into(),
            listening: None,
            listener_ready: None,
            screenshot,
            smoke_seconds,
            smoke_started: None,
        };
        app.configure_bindings(app.prefs.hotkeys.clone(), None);
        app
    }

    fn configure_bindings(&mut self, bindings: HotkeyBindings, save: Option<Preferences>) {
        if let Some(service) = &self.hotkeys {
            match service.configure(bindings.clone()) {
                Ok(result) => {
                    self.pending_bindings = Some(PendingBindings {
                        result,
                        bindings,
                        save,
                    })
                }
                Err(error) => self.notice = Some((error.to_string(), true)),
            }
        } else if save.is_some() {
            self.notice = Some((
                "Ayarlar kaydedildi; genel kısayol hizmeti bu oturumda kullanılamıyor.".into(),
                true,
            ));
        }
    }
    fn persist(&mut self, prefs: Preferences) -> bool {
        let result = storage::save(&self.settings_file, &prefs);
        match result {
            Ok(()) => {
                self.saved = prefs;
                self.notice = Some((
                    "Ayarlar kaydedildi. Kayıt ve kısayol seçimlerin bu bilgisayarda kalır.".into(),
                    false,
                ));
                true
            }
            Err(error) => {
                self.notice = Some((format!("Ayar dosyası kaydedilemedi: {error:#}"), true));
                false
            }
        }
    }
    fn save(&mut self) {
        if let Err(error) = self.prefs.validate() {
            self.notice = Some((error.to_string(), true));
            return;
        }
        if self.persist(self.prefs.clone()) {
            self.configure_bindings(self.prefs.hotkeys.clone(), Some(self.prefs.clone()));
        }
    }
    fn refresh_sources(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::sync_channel(1);
        self.inspection = rx;
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(recording_windows::inspect().map_err(|e| format!("{e:#}")));
            ctx.request_repaint();
        });
        self.notice = Some(("Kaynak listesi yenileniyor…".into(), false));
    }
    fn begin_assignment(&mut self, action: usize) {
        if let Some(service) = &self.hotkeys {
            match service.suspend() {
                Ok(ready) => {
                    self.listener_ready = Some(ready);
                    self.listening = Some(action);
                    self.shortcut_status = "Yeni kısayolunu bas. İptal: Esc".into();
                }
                Err(error) => self.notice = Some((error.to_string(), true)),
            }
        }
    }
    fn process_shortcuts(&mut self, ctx: &egui::Context) {
        if let Some(ready) = &self.listener_ready
            && let Ok(result) = ready.try_recv()
        {
            self.listener_ready = None;
            if let Err(error) = result {
                self.listening = None;
                self.notice = Some((error, true));
            }
        }
        if let Some(action) = self.listening {
            if self.listener_ready.is_none() {
                let key = ctx.input(|i| {
                    i.events.iter().find_map(|e| match e {
                        egui::Event::Key {
                            key,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } => Some((*key, *modifiers)),
                        _ => None,
                    })
                });
                if let Some((key, modifiers)) = key {
                    if key == egui::Key::Escape {
                        self.listening = None;
                    } else {
                        match hotkeys::from_key(key, modifiers) {
                            Ok(shortcut) => {
                                match action {
                                    0 => self.prefs.hotkeys.toggle = shortcut,
                                    1 => self.prefs.hotkeys.stop = shortcut,
                                    _ => self.prefs.hotkeys.mute = shortcut,
                                };
                                self.listening = None;
                                self.notice = Some((
                                    "Yeni kısayol seçildi. Etkinleştirmek için ayarları kaydet."
                                        .into(),
                                    false,
                                ));
                            }
                            Err(error) => self.shortcut_status = error.to_string(),
                        }
                    }
                    if self.listening.is_none() {
                        self.configure_bindings(self.active_bindings.clone(), None);
                    }
                }
            }
            if let Some(service) = &self.hotkeys {
                while service.events.try_recv().is_ok() {}
            }
        } else {
            let mut actions = Vec::new();
            if let Some(service) = &self.hotkeys {
                while let Ok(action) = service.events.try_recv() {
                    actions.push(action);
                }
            }
            for action in actions {
                match action {
                    hotkeys::Action::Toggle => {
                        if self.job.is_some() {
                            self.stop();
                        } else {
                            self.start(ctx);
                        }
                    }
                    hotkeys::Action::Stop => self.stop(),
                    hotkeys::Action::Mute => {
                        self.prefs.recording.audio.microphone_muted =
                            !self.prefs.recording.audio.microphone_muted;
                    }
                }
            }
        }
        let response = self
            .pending_bindings
            .as_ref()
            .and_then(|p| match p.result.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Kısayol hizmeti beklenmedik şekilde kapandı.".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some(response) = response {
            let pending = self.pending_bindings.take().unwrap();
            match response {
                Ok(()) => {
                    self.shortcut_status = if pending.bindings.enabled {
                        "Genel kısayollar etkin".into()
                    } else {
                        "Genel kısayollar kapalı".into()
                    };
                    self.active_bindings = pending.bindings;
                }
                Err(error) => {
                    let saved_other_settings = pending.save.is_some();
                    if let Some(mut prefs) = pending.save {
                        prefs.hotkeys = self.active_bindings.clone();
                        if !self.persist(prefs) {
                            return;
                        }
                    }
                    self.shortcut_status = "Kısayol çakışması — önceki seçim korunuyor".into();
                    self.notice = Some((
                        if saved_other_settings {
                            format!("Diğer ayarlar kaydedildi. Kısayol uygulanamadı: {error}")
                        } else {
                            error
                        },
                        true,
                    ));
                }
            }
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        if self.job.is_some() {
            return;
        }
        if self.prefs.interface.window_capture && self.prefs.recording.window.is_none() {
            self.notice = Some(("Önce kaydedilecek oyun veya pencereyi seç.".into(), true));
            return;
        }
        if let Err(error) = self.prefs.recording.validate() {
            self.notice = Some((error.to_string(), true));
            return;
        }
        self.audio_preview.take();
        self.preview_settings = None;
        self.metrics = Arc::new(Mutex::new(Metrics::default()));
        let metrics = self.metrics.clone();
        let settings = self.prefs.recording.clone();
        let audio = Arc::new(AudioControls::new(&settings.audio));
        let controls = audio.clone();
        let ui_settings = self.prefs.interface.clone();
        let ctx = ctx.clone();
        let delay = if self.smoke_seconds.is_some() {
            0
        } else {
            ui_settings.countdown
        };
        let phase = Arc::new(AtomicU8::new(if delay > 0 { 0 } else { 1 }));
        let worker_phase = phase.clone();
        let countdown_until = Instant::now() + Duration::from_secs(delay.into());
        let (stop, commands) = mpsc::sync_channel(1);
        let (tx, result) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let result = (|| -> anyhow::Result<Option<RecordingReport>> {
                if delay > 0
                    && !matches!(
                        commands.recv_timeout(Duration::from_secs(delay.into())),
                        Err(mpsc::RecvTimeoutError::Timeout)
                    )
                {
                    return Ok(None);
                }
                if commands.try_recv().is_ok() {
                    return Ok(None);
                }
                worker_phase.store(1, Ordering::Relaxed);
                ctx.request_repaint();
                let mut recording =
                    recording_windows::start_with_audio_controls(settings, metrics, controls)?;
                worker_phase.store(2, Ordering::Relaxed);
                if ui_settings.minimize_on_start {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                ctx.request_repaint();
                let mut repaint = Instant::now();
                loop {
                    match commands.recv_timeout(Duration::from_millis(200)) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    recording.poll();
                    if recording.is_finished() {
                        break;
                    }
                    if repaint.elapsed() >= Duration::from_secs(ui_settings.refresh_seconds.into())
                    {
                        ctx.request_repaint();
                        repaint = Instant::now();
                    }
                }
                worker_phase.store(3, Ordering::Relaxed);
                ctx.request_repaint();
                let report = recording.finish()?;
                if ui_settings.restore_on_stop {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                }
                Ok(Some(report))
            })()
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.job = Some(Job {
            stop,
            result,
            worker: Some(worker),
            phase,
            countdown_until,
            audio,
        });
        self.status = "Kayıt hazırlanıyor…".into();
        self.stopping = false;
        self.notice = None;
    }
    fn stop(&mut self) {
        if let Some(job) = &self.job
            && !self.stopping
        {
            let _ = job.stop.try_send(());
            self.stopping = true;
            self.status = "Kayıt kapatılıyor, ses ekleniyor ve MP4 doğrulanıyor…".into();
        }
    }
    fn open_folder(&mut self) {
        let folder = self
            .last_file
            .as_ref()
            .and_then(|p| p.parent())
            .unwrap_or(&self.prefs.recording.output_dir);
        if let Err(error) = std::fs::create_dir_all(folder).and_then(|_| {
            std::process::Command::new("explorer.exe")
                .arg(folder)
                .spawn()
                .map(|_| ())
        }) {
            self.notice = Some((format!("Klasör açılamadı: {error}"), true));
        }
    }
}

impl eframe::App for FeatherRec {
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.process_shortcuts(ctx);
        if let Some(preview) = &self.audio_preview {
            if self.preview_settings.as_ref().is_some_and(|s| {
                s.audio.source != self.prefs.recording.audio.source
                    || s.audio.microphone != self.prefs.recording.audio.microphone
                    || s.audio.output_device != self.prefs.recording.audio.output_device
                    || s.audio.microphone_device != self.prefs.recording.audio.microphone_device
                    || s.audio.application != self.prefs.recording.audio.application
                    || s.window != self.prefs.recording.window
            }) {
                self.audio_preview.take();
                self.preview_settings = None;
            } else {
                preview.controls.update(&self.prefs.recording.audio);
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }
        let screenshots: Vec<_> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| {
                    if let egui::Event::Screenshot { image, .. } = e {
                        Some(image.clone())
                    } else {
                        None
                    }
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
                self.notice = Some((error.to_string(), true));
            }
            if self.smoke_seconds.is_none() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if let Ok(result) = self.inspection.try_recv() {
            match result {
                Ok(info) => {
                    if self.status.starts_with("Donanım") {
                        self.status = "Hazır. Kaydın yalnızca bu bilgisayarda saklanır.".into();
                    }
                    self.hardware = Some(info);
                }
                Err(error) => {
                    self.status = "Kaynaklar alınamadı".into();
                    self.notice = Some((error, true));
                }
            }
        }
        let completed = self.job.as_ref().and_then(|j| match j.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("Kayıt iş parçacığı kapandı.".into()))
            }
            Err(mpsc::TryRecvError::Empty) => None,
        });
        if let Some(result) = completed {
            self.job.take();
            self.stopping = false;
            let successful = result
                .as_ref()
                .is_ok_and(|r| r.as_ref().is_none_or(|r| r.finalized));
            match result {
                Ok(Some(report)) => {
                    *self.metrics.lock().unwrap() = report.metrics.clone();
                    self.status = if report.finalized {
                        "Kayıt kaydedildi ve doğrulandı.".into()
                    } else {
                        "Kayıt tamamlanamadı. Geçici dosya kurtarma için korundu.".into()
                    };
                    if !report.finalized {
                        self.notice = Some((report.validation, true));
                    }
                    self.last_file = Some(report.file);
                }
                Ok(None) => self.status = "Geri sayım iptal edildi.".into(),
                Err(error) => {
                    self.status = "Kayıt başlatılamadı veya tamamlanamadı.".into();
                    self.notice = Some((error, true));
                }
            }
            if (self.closing && successful) || self.smoke_seconds.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.closing = false;
            }
        }
        if self.hardware.is_some() {
            if self.smoke_seconds.is_some() && self.smoke_started.is_none() {
                self.prefs.recording.output_dir = portable_root().join("validation");
                self.smoke_started = Some(Instant::now());
                self.start(ctx);
            }
            if let (Some(seconds), Some(started)) = (self.smoke_seconds, self.smoke_started)
                && started.elapsed() >= Duration::from_secs(seconds)
                && !self.stopping
            {
                self.closing = true;
                self.stop();
            }
            if self.screenshot.is_some()
                && screenshots.is_empty()
                && self.pending_bindings.is_none()
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.job.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.closing = true;
            self.stop();
        }
        if let Some(job) = &self.job {
            job.audio.update(&self.prefs.recording.audio);
            ctx.request_repaint_after(if job.phase.load(Ordering::Relaxed) == 0 {
                Duration::from_millis(200)
            } else {
                Duration::from_secs(self.prefs.interface.refresh_seconds.into())
            });
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.draw(ui);
        if let Some(job) = &self.job {
            job.audio.update(&self.prefs.recording.audio);
        }
    }
}

fn main() {
    let icon = image::load_from_memory(include_bytes!("../../../assets/featherrec.png"))
        .expect("Embedded application icon is invalid")
        .into_rgba8();
    let icon = egui::IconData {
        width: icon.width(),
        height: icon.height(),
        rgba: icon.into_raw(),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1080.0, 800.0])
            .with_min_inner_size([900.0, 680.0])
            .with_title("FeatherRec")
            .with_icon(icon),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    if let Err(error) = eframe::run_native(
        "FeatherRec",
        options,
        Box::new(|cc| Ok(Box::new(FeatherRec::new(cc)))),
    ) {
        let _ = rfd::MessageDialog::new()
            .set_title("FeatherRec başlatılamadı")
            .set_description(error.to_string())
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
}
