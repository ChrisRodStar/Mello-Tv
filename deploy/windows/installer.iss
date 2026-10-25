; Inno Setup Script for Stream TV Media Server
; Builds Stream-Setup-x64.exe

#define MyAppName "Stream TV"
#define MyAppVersion "2.0.0"
#define MyAppPublisher "Stream TV"
#define MyAppURL "https://github.com/stream-tv"
#define MyAppExeName "stream-server.exe"

[Setup]
AppId={{D37E5796-8A05-49C9-7517-D4C8198EBF57}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName={autopf}\Stream TV
DefaultGroupName={#MyAppName}
AllowNoIcons=yes
OutputDir=..\..\dist\windows
OutputBaseFilename=Stream-Setup-{#MyAppVersion}-x64
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "startupicon"; Description: "Start Stream TV automatically at Windows login"; GroupDescription: "Windows Startup:"

[Files]
Source: "..\..\target\release\stream-server.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\web\dist\*"; DestDir: "{app}\web"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\..\README.md"; DestDir: "{app}"; Flags: isreadme

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Parameters: "--static-dir ""{app}\web"""
Name: "{group}\Open Stream Web App"; Filename: "http://localhost:8080"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Parameters: "--static-dir ""{app}\web"""; Tasks: desktopicon
Name: "{userstartup}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Parameters: "--static-dir ""{app}\web"""; Tasks: startupicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Parameters: "--static-dir ""{app}\web"""; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent
Filename: "http://localhost:8080"; Description: "Open Stream in your web browser"; Flags: shellexec postinstall skipifsilent
