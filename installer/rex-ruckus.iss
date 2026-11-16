; Inno Setup 6.6+ script for Rex Ruckus: Meltdown. Build the release exe first, then:
;   iscc /DAppVersion=0.2.0 installer\rex-ruckus.iss
; The installer lands in installer\Output\. The icon and wizard images come from
; scripts/build-icons.sh (rerun it after editing assets/icon/*.svg).

#ifndef AppVersion
  #define AppVersion "0.0.0-dev"
#endif
; VersionInfoVersion takes only digits: strip a "-dev"/"-sha" suffix.
#define NumericVersion Copy(AppVersion, 1, Pos("-", AppVersion + "-") - 1)
#define AppName "Rex Ruckus: Meltdown"
#define RepoURL "https://github.com/DeepDiver1975/rex-ruckus"

[Setup]
AppId={{6F2A3C1E-8B4D-4E7A-9C5F-2D1B0A9E7F43}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Rex Ruckus contributors
AppCopyright=Rex Ruckus contributors, GPL-3.0-or-later
AppPublisherURL={#RepoURL}
AppSupportURL={#RepoURL}/issues
AppUpdatesURL={#RepoURL}/releases
VersionInfoVersion={#NumericVersion}
VersionInfoProductName={#AppName}
VersionInfoDescription={#AppName} Setup
; Per-user install by default (no UAC prompt); the dialog offers an all-users install.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
DefaultDirName={autopf}\Rex Ruckus
DisableProgramGroupPage=yes
LicenseFile=..\LICENSE
OutputDir=Output
OutputBaseFilename=rex-ruckus-{#AppVersion}-windows-x64-setup
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
WizardStyle=modern dynamic
SetupIconFile=rex-ruckus.ico
WizardImageFile=wizard\large-100.png,wizard\large-150.png,wizard\large-200.png,wizard\large-250.png
WizardSmallImageFile=wizard\small-100.png,wizard\small-150.png,wizard\small-200.png,wizard\small-250.png
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\rex-ruckus.exe

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\rex-ruckus.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\*"; DestDir: "{app}\assets"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"
Source: "..\CREDITS.md"; DestDir: "{app}"
Source: "..\README.md"; DestDir: "{app}"

[Icons]
; One entry at the top of Start > All apps; uninstall lives in Settings > Apps.
Name: "{autoprograms}\Rex Ruckus Meltdown"; Filename: "{app}\rex-ruckus.exe"; WorkingDir: "{app}"; Comment: "Fast 90s-style 3D shooter"
Name: "{autodesktop}\Rex Ruckus Meltdown"; Filename: "{app}\rex-ruckus.exe"; WorkingDir: "{app}"; Comment: "Fast 90s-style 3D shooter"; Tasks: desktopicon

[InstallDelete]
; Earlier installers made a "Rex Ruckus" Start Menu folder with an uninstall shortcut.
Type: filesandordirs; Name: "{autoprograms}\Rex Ruckus"

[Run]
Filename: "{app}\rex-ruckus.exe"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
