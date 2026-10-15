use anyhow::{Result, ensure};
use recorder_core::Settings;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shortcut {
    pub key: u32,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Shortcut {
    pub fn validate(self) -> Result<()> {
        ensure!(
            self.ctrl || self.alt,
            "En az Ctrl veya Alt kullan. Tek başına bir tuş atanamaz."
        );
        ensure!(
            matches!(self.key, 0x30..=0x39 | 0x41..=0x5A | 0x70..=0x87) && self.key != 0x7B,
            "Harf, rakam veya F1–F24 kullan. F12 Windows tarafından ayrılmıştır."
        );
        Ok(())
    }
    pub fn label(self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        parts.push(if self.key >= 0x70 {
            format!("F{}", self.key - 0x6F)
        } else {
            char::from_u32(self.key).unwrap_or('?').to_string()
        });
        parts.join(" + ")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeyBindings {
    pub enabled: bool,
    pub toggle: Shortcut,
    pub stop: Shortcut,
    pub mute: Shortcut,
}
impl Default for HotkeyBindings {
    fn default() -> Self {
        let shortcut = |key| Shortcut {
            key,
            ctrl: true,
            shift: true,
            alt: false,
        };
        Self {
            enabled: true,
            toggle: shortcut(0x78),
            stop: shortcut(0x79),
            mute: shortcut(0x4D),
        }
    }
}
impl HotkeyBindings {
    pub fn validate(&self) -> Result<()> {
        self.toggle.validate()?;
        self.stop.validate()?;
        self.mute.validate()?;
        ensure!(
            self.toggle != self.stop && self.toggle != self.mute && self.stop != self.mute,
            "Her işlem için farklı bir kısayol seç."
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InterfaceSettings {
    pub show_diagnostics: bool,
    pub refresh_seconds: u32,
    pub minimize_on_start: bool,
    pub restore_on_stop: bool,
    pub countdown: u32,
    pub window_capture: bool,
}
impl Default for InterfaceSettings {
    fn default() -> Self {
        Self {
            show_diagnostics: true,
            refresh_seconds: 1,
            minimize_on_start: false,
            restore_on_stop: false,
            countdown: 0,
            window_capture: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(flatten)]
    pub recording: Settings,
    #[serde(default)]
    pub hotkeys: HotkeyBindings,
    #[serde(default)]
    pub interface: InterfaceSettings,
}
impl Preferences {
    pub fn new(recording: Settings) -> Self {
        Self {
            recording,
            hotkeys: HotkeyBindings::default(),
            interface: InterfaceSettings::default(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        // A saved process/window handle is intentionally not reused after restart.
        let mut settings = self.recording.clone();
        if settings.window.is_none() && settings.audio.application.is_none() {
            settings.audio.source = recorder_core::SystemAudio::Off;
        }
        settings.validate()?;
        self.hotkeys.validate()?;
        ensure!(
            [1, 2, 5].contains(&self.interface.refresh_seconds),
            "Geçersiz arayüz yenileme aralığı"
        );
        ensure!(
            [0, 3, 5, 10].contains(&self.interface.countdown),
            "Geçersiz geri sayım"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imports_old_preferences_without_enabling_microphone() {
        let old = serde_json::to_string(&Settings::default()).unwrap();
        let prefs: Preferences = serde_json::from_str(&old).unwrap();
        prefs.validate().unwrap();
        assert!(!prefs.recording.audio.microphone);
        assert_eq!(prefs.hotkeys.toggle.label(), "Ctrl + Shift + F9");
    }
    #[test]
    fn rejects_duplicate_and_reserved_hotkeys() {
        let mut bindings = HotkeyBindings::default();
        bindings.stop = bindings.toggle;
        assert!(bindings.validate().is_err());
        bindings.stop.key = 0x7B;
        assert!(bindings.validate().is_err());
    }
}
