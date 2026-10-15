use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=../../assets/featherrec.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let icon = root
        .join("assets/featherrec.ico")
        .canonicalize()
        .expect("Missing application icon")
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/");
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let numeric = version
        .split('.')
        .map(|s| s.parse::<u16>().unwrap().to_string())
        .collect::<Vec<_>>()
        .join(",");
    let resource = output.join("featherrec.rc");
    fs::write(
        &resource,
        format!(
            r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numeric},0
PRODUCTVERSION {numeric},0
FILEFLAGSMASK 0x3fL
FILEFLAGS 0
FILEOS 0x40004L
FILETYPE 1
FILESUBTYPE 0
BEGIN
 BLOCK "StringFileInfo"
 BEGIN
  BLOCK "040904B0"
  BEGIN
   VALUE "FileDescription", "FeatherRec ekran kaydedici\0"
   VALUE "ProductName", "FeatherRec\0"
   VALUE "FileVersion", "{version}\0"
   VALUE "ProductVersion", "{version}\0"
   VALUE "OriginalFilename", "featherrec.exe\0"
   VALUE "LegalCopyright", "MIT License\0"
  END
 END
 BLOCK "VarFileInfo"
 BEGIN
  VALUE "Translation", 0x0409, 1200
 END
END
"#
        ),
    )
    .unwrap();
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("RC") {
        candidates.push(PathBuf::from(path));
    }
    if let Some(root) = env::var_os("WindowsSdkDir") {
        add_sdk(&mut candidates, PathBuf::from(root).join("bin"));
    }
    let programs = env::var_os("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:/Program Files (x86)"));
    add_sdk(&mut candidates, programs.join("Windows Kits/10/bin"));
    candidates.push(PathBuf::from("rc.exe"));
    let compiled = output.join("featherrec.res");
    let mut success = false;
    for compiler in candidates {
        match Command::new(&compiler)
            .arg("/nologo")
            .arg("/c65001")
            .arg("/fo")
            .arg(&compiled)
            .arg(&resource)
            .status()
        {
            Ok(status) if status.success() => {
                success = true;
                break;
            }
            Ok(status) => panic!("Windows resource compiler failed: {status}"),
            Err(_) => continue,
        }
    }
    assert!(
        success,
        "Windows SDK rc.exe is required to embed the application icon"
    );
    for binary in ["featherrec", "featherrec-cli"] {
        println!("cargo:rustc-link-arg-bin={binary}={}", compiled.display());
    }
}

fn add_sdk(candidates: &mut Vec<PathBuf>, bin: PathBuf) {
    if let Ok(entries) = fs::read_dir(bin) {
        let mut versions: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|e| e.path().join("x64/rc.exe"))
            .filter(|p| p.is_file())
            .collect();
        versions.sort();
        versions.reverse();
        candidates.extend(versions);
    }
}
