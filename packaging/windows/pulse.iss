; Pulse's Windows installer (Inno Setup 6). Built by scripts/package-windows.ps1,
; which passes /DAppVersion=<version> and /DStage=<folder with pulse.exe>.
;
; Installs for the current user, without administrator rights, in
; %LOCALAPPDATA%\Programs\Pulse (the updater replaces pulse.exe there), with a
; Start menu entry, an optional desktop one and .pulse files opening in Pulse.
; Installing again, or a newer version, goes over the same installation. The
; user's projects (Music\Pulse) and settings (%APPDATA%\Pulse,
; %LOCALAPPDATA%\Pulse) are never touched, not even when uninstalling.

#ifndef AppVersion
  #error Falta /DAppVersion=<versión>
#endif
#ifndef Stage
  #define Stage "..\..\dist\Pulse"
#endif

[Setup]
AppId={{6F1C9B52-3E4A-4D7B-9C21-5A8E0D4F7B13}
AppName=Pulse
AppVersion={#AppVersion}
AppVerName=Pulse {#AppVersion}
AppPublisher=David Fernández Morilla
AppCopyright=Copyright © 2026 David Fernández Morilla
VersionInfoVersion={#AppVersion}
DefaultDirName={autopf}\Pulse
DefaultGroupName=Pulse
DisableProgramGroupPage=yes
; Only for this user: in Program Files the updater couldn't replace pulse.exe
; (it runs without administrator rights), so that choice isn't offered.
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
OutputBaseFilename=Pulse-{#AppVersion}-windows-x64-setup
SetupIconFile=pulse.ico
UninstallDisplayIcon={app}\pulse.exe
UninstallDisplayName=Pulse
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Stage}\pulse.exe"; DestDir: "{app}"; Flags: ignoreversion
; The Visual C++ runtime, beside pulse.exe (app-local, as Microsoft allows).
Source: "{#Stage}\*.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Stage}\THIRD_PARTY_NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Stage}\LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Pulse"; Filename: "{app}\pulse.exe"
Name: "{autodesktop}\Pulse"; Filename: "{app}\pulse.exe"; Tasks: desktopicon

[Registry]
; .pulse projects open in Pulse (HKA: the user's classes, or the machine's when
; installed for everyone).
Root: HKA; Subkey: "Software\Classes\.pulse"; ValueType: string; ValueName: ""; ValueData: "Pulse.Proyecto"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Pulse.Proyecto"; ValueType: string; ValueName: ""; ValueData: "Proyecto de Pulse"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Pulse.Proyecto\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\pulse.exe,0"
Root: HKA; Subkey: "Software\Classes\Pulse.Proyecto\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\pulse.exe"" ""%1"""

[UninstallDelete]
; What the updater leaves beside pulse.exe: the previous version and its work folder.
Type: files; Name: "{app}\pulse (anterior).exe"
Type: files; Name: "{app}\.pulse-cambio.exe"
Type: filesandordirs; Name: "{app}\.pulse-update"
Type: files; Name: "{app}\*.dll.anterior"

[Run]
Filename: "{app}\pulse.exe"; Description: "{cm:LaunchProgram,Pulse}"; Flags: nowait postinstall skipifsilent
