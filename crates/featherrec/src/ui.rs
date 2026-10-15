use super::{FeatherRec, Page, SettingsTab};
use crate::preferences::HotkeyBindings;
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use recorder_core::{AudioTrack, EncoderPolicy, Preset, RateControl, SystemAudio};
use std::sync::atomic::Ordering;

const BG: Color32 = Color32::from_rgb(15, 18, 25);
const PANEL: Color32 = Color32::from_rgb(23, 28, 38);
const LINE: Color32 = Color32::from_rgb(44, 52, 67);
const MUTED: Color32 = Color32::from_rgb(157, 168, 188);
const ACCENT: Color32 = Color32::from_rgb(143, 177, 255);
const RED: Color32 = Color32::from_rgb(236, 91, 111);
const GREEN: Color32 = Color32::from_rgb(107, 212, 175);

pub fn install_theme(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.visuals.panel_fill = BG;
        s.visuals.window_fill = PANEL;
        s.visuals.extreme_bg_color = Color32::from_rgb(12, 15, 21);
        s.visuals.override_text_color = Some(Color32::from_rgb(231, 236, 246));
        s.visuals.selection.bg_fill = Color32::from_rgb(51, 72, 113);
        s.visuals.selection.stroke = Stroke::new(1.0, ACCENT);
        s.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        s.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(34, 41, 55);
        s.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(48, 59, 80);
        s.spacing.item_spacing = Vec2::new(10.0, 10.0);
        s.spacing.button_padding = Vec2::new(14.0, 9.0);
        s.spacing.interact_size.y = 32.0;
        s.text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        s.text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        s.text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(25.0));
        s.text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
    });
}

fn caption(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text).color(MUTED).size(12.0));
}
fn meter_level(peak: f32) -> f32 {
    if peak <= 0.000_001 {
        0.0
    } else {
        ((20.0 * peak.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
    }
}
fn card(ui: &mut egui::Ui, title: &str, subtitle: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(12)
        .inner_margin(18)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.label(RichText::new(title).strong().size(16.0));
            }
            if !subtitle.is_empty() {
                caption(ui, subtitle);
            }
            ui.add_space(3.0);
            body(ui);
        });
    ui.add_space(4.0);
}
fn preset_name(p: Preset) -> &'static str {
    match p {
        Preset::Compact => "Küçük dosya",
        Preset::Potato => "Potato",
        Preset::Smooth720 => "Akıcı 720p",
        Preset::Balanced30 => "Dengeli 30",
        Preset::Balanced => "Dengeli",
        Preset::Quality => "Yüksek kalite",
    }
}
fn source_name(source: SystemAudio) -> &'static str {
    match source {
        SystemAudio::Off => "Kapalı",
        SystemAudio::System => "Sistem sesi",
        SystemAudio::Application => "Yalnızca seçilen uygulama",
    }
}
fn device_combo(
    ui: &mut egui::Ui,
    id: &str,
    selected: &mut String,
    devices: &[recording_windows::AudioDevice],
) {
    let label = if selected.is_empty() {
        "Windows varsayılanı"
    } else {
        devices
            .iter()
            .find(|d| d.id == *selected)
            .map_or("Aygıt kullanılamıyor — yeniden seç", |d| {
                d.name.as_str()
            })
    };
    egui::ComboBox::from_id_salt(id)
        .width(ui.available_width().min(580.0))
        .selected_text(label)
        .show_ui(ui, |ui| {
            ui.selectable_value(selected, String::new(), "Windows varsayılanı");
            for device in devices {
                ui.selectable_value(selected, device.id.clone(), &device.name);
            }
        });
}

