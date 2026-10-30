; BossKey 安装版脚本（Inno Setup 6）
; 由 scripts\release.ps1 调用：
;   ISCC.exe /DAppVersion=0.1.18 /DExePath=..\target\release\bosskey.exe installer\bosskey.iss
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef ExePath
  #define ExePath "..\target\release\bosskey.exe"
#endif

#define AppName "BossKey"
#define AppPublisher "brucevon"
#define AppUrl "https://github.com/brucevon/bosskey"

[Setup]
AppId={{8C1F2A6D-3B7E-4C2A-9D5E-0A1B2C3D4E5F}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} v{#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppUrl}
AppSupportURL={#AppUrl}
AppUpdatesURL={#AppUrl}/releases
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
LicenseFile=..\LICENSE
SetupIconFile=..\assets\bosskey.ico
UninstallDisplayIcon={app}\BossKey.exe
UninstallDisplayName={#AppName} v{#AppVersion}
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=BossKey_v{#AppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "附加任务："; Flags: unchecked

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; DestName: "BossKey.exe"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\BossKey.exe"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\BossKey.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\BossKey.exe"; Description: "启动 {#AppName}"; Flags: nowait postinstall skipifsilent
