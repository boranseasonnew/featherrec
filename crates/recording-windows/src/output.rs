use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn prepare_folder(path: &Path) -> Result<PathBuf> {
    fs::create_dir_all(path).with_context(|| format!(
        "Kayıt klasörü oluşturulamadı: {}. Kayıt konumunu değiştir veya klasörün yazma izinlerini kontrol et.", path.display()
    ))?;
    fs::canonicalize(path).with_context(|| {
        format!(
            "Kayıt klasörü açılamadı: {}. Ayarlar bölümünden erişilebilir bir kayıt konumu seç.",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(
                std::env::temp_dir().join(format!(
                    "featherrec-output-{}-{}",
                    std::process::id(),
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                )),
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn creates_missing_unicode_folder_and_accepts_existing_folder() -> Result<()> {
        let fixture = Fixture::new();
        let selected = fixture.0.join("Kayıtlar/çekimler");
        let folder = prepare_folder(&selected)?;
        assert!(folder.is_absolute() && folder.is_dir());
        assert_eq!(prepare_folder(&selected)?, folder);
        let file = folder.join("test.tmp");
        fs::write(&file, b"test")?;
        assert_eq!(fs::read(&file)?, b"test");
        Ok(())
    }

    #[test]
    fn reports_the_selected_path_when_a_parent_is_not_a_directory() -> Result<()> {
        let fixture = Fixture::new();
        fs::create_dir_all(&fixture.0)?;
        let parent = fixture.0.join("file");
        fs::write(&parent, b"keep")?;
        let selected = parent.join("recordings");
        let error = prepare_folder(&selected).unwrap_err();
        assert!(error.to_string().contains(&selected.display().to_string()));
        assert!(error.to_string().contains("Kayıt klasörü"));
        assert_eq!(fs::read(parent)?, b"keep");
        Ok(())
    }
}
