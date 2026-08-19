; Per-user, unsigned installer for RunDog.
; The release workflow invokes ISCC with /DAppVersion=X.Y.Z.

#ifndef AppVersion
  #define AppVersion "1.1.6"
#endif

#ifndef UpdateRepository
  #define UpdateRepository "tsuyoshi-otake/run-dog"
#endif

#define AppName "RunDog"
#define AppPublisher "SystemExe Research and Development"
#define AppExeName "RunDog.exe"

[Setup]
AppId={{9A37D507-7198-4BEB-8F84-9B95443C653C}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
AppUpdatesURL=https://github.com/{#UpdateRepository}/releases
DefaultDirName={localappdata}\Programs\RunDog
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=RunDog-Setup-x64
Compression=lzma2/ultra64
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\{#AppExeName}
SetupIconFile=..\assets\rundog.ico
CloseApplications=yes
CloseApplicationsFilter={#AppExeName}
RestartApplications=no

[Files]
Source: "..\target\release\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\RunDog"; Filename: "{app}\{#AppExeName}"
Name: "{autodesktop}\RunDog"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Run]
; Always start the resident application after installation, including a silent
; in-app upgrade. The updater starts Setup with CreateProcess after SHA-256
; verification, then asks the existing instance to close.
Filename: "{app}\{#AppExeName}"; Flags: nowait
