use crate::preferences::Preferences;
use anyhow::{Context, Result};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn settings_path() -> Result<PathBuf> {
    let root = std::env::var_os("LOCALAPPDATA")
        .filter(|p| !p.is_empty())
        .context("Windows kullanıcı ayar klasörü bulunamadı")?;
    Ok(PathBuf::from(root).join("FeatherRec/settings.json"))
}

pub fn load(path: &Path, legacy: &Path) -> Result<Option<Preferences>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let bytes = match fs::read(legacy) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error).context("Eski ayar dosyası okunamadı"),
            };
            let prefs: Preferences =
                serde_json::from_slice(&bytes).context("Eski ayar dosyası geçersiz")?;
            save(path, &prefs)?;
            return Ok(Some(prefs));
        }
        Err(error) => return Err(error).context("Kullanıcı ayarları okunamadı"),
    };
    let prefs: Preferences =
        serde_json::from_slice(&bytes).context("Kaydedilmiş ayarlar geçersiz")?;
    prefs.validate()?;
    Ok(Some(prefs))
}

/// Persist synchronously before acknowledging Save, independently of asynchronous
/// global-hotkey registration. A close immediately after Save cannot lose it.
pub fn save(path: &Path, prefs: &Preferences) -> Result<()> {
    prefs.validate()?;
    let parent = path.parent().context("Ayar klasörü bulunamadı")?;
    fs::create_dir_all(parent)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let temporary = path.with_extension(format!("{}.{}.tmp", std::process::id(), nonce));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(prefs)?)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path).context("Ayar dosyası güncellenemedi")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use recorder_core::{Preset, Settings, SystemAudio};
    struct Folder(PathBuf);
    impl Folder {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            Self(std::env::temp_dir().join(format!(
                "featherrec-settings-{}-{nonce}",
                std::process::id()
            )))
        }
    }
    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn settings_survive_restart_and_replace_existing_file() -> Result<()> {
        let folder = Folder::new();
        let path = folder.0.join("profile/settings.json");
        let legacy = folder.0.join("different-exe/featherrec-settings.json");
        let mut prefs = Preferences::new(Settings::default());
        save(&path, &prefs)?;
        prefs.recording.apply_preset(Preset::Quality);
        prefs.recording.audio.source = SystemAudio::System;
        prefs.recording.audio.system_gain = 0.3;
        prefs.recording.audio.microphone_muted = true;
        prefs.interface.countdown = 5;
        prefs.hotkeys.toggle.alt = true;
        save(&path, &prefs)?;
        assert_eq!(load(&path, &legacy)?.unwrap(), prefs);
        assert_eq!(fs::read_dir(path.parent().unwrap())?.count(), 1);
        Ok(())
    }

    #[test]
    fn migrates_portable_preferences_once_without_overwriting_newer_settings() -> Result<()> {
        let folder = Folder::new();
        fs::create_dir_all(&folder.0)?;
        let legacy = folder.0.join("featherrec-settings.json");
        let path = folder.0.join("profile/settings.json");
        let mut old = Preferences::new(Settings::default());
        old.recording.apply_preset(Preset::Balanced);
        fs::write(&legacy, serde_json::to_vec(&old)?)?;
        assert_eq!(load(&path, &legacy)?.unwrap(), old);
        let newer = Preferences::new(Settings::default());
        save(&path, &newer)?;
        assert_eq!(load(&path, &legacy)?.unwrap(), newer);
        Ok(())
    }

    #[test]
    fn rejects_invalid_save_and_preserves_existing_settings() -> Result<()> {
        let folder = Folder::new();
        let path = folder.0.join("settings.json");
        let mut prefs = Preferences::new(Settings::default());
        save(&path, &prefs)?;
        let before = fs::read(&path)?;
        prefs.recording.fps = 17;
        assert!(save(&path, &prefs).is_err());
        assert_eq!(fs::read(&path)?, before);
        Ok(())
    }
}
