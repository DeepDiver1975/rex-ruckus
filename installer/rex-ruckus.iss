; Inno Setup script for Rex Ruckus: Meltdown. Build the release exe first, then:
;   iscc /DAppVersion=0.1.0 installer\rex-ruckus.iss
; The installer lands in installer\Output\.

#ifndef AppVersion
  #define AppVersion "0.0.0-dev"
#endif

[Setup]
AppId={{6F2A3C1E-8B4D-4E7A-9C5F-2D1B0A9E7F43}
AppName=Rex Ruckus: Meltdown
AppVersion={#AppVersion}
AppPublisher=Rex Ruckus contributors
AppPublisherURL=https://github.com/DeepDiver1975/rex-ruckus
DefaultDirName={autopf}\Rex Ruckus
DefaultGroupName=Rex Ruckus
DisableProgramGroupPage=yes
LicenseFile=..\LICENSE
OutputDir=Output
OutputBaseFilename=rex-ruckus-{#AppVersion}-windows-x64-setup
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
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
Name: "{group}\Rex Ruckus Meltdown"; Filename: "{app}\rex-ruckus.exe"; WorkingDir: "{app}"
Name: "{group}\Uninstall Rex Ruckus"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Rex Ruckus Meltdown"; Filename: "{app}\rex-ruckus.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\rex-ruckus.exe"; Description: "{cm:LaunchProgram,Rex Ruckus: Meltdown}"; Flags: nowait postinstall skipifsilent
