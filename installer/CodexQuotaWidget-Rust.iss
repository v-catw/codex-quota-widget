#define RustVersion "0.2.9"
#define AppName "codex用量监测"
#define AppExe "CodexQuotaWidget-Rust.exe"
#define PublishDir "..\artifacts\rust\mascot-v" + RustVersion
#define IconFile "..\assets\codex-quota-logo.ico"

[Setup]
AppId={{F02BAAC7-F392-454F-855A-C8026EE451F8}
AppName={#AppName} (Rust)
AppVersion={#RustVersion}
AppMutex=Local\CodexQuotaWidgetRust
DefaultDirName={localappdata}\Programs\CodexQuotaWidgetRust
DefaultGroupName={#AppName} (Rust)
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
DisableProgramGroupPage=yes
MinVersion=10.0
OutputDir=..\artifacts\rust\installer
OutputBaseFilename=CodexQuotaWidget-Rust-{#RustVersion}-Setup-x64
SetupIconFile={#IconFile}
UninstallDisplayIcon={app}\{#AppExe}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ShowLanguageDialog=yes

[Languages]
Name: "chinese"; MessagesFile: "Languages\ChineseSimplified.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
chinese.WatchTask=登录 Windows 后等待 Codex 启动
english.WatchTask=Wait for Codex after Windows sign in

[Tasks]
Name: "watch"; Description: "{cm:WatchTask}"; Flags: unchecked

[Files]
Source: "{#PublishDir}\CodexQuotaWidget-Rust-{#RustVersion}-x64.exe"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName} (Rust)"; Filename: "{app}\{#AppExe}"
Name: "{userstartup}\CodexQuotaWidgetRust"; Filename: "{app}\{#AppExe}"; Parameters: "--watch"; Tasks: watch

[InstallDelete]
Type: files; Name: "{userstartup}\CodexQuotaWidgetRust.lnk"; Tasks: not watch

[Run]
Filename: "{app}\{#AppExe}"; Parameters: "--watch"; Description: "{cm:LaunchProgram,{#AppName}}"; Tasks: watch; Flags: nowait postinstall skipifsilent
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Tasks: not watch; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: files; Name: "{localappdata}\CodexQuotaWidgetRust\settings.json"
Type: dirifempty; Name: "{localappdata}\CodexQuotaWidgetRust"