impl FeatherRec {
    pub(super) fn draw(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("navigation")
            .exact_size(184.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(18, 22, 31))
                    .inner_margin(18),
            )
            .show(ui, |ui| {
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), egui::Sense::hover());
                    let p = ui.painter();
                    let c = rect.center();
                    p.line_segment(
                        [c + Vec2::new(-8.0, 10.0), c + Vec2::new(8.0, -11.0)],
                        Stroke::new(2.5, ACCENT),
                    );
                    p.line_segment(
                        [c + Vec2::new(-3.0, 3.0), c + Vec2::new(-6.0, -7.0)],
                        Stroke::new(4.0, ACCENT),
                    );
                    p.line_segment(
                        [c + Vec2::new(1.0, -2.0), c + Vec2::new(10.0, -3.0)],
                        Stroke::new(4.0, ACCENT),
                    );
                    ui.label(RichText::new("FeatherRec").strong().size(18.0));
                });
                caption(ui, "Hafif kayıt. Tam kontrol.");
                ui.add_space(34.0);
                for (page, label) in [
                    (Page::Recorder, "Kayıt stüdyosu"),
                    (Page::Settings, "Ayarlar"),
                ] {
                    if ui
                        .add_sized(
                            [148.0, 43.0],
                            egui::Button::new(label).selected(self.page == page),
                        )
                        .clicked()
                    {
                        self.page = page;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    caption(ui, concat!(env!("CARGO_PKG_VERSION"), " · Windows"));
                    ui.label(
                        RichText::new("Yalnızca bu bilgisayarda")
                            .color(GREEN)
                            .size(12.0),
                    );
                    ui.add_space(12.0);
                    if let Some(h) = &self.hardware {
                        caption(
                            ui,
                            if h.h264_hardware_encoders.is_empty() {
                                "Donanım kodlayıcı bulunamadı"
                            } else {
                                "Donanım kodlayıcı mevcut"
                            },
                        );
                        if let Some(gpu) = h.gpu_adapters.first() {
                            caption(ui, gpu.split(" (vendor").next().unwrap_or(gpu));
                        }
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(24))
            .show(ui, |ui| {
                if self.page == Page::Settings {
                    egui::Panel::bottom("settings_save")
                        .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin {
                            top: 12,
                            ..Default::default()
                        }))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let dirty = self.prefs != self.saved;
                                if ui
                                    .add_enabled(
                                        self.pending_bindings.is_none() && self.listening.is_none(),
                                        egui::Button::new("Ayarları kaydet")
                                            .fill(Color32::from_rgb(49, 76, 128)),
                                    )
                                    .clicked()
                                {
                                    self.save();
                                }
                                if ui
                                    .add_enabled(
                                        dirty && self.listening.is_none(),
                                        egui::Button::new("Geri al"),
                                    )
                                    .clicked()
                                {
                                    self.prefs = self.saved.clone();
                                }
                                caption(
                                    ui,
                                    if dirty {
                                        "Kaydedilmemiş değişiklikler"
                                    } else {
                                        "Tüm değişiklikler kaydedildi"
                                    },
                                );
                            });
                        });
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.heading(if self.page == Page::Recorder {
                                "Kayıt stüdyosu"
                            } else {
                                "Ayarlar"
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(RichText::new("H.264 / MP4").color(MUTED).size(12.0));
                                },
                            );
                        });
                        caption(
                            ui,
                            if self.page == Page::Recorder {
                                "Oyununu, pencereni veya ekranını kaydet."
                            } else {
                                "Donanımına göre ayarla. Seçimlerin yerel olarak saklanır."
                            },
                        );
                        ui.add_space(14.0);
                        if let Some((message, error)) = self.notice.clone() {
                            egui::Frame::new()
                                .fill(if error {
                                    Color32::from_rgb(62, 30, 41)
                                } else {
                                    Color32::from_rgb(27, 48, 54)
                                })
                                .corner_radius(8)
                                .inner_margin(12)
                                .show(ui, |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(RichText::new(message).color(if error {
                                            RED
                                        } else {
                                            GREEN
                                        }));
                                        if ui.small_button("Kapat").clicked() {
                                            self.notice = None;
                                        }
                                    });
                                });
                            ui.add_space(8.0);
                        }
                        match self.page {
                            Page::Recorder => self.recorder_page(ui),
                            Page::Settings => self.settings_page(ui),
                        }
                    });
            });
    }

    fn recorder_page(&mut self, ui: &mut egui::Ui) {
        let m = self.metrics.lock().unwrap().clone();
        let phase = self.job.as_ref().map(|j| j.phase.load(Ordering::Relaxed));
        card(ui, "", "", |ui| {
            ui.horizontal(|ui| {
                let color = if phase == Some(2) { RED } else { ACCENT };
                let (r, _) = ui.allocate_exact_size(Vec2::splat(44.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(r.center(), 21.0, color.gamma_multiply(0.13));
                ui.painter().circle_filled(r.center(), 7.0, color);
                ui.vertical(|ui| {
                    let title = match phase {
                        Some(0) => "Birazdan kayıttasın",
                        Some(1) => "Kayıt hazırlanıyor",
                        Some(2) => "Kayıt devam ediyor",
                        Some(_) => "Kayıt tamamlanıyor",
                        None => "Kayda hazırsın",
                    };
                    ui.label(RichText::new(title).size(21.0).strong());
                    caption(ui, &self.status);
                });
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let seconds = m.elapsed_seconds.max(0.0) as u64;
                ui.label(
                    RichText::new(format!(
                        "{:02}:{:02}:{:02}",
                        seconds / 3600,
                        seconds / 60 % 60,
                        seconds % 60
                    ))
                    .monospace()
                    .size(38.0),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if self.stopping {
                        "Tamamlanıyor…"
                    } else if phase == Some(0) {
                        "Geri sayımı iptal et"
                    } else if self.job.is_some() {
                        "Kaydı durdur"
                    } else {
                        "Kaydı başlat"
                    };
                    if ui
                        .add_enabled(
                            !self.stopping && self.hardware.is_some(),
                            egui::Button::new(RichText::new(label).strong())
                                .fill(if self.job.is_some() {
                                    Color32::from_rgb(111, 40, 57)
                                } else {
                                    Color32::from_rgb(153, 46, 70)
                                })
                                .min_size(Vec2::new(174.0, 47.0)),
                        )
                        .clicked()
                    {
                        if self.job.is_some() {
                            self.stop();
                        } else {
                            self.start(ui.ctx());
                        }
                    }
                });
            });
            if phase == Some(0)
                && let Some(job) = &self.job
            {
                caption(
                    ui,
                    format!(
                        "{} saniye sonra başlayacak",
                        job.countdown_until
                            .saturating_duration_since(std::time::Instant::now())
                            .as_secs()
                            + 1
                    ),
                );
            }
            ui.horizontal_wrapped(|ui| {
                caption(
                    ui,
                    format!(
                        "{}p · {} FPS hedef · {}",
                        self.prefs.recording.height,
                        self.prefs.recording.fps,
                        if self.prefs.recording.rate_control == RateControl::Cqp {
                            format!("CQP {}", self.prefs.recording.qp)
                        } else {
                            format!("{:.1} Mbps", self.prefs.recording.bitrate as f32 / 1e6)
                        }
                    ),
                );
                if self.active_bindings.enabled {
                    caption(
                        ui,
                        format!("Başlat / durdur: {}", self.active_bindings.toggle.label()),
                    );
                }
            });
        });
        card(
            ui,
            "Donanımına uygun profil",
            "Başlangıç önerileri. FPS hedefi, oyuna ve donanımına göre değişir.",
            |ui| {
                ui.add_enabled_ui(self.job.is_none(), |ui| {
                    ui.columns(3, |columns| {
                        for (column, preset, label, detail) in [
                            (0, Preset::Potato, "Potato", "Düşük donanım · 720p / 30"),
                            (1, Preset::Balanced, "Dengeli", "Orta donanım · 1080p / 60"),
                            (
                                2,
                                Preset::Quality,
                                "Yüksek kalite",
                                "Güçlü donanım · 16 Mbps",
                            ),
                        ] {
                            if columns[column]
                                .add_sized(
                                    [columns[column].available_width(), 36.0],
                                    egui::Button::new(label)
                                        .selected(self.prefs.recording.preset == preset),
                                )
                                .clicked()
                            {
                                self.prefs.recording.apply_preset(preset);
                            }
                            caption(&mut columns[column], detail);
                        }
                    });
                });
            },
        );
        card(
            ui,
            "Kayıt kaynağı",
            "Kayda başlamadan önce neyi yakalayacağını seç.",
            |ui| {
                ui.add_enabled_ui(self.job.is_none(), |ui| {
                    self.source_controls(ui, false);
                });
            },
        );
        card(
            ui,
            "Ses masası",
            "Sistem veya uygulama sesi ile mikrofonunu ayrı yönet.",
            |ui| {
                self.audio_controls(ui, false);
            },
        );
        if self.prefs.interface.show_diagnostics {
            card(
                ui,
                "Kayıt durumu",
                "Ölçümler kayıt sırasında güncellenir.",
                |ui| {
                    ui.columns(4, |c| {
                        for (i, label, value) in [
                            (0, "Kare", m.submitted.to_string()),
                            (1, "Düşen kare", m.dropped.to_string()),
                            (
                                2,
                                "Bellek",
                                format!("{:.0} MB", m.working_set_bytes as f64 / 1048576.0),
                            ),
                            (3, "İşlemci", format!("%{:.1}", m.process_cpu_percent)),
                        ] {
                            caption(&mut c[i], label);
                            c[i].label(RichText::new(value).strong().size(20.0));
                        }
                    });
                    if let Some(warning) = &m.warning {
                        ui.colored_label(Color32::from_rgb(238, 196, 113), warning);
                    }
                    if let Some(error) = &m.error {
                        ui.colored_label(RED, error);
                    }
                },
            );
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("Kayıt klasörünü aç").clicked() {
                self.open_folder();
            }
            if let Some(path) = &self.last_file {
                caption(ui, path.file_name().unwrap_or_default().to_string_lossy());
            }
        });
    }

    fn settings_page(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (tab, name) in [
                (SettingsTab::General, "Genel"),
                (SettingsTab::Recording, "Kayıt"),
                (SettingsTab::Audio, "Ses"),
                (SettingsTab::Shortcuts, "Kısayollar"),
                (SettingsTab::Advanced, "Gelişmiş"),
            ] {
                ui.selectable_value(&mut self.settings_tab, tab, name);
            }
        });
        ui.add_space(12.0);
        match self.settings_tab {
            SettingsTab::General => self.general_settings(ui),
            SettingsTab::Recording => self.recording_settings(ui),
            SettingsTab::Audio => {
                card(
                    ui,
                    "Ses kaynakları",
                    "Giriş, çıkış ve uygulama sesini seç. Mikrofon varsayılan olarak kapalıdır.",
                    |ui| self.audio_controls(ui, true),
                );
                card(ui, "Ses kalitesi", "AAC · 48 kHz · stereo", |ui| {
                    ui.add_enabled_ui(self.job.is_none(), |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for rate in [96_000, 128_000, 192_000, 256_000] {
                                ui.selectable_value(
                                    &mut self.prefs.recording.audio.bitrate,
                                    rate,
                                    format!("{} kbps", rate / 1000),
                                );
                            }
                        });
                    });
                    caption(
                        ui,
                        "Ses, durdurduğunda MP4'e eklenir. Her parça yaklaşık 11 MB/dakika geçici disk alanı kullanır; daha çok parça, daha uzun sonlandırma ve daha büyük dosya demektir.",
                    );
                });
            }
            SettingsTab::Shortcuts => self.shortcut_settings(ui),
            SettingsTab::Advanced => self.advanced_settings(ui),
        }
    }

    fn general_settings(&mut self, ui: &mut egui::Ui) {
        card(
            ui,
            "Kayıt konumu",
            "MP4 dosyaları bu klasöre kaydedilir.",
            |ui| {
                ui.add_enabled_ui(self.job.is_none(), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(self.prefs.recording.output_dir.display().to_string());
                        if ui.button("Değiştir").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .set_directory(&self.prefs.recording.output_dir)
                                .pick_folder()
                        {
                            self.prefs.recording.output_dir = path;
                        }
                    });
                });
            },
        );
        card(
            ui,
            "Kayıt davranışı",
            "Hazırlanmak için kendine birkaç saniye ayırabilirsin.",
            |ui| {
                ui.add_enabled_ui(self.job.is_none(), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Geri sayım");
                        for seconds in [0, 3, 5, 10] {
                            ui.selectable_value(
                                &mut self.prefs.interface.countdown,
                                seconds,
                                if seconds == 0 {
                                    "Kapalı".into()
                                } else {
                                    format!("{seconds} sn")
                                },
                            );
                        }
                    });
                    ui.checkbox(
                        &mut self.prefs.interface.minimize_on_start,
                        "Kayıt başlayınca pencereyi küçült",
                    );
                    ui.checkbox(
                        &mut self.prefs.interface.restore_on_stop,
                        "Kayıt bittiğinde pencereyi geri getir",
                    );
                });
            },
        );
        card(
            ui,
            "Arayüz",
            "Daha seyrek güncelleme, arayüzün daha az çalışmasını sağlar.",
            |ui| {
                ui.checkbox(
                    &mut self.prefs.interface.show_diagnostics,
                    "Kayıt durumunu göster",
                );
                ui.horizontal_wrapped(|ui| {
                    ui.label("Güncelleme aralığı");
                    for seconds in [1, 2, 5] {
                        ui.selectable_value(
                            &mut self.prefs.interface.refresh_seconds,
                            seconds,
                            format!("{seconds} sn"),
                        );
                    }
                });
            },
        );
    }

    fn recording_settings(&mut self, ui: &mut egui::Ui) {
        card(
            ui,
            "Kayıt profilleri",
            "Profil çözünürlük, FPS ve video bit hızını ayarlar; bit hızı moduna döner.",
            |ui| {
                ui.add_enabled_ui(self.job.is_none(), |ui| {
                    egui::Grid::new("presets")
                        .num_columns(3)
                        .spacing([18.0, 10.0])
                        .show(ui, |ui| {
                            for (p, detail, rate) in [
                                (Preset::Compact, "720p · 30 FPS", "2.5 Mbps / küçük dosya"),
                                (Preset::Potato, "720p · 30 FPS", "4 Mbps / düşük donanım"),
                                (Preset::Smooth720, "720p · 60 FPS", "6 Mbps / akıcılık"),
                                (Preset::Balanced30, "1080p · 30 FPS", "6 Mbps / daha hafif"),
                                (Preset::Balanced, "1080p · 60 FPS", "8 Mbps / orta donanım"),
                                (Preset::Quality, "1080p · 60 FPS", "16 Mbps / güçlü donanım"),
                            ] {
                                if ui
                                    .selectable_label(
                                        self.prefs.recording.preset == p,
                                        preset_name(p),
                                    )
                                    .clicked()
                                {
                                    self.prefs.recording.apply_preset(p);
                                }
                                ui.label(detail);
                                caption(ui, rate);
                                ui.end_row();
                            }
                        });
                });
            },
        );
        card(ui, "Görüntü kaynağı", "", |ui| {
            ui.add_enabled_ui(self.job.is_none(), |ui| self.source_controls(ui, true));
        });
        card(ui, "Görüntü ve imleç", "", |ui| {
            ui.add_enabled_ui(self.job.is_none(),|ui| {
                ui.horizontal_wrapped(|ui| { ui.label("Çözünürlük"); for (w,h,label) in [(1280,720,"720p"),(1920,1080,"1080p")] { if ui.selectable_label(self.prefs.recording.height==h,label).clicked() { self.prefs.recording.width=w; self.prefs.recording.height=h; } } ui.label("FPS hedefi"); ui.selectable_value(&mut self.prefs.recording.fps,30,"30"); ui.selectable_value(&mut self.prefs.recording.fps,60,"60"); });
                ui.checkbox(&mut self.prefs.recording.cursor,"Fare imlecini kaydet");
                ui.checkbox(&mut self.prefs.recording.show_capture_border,"Windows'un sarı kayıt çerçevesini göster");
                caption(ui,"Sarı çerçeve varsayılan olarak kapalıdır. Bunu kapatabilmek Windows sürümünün desteğine bağlıdır.");
            });
        });
    }

    fn source_controls(&mut self, ui: &mut egui::Ui, detailed: bool) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_value(&mut self.prefs.interface.window_capture, false, "Ekran")
                .changed()
            {
                self.prefs.recording.window = None;
                if self.prefs.recording.audio.source == SystemAudio::Application
                    && self.prefs.recording.audio.application.is_none()
                {
                    self.prefs.recording.audio.source = SystemAudio::Off;
                }
            }
            ui.selectable_value(
                &mut self.prefs.interface.window_capture,
                true,
                "Pencere / oyun",
            );
            if ui.small_button("Listeyi yenile").clicked() {
                self.refresh_sources(ui.ctx());
            }
        });
        if self.prefs.interface.window_capture {
            if detailed {
                ui.add(
                    egui::TextEdit::singleline(&mut self.window_filter)
                        .hint_text("Pencere adıyla ara…")
                        .desired_width(350.0),
                );
            }
            let selected = self
                .prefs
                .recording
                .window
                .as_ref()
                .map_or("Pencere seç…", |w| w.title.as_str())
                .to_owned();
            egui::ComboBox::from_id_salt("window")
                .width(ui.available_width().min(590.0))
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    if let Some(h) = &self.hardware {
                        for w in &h.windows {
                            if w.title
                                .to_lowercase()
                                .contains(&self.window_filter.to_lowercase())
                            {
                                ui.selectable_value(
                                    &mut self.prefs.recording.window,
                                    Some(w.clone()),
                                    &w.title,
                                );
                            }
                        }
                    }
                });
            if self.prefs.recording.window.is_none() {
                ui.colored_label(ACCENT, "Başlamak için açık bir oyun veya pencere seç.");
            }
            if detailed {
                caption(
                    ui,
                    "Windows pencere yakalama kullanılır. Bazı tam ekran veya korumalı oyunlar görüntü vermeyebilir; kenarlıksız pencere modunu dene.",
                );
            }
        } else {
            let label = self
                .hardware
                .as_ref()
                .and_then(|h| {
                    h.displays
                        .iter()
                        .find(|d| d.index == self.prefs.recording.monitor)
                })
                .map_or_else(
                    || "Ekran seç…".into(),
                    |d| {
                        format!(
                            "Ekran {} · {} × {} · {} Hz",
                            d.index, d.width, d.height, d.refresh_hz
                        )
                    },
                );
            egui::ComboBox::from_id_salt("monitor")
                .width(370.0)
                .selected_text(label)
                .show_ui(ui, |ui| {
                    if let Some(h) = &self.hardware {
                        for d in &h.displays {
                            ui.selectable_value(
                                &mut self.prefs.recording.monitor,
                                d.index,
                                format!(
                                    "Ekran {} · {} × {} · {} Hz",
                                    d.index, d.width, d.height, d.refresh_hz
                                ),
                            );
                        }
                    }
                });
        }
    }

    fn audio_controls(&mut self, ui: &mut egui::Ui, detailed: bool) {
        let devices = self
            .hardware
            .as_ref()
            .map(|h| h.audio_devices.clone())
            .unwrap_or_default();
        let preview = self.audio_preview.as_ref().map(|p| p.snapshot());
        let m = if self.job.is_some() {
            self.metrics.lock().unwrap().clone()
        } else {
            Default::default()
        };
        let system_peak = preview.as_ref().map_or(m.system_peak, |p| p.system_peak);
        let microphone_peak = preview
            .as_ref()
            .map_or(m.microphone_peak, |p| p.microphone_peak);
        ui.add_enabled_ui(self.job.is_none(), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Bilgisayar sesi");
                egui::ComboBox::from_id_salt("audio_source")
                    .selected_text(source_name(self.prefs.recording.audio.source))
                    .show_ui(ui, |ui| {
                        for s in [
                            SystemAudio::Off,
                            SystemAudio::System,
                            SystemAudio::Application,
                        ] {
                            ui.selectable_value(
                                &mut self.prefs.recording.audio.source,
                                s,
                                source_name(s),
                            );
                        }
                    });
            });
            if self.prefs.recording.audio.source == SystemAudio::Application {
                egui::ComboBox::from_id_salt("audio_application")
                    .width(ui.available_width().min(580.0))
                    .selected_text(
                        self.prefs
                            .recording
                            .audio
                            .application
                            .as_ref()
                            .map_or("Görüntü için seçilen uygulama", |w| w.title.as_str()),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.prefs.recording.audio.application,
                            None,
                            "Görüntü için seçilen uygulama",
                        );
                        if let Some(h) = &self.hardware {
                            for window in &h.windows {
                                ui.selectable_value(
                                    &mut self.prefs.recording.audio.application,
                                    Some(window.clone()),
                                    &window.title,
                                );
                            }
                        }
                    });
                if self.prefs.recording.audio.application.is_none()
                    && self.prefs.recording.window.is_none()
                {
                    ui.colored_label(RED, "Sesini yakalayacağın uygulamayı seç.");
                }
            }
            if self.prefs.recording.audio.source == SystemAudio::System {
                caption(ui, "Çıkış aygıtı · bilgisayar sesi bu aygıttan alınır");
                device_combo(
                    ui,
                    "system_device",
                    &mut self.prefs.recording.audio.output_device,
                    &devices.outputs,
                );
                if self.prefs.recording.audio.output_device.is_empty() {
                    caption(
                        ui,
                        format!(
                            "Windows varsayılanı: {}",
                            devices
                                .default_output
                                .as_ref()
                                .map_or("Aygıt bulunamadı", |d| d.name.as_str())
                        ),
                    );
                }
            }
            ui.checkbox(
                &mut self.prefs.recording.audio.microphone,
                "Mikrofonu kayda ekle",
            );
            if self.prefs.recording.audio.microphone {
                caption(ui, "Giriş aygıtı · mikrofon");
                device_combo(
                    ui,
                    "microphone_device",
                    &mut self.prefs.recording.audio.microphone_device,
                    &devices.microphones,
                );
                if self.prefs.recording.audio.microphone_device.is_empty() {
                    caption(
                        ui,
                        format!(
                            "Windows varsayılanı: {}",
                            devices
                                .default_microphone
                                .as_ref()
                                .map_or("Aygıt bulunamadı", |d| d.name.as_str())
                        ),
                    );
                }
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(if self.audio_preview.is_some() {
                        "Ses denemesini bitir"
                    } else {
                        "Ses denemesini başlat"
                    })
                    .clicked()
                {
                    if self.audio_preview.take().is_some() {
                        self.preview_settings = None;
                    } else {
                        match recording_windows::AudioPreview::start(&self.prefs.recording) {
                            Ok(preview) => {
                                self.preview_settings = Some(self.prefs.recording.clone());
                                self.audio_preview = Some(preview);
                            }
                            Err(error) => self.notice = Some((format!("{error:#}"), true)),
                        }
                    }
                }
                if ui.small_button("Aygıtları yenile").clicked() {
                    self.refresh_sources(ui.ctx());
                }
            });
        });
        if let Some(preview) = &preview {
            if let Some(error) = &preview.error {
                ui.colored_label(RED, error);
            } else if preview.running {
                caption(ui, format!("Ölçülüyor: {}", preview.devices.join(" · ")));
            } else {
                caption(ui, "Ses aygıtları açılıyor…");
            }
        } else if self.job.is_none() {
            caption(
                ui,
                "Göstergeyi kayıt öncesinde görmek için ses denemesini başlat. Ses çalınmaz ve dosya yazılmaz.",
            );
        }
        ui.add_space(4.0);
        if self.prefs.recording.audio.source != SystemAudio::Off {
            ui.horizontal(|ui| {
                ui.label("Oyun / sistem");
                ui.add(
                    egui::Slider::new(&mut self.prefs.recording.audio.system_gain, 0.0..=2.0)
                        .custom_formatter(|v, _| format!("%{:.0}", v * 100.0)),
                );
            });
            ui.add(
                egui::ProgressBar::new(meter_level(system_peak))
                    .fill(GREEN)
                    .desired_height(8.0),
            );
        }
        if self.prefs.recording.audio.microphone {
            ui.horizontal(|ui| {
                ui.label("Mikrofon");
                ui.add(
                    egui::Slider::new(&mut self.prefs.recording.audio.microphone_gain, 0.0..=2.0)
                        .custom_formatter(|v, _| format!("%{:.0}", v * 100.0)),
                );
                ui.checkbox(&mut self.prefs.recording.audio.microphone_muted, "Sustur");
            });
            ui.add(
                egui::ProgressBar::new(meter_level(microphone_peak))
                    .fill(if self.prefs.recording.audio.microphone_muted {
                        MUTED
                    } else {
                        GREEN
                    })
                    .desired_height(8.0),
            );
        }
        if detailed {
            caption(
                ui,
                "Susturma ve ses seviyeleri kayıt sırasında da değiştirilebilir. Windows varsayılanı seçilirse hoparlör/kulaklık değişimi takip edilir; belirli aygıt seçimi sabit kalır.",
            );
            if self.prefs.recording.audio.source == SystemAudio::Application {
                caption(
                    ui,
                    "Seçilen pencerenin uygulaması ve alt süreçleri yakalanır. Desteklenmezse sistem sesine geçilmez; hata gösterilir.",
                );
            }
            if let Some(error) = self
                .hardware
                .as_ref()
                .and_then(|h| h.audio_device_error.as_ref())
            {
                ui.colored_label(RED, error);
            }
            self.audio_tracks(ui);
        } else if ui.small_button("Ses parçalarını ayarla").clicked() {
            self.page = Page::Settings;
            self.settings_tab = SettingsTab::Audio;
        }
        if self.audio_preview.is_some()
            || (self.job.is_some() && self.prefs.recording.audio.enabled())
        {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn audio_tracks(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label(RichText::new("MP4 ses parçaları").strong());
        caption(
            ui,
            "Seçtiğin sayıda ayrı ses akışı yazılır. Editörün çoklu MP4 ses akışını desteklemelidir; bazı editörlerde parçaları ayrıca etkinleştirmek gerekir. Oynatıcı genellikle yalnızca ilk parçayı çalar.",
        );
        ui.add_enabled_ui(self.job.is_none(), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Parça sayısı");
                for count in 1..=5 {
                    if ui
                        .selectable_label(
                            self.prefs.recording.audio.tracks.len() == count,
                            count.to_string(),
                        )
                        .clicked()
                    {
                        self.prefs
                            .recording
                            .audio
                            .tracks
                            .resize_with(count, || AudioTrack {
                                system: false,
                                microphone: false,
                            });
                    }
                }
            });
            if ui
                .button("Önerilen: 1 karışım · 2 oyun/sistem · 3 mikrofon")
                .clicked()
            {
                self.prefs.recording.audio.tracks = vec![
                    AudioTrack::default(),
                    AudioTrack {
                        system: true,
                        microphone: false,
                    },
                    AudioTrack {
                        system: false,
                        microphone: true,
                    },
                ];
            }
            egui::Grid::new("audio_track_routing").show(ui, |ui| {
                ui.label("Parça");
                ui.label("Oyun / sistem");
                ui.label("Mikrofon");
                ui.end_row();
                for (index, track) in self.prefs.recording.audio.tracks.iter_mut().enumerate() {
                    ui.label((index + 1).to_string());
                    ui.checkbox(&mut track.system, "");
                    ui.checkbox(&mut track.microphone, "");
                    if !track.system && !track.microphone {
                        caption(ui, "Boş parça · sessizlik");
                    }
                    ui.end_row();
                }
            });
        });
        if !self.prefs.recording.audio.enabled() {
            caption(ui, "Ses kaynakları kapalıyken MP4'e ses parçası eklenmez.");
        }
    }

    fn shortcut_settings(&mut self, ui: &mut egui::Ui) {
        card(
            ui,
            "Genel kısayollar",
            "FeatherRec arka plandayken de kullanılabilir.",
            |ui| {
                ui.add_enabled_ui(
                    self.listening.is_none() && self.pending_bindings.is_none(),
                    |ui| {
                        ui.checkbox(
                            &mut self.prefs.hotkeys.enabled,
                            "Genel kısayolları etkinleştir",
                        );
                    },
                );
                for (index, label, binding) in [
                    (0, "Kaydı başlat / durdur", self.prefs.hotkeys.toggle),
                    (1, "Kaydı durdur", self.prefs.hotkeys.stop),
                    (2, "Mikrofonu sustur / aç", self.prefs.hotkeys.mute),
                ] {
                    ui.horizontal(|ui| {
                        ui.add_sized([215.0, 32.0], egui::Label::new(label));
                        if ui
                            .add_enabled(
                                self.pending_bindings.is_none() && self.listening.is_none(),
                                egui::Button::new(binding.label()).min_size(Vec2::new(195.0, 32.0)),
                            )
                            .clicked()
                        {
                            self.begin_assignment(index);
                        }
                    });
                    if self.listening == Some(index) {
                        ui.colored_label(ACCENT, &self.shortcut_status);
                    }
                }
                caption(
                    ui,
                    "Atamak için kısayol düğmesine tıkla, ardından Ctrl veya Alt içeren kombinasyona bas. İptal: Esc. F12 Windows tarafından ayrılmıştır.",
                );
                if self.listening.is_none() {
                    caption(ui, &self.shortcut_status);
                }
                if ui
                    .add_enabled(
                        self.listening.is_none(),
                        egui::Button::new("Varsayılan kısayollar"),
                    )
                    .clicked()
                {
                    self.prefs.hotkeys = HotkeyBindings::default();
                }
                caption(
                    ui,
                    "Yeni kombinasyonlar Ayarları kaydet ile uygulanır. Başka bir uygulamayla çakışırsa önceki kısayollar korunur.",
                );
            },
        );
    }

    fn advanced_settings(&mut self, ui: &mut egui::Ui) {
        card(
            ui,
            "Ne yaptığını biliyorsan kurcala.",
            "Bu ayarlar işlemci kullanımını, belleği ve görüntü kalitesini etkiler.",
            |ui| {
                ui.checkbox(
                    &mut self.advanced_unlocked,
                    "Gelişmiş ayarları düzenlemeyi aç",
                );
            },
        );
        ui.add_enabled_ui(self.advanced_unlocked && self.job.is_none(),|ui| {
            card(ui,"Kodlama","Varsayılan ayarlar çoğu kayıt için yeterlidir.",|ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Kalite kontrolü");
                    ui.selectable_value(&mut self.prefs.recording.rate_control, RateControl::Bitrate, "Bit hızı · varsayılan");
                    ui.selectable_value(&mut self.prefs.recording.rate_control, RateControl::Cqp, "CQP · donanıma bağlı");
                });
                if self.prefs.recording.rate_control == RateControl::Cqp {
                    ui.add(egui::Slider::new(&mut self.prefs.recording.qp,16..=51).text("QP"));
                    caption(ui,"Düşük QP daha yüksek kalite ve büyük dosya demektir. Başlangıç: 24. Kodlayıcı desteklemiyorsa kayıt açıklayıcı hatayla durur; sessizce başka moda geçilmez. CQP tek başına performans modu değildir.");
                } else {
                    let mut mbps=self.prefs.recording.bitrate as f64/1e6;
                    if ui.add(egui::Slider::new(&mut mbps,1.0..=50.0).step_by(0.5).text("Video bit hızı · Mbps")).changed() { self.prefs.recording.bitrate=(mbps*1e6) as u32; }
                    caption(ui,"Yüksek bit hızı daha fazla ayrıntı ve daha büyük dosya demektir. Kodlama yükü de artabilir.");
                }
                ui.checkbox(&mut self.prefs.recording.low_latency,"Düşük gecikmeli kodlama");
                let mut software=self.prefs.recording.encoder==EncoderPolicy::AllowSoftware;
                if ui.checkbox(&mut software,"Donanım kullanılamazsa yazılımla kodlamaya izin ver").changed() { self.prefs.recording.encoder=if software { EncoderPolicy::AllowSoftware } else { EncoderPolicy::Hardware }; }
                caption(ui,"Yazılımla kodlama işlemci kullanımını belirgin artırabilir. Kapalıyken gerçek bir donanım kodlayıcı doğrulanmadan kayıt başlamaz.");
            });
            card(ui,"Bellek ve raporlar","",|ui| {
                ui.add(egui::Slider::new(&mut self.prefs.recording.buffer_frames,3..=8).text("GPU kare tamponu"));
                caption(ui,"Daha fazla tampon ani yükleri karşılayabilir; GPU belleği ve gecikme artar. Varsayılan: 4.");
                ui.checkbox(&mut self.prefs.recording.write_report,"Kaydın yanına yerel tanılama raporu yaz");
                caption(ui,"Rapor; kayıt ayarlarını, hata ve performans ölçümlerini içerir. MP4 doğrulaması her zaman çalışır.");
                if ui.button("Gelişmiş varsayılanları geri yükle").clicked() { self.prefs.recording.buffer_frames=4; self.prefs.recording.low_latency=true; self.prefs.recording.encoder=EncoderPolicy::Hardware; self.prefs.recording.write_report=true; self.prefs.recording.rate_control=RateControl::Bitrate; self.prefs.recording.qp=24; self.prefs.recording.bitrate=match self.prefs.recording.preset { Preset::Compact=>2_500_000,Preset::Potato=>4_000_000,Preset::Smooth720|Preset::Balanced30=>6_000_000,Preset::Balanced=>8_000_000,Preset::Quality=>16_000_000 }; }
            });
        });
    }
}
