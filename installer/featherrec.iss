#define AppVersion "0.1.1"
#define Root SourcePath + "..\"

[Setup]
AppId={{971DC42C-BB3A-4805-8D50-086FD36614A9}
AppName=FeatherRec
AppVersion={#AppVersion}
AppPublisher=FeatherRec contributors
AppPublisherURL=https://github.com/boranseasonnew/featherrec
AppSupportURL=https://github.com/boranseasonnew/featherrec/issues
DefaultDirName={localappdata}\Programs\FeatherRec
DefaultGroupName=FeatherRec
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#Root}target
OutputBaseFilename=FeatherRec-Setup-{#AppVersion}
SetupIconFile={#Root}assets\featherrec.ico
UninstallDisplayIcon={app}\featherrec.exe
LicenseFile={#Root}LICENSE
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
DisableProgramGroupPage=yes
CloseApplications=yes

[Languages]
Name: "turkish"; MessagesFile: "compiler:Languages\Turkish.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Masaüstü kısayolu oluştur"; Flags: unchecked

[Files]
Source: "{#Root}target\release\featherrec.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}target\release\featherrec-cli.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\FeatherRec"; Filename: "{app}\featherrec.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\FeatherRec"; Filename: "{app}\featherrec.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\featherrec.exe"; Description: "FeatherRec'i aç"; Flags: nowait postinstall skipifsilent
