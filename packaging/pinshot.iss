; Inno Setup script. Defines passed by package.py: AppVersion, Arch, SourceDir, OutputDir, OutputBase, IconFile
#define AppName "PinShot"
[Setup]
AppId={{8B3F7F2A-5E54-4B6D-9C57-3F0F1B1F6A11}
AppName={#AppName}
AppVersion={#AppVersion}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBase}
SetupIconFile={#IconFile}
UninstallDisplayIcon={app}\PinShot.exe
Compression=lzma2
SolidCompression=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
WizardStyle=modern
#if Arch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
[Tasks]
Name: "autostart"; Description: "Start PinShot when I sign in"; Flags: unchecked
[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: recursesubdirs ignoreversion
[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\PinShot.exe"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\PinShot.exe"; Tasks: autostart
[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "PinShot"; ValueData: """{app}\PinShot.exe"""; Tasks: autostart; Flags: uninsdeletevalue
[Run]
Filename: "{app}\PinShot.exe"; Description: "Launch PinShot"; Flags: nowait postinstall skipifsilent
