#define AppName "Game Studio Crew"
#define AppVersion "0.1.0"
#define AppPublisher "tugcantopaloglu"
#define AppUrl "https://github.com/tugcantopaloglu/game-studio-crew"
#define ShellExe "game-studio.exe"
#define DaemonExe "studiod.exe"
#define WebView2Client "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

[Setup]
AppId={{4C9E0B8A-6D3F-4E21-9C77-2A5E1D0F8B34}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppSupportURL={#AppUrl}
AppUpdatesURL={#AppUrl}
DefaultDirName={autopf}\Game Studio Crew
DefaultGroupName=Game Studio Crew
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=out
OutputBaseFilename=game-studio-crew-{#AppVersion}-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\{#ShellExe}
ChangesEnvironment=no
ChangesAssociations=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "..\desktop\target\release\{#ShellExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\{#DaemonExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#ShellExe}"; Comment: "Open the studio floor"

[Run]
Filename: "{app}\{#ShellExe}"; Description: "Start {#AppName}"; Flags: nowait postinstall skipifsilent

[Code]
function WebView2IsInstalled(): Boolean;
begin
  Result := RegKeyExists(HKEY_LOCAL_MACHINE, 'SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{#WebView2Client}')
    or RegKeyExists(HKEY_LOCAL_MACHINE, 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{#WebView2Client}')
    or RegKeyExists(HKEY_CURRENT_USER, 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{#WebView2Client}');
end;

function InitializeSetup(): Boolean;
begin
  Result := True;
  if WebView2IsInstalled() then
    Exit;
  if WizardSilent() then
    Exit;
  Result := MsgBox(
    'The Microsoft Edge WebView2 runtime was not found. The studio window draws the floor with it,'
    + #13#10 + 'so the app will open an empty window until the runtime is installed. Windows 11 normally ships it.'
    + #13#10#13#10 + 'Install Game Studio Crew anyway?',
    mbConfirmation, MB_YESNO) = IDYES;
end;

procedure ReportRequirements();
var
  Report: AnsiString;
  ReportPath: String;
  Code: Integer;
begin
  if WizardSilent() then
    Exit;
  ReportPath := ExpandConstant('{tmp}\studio-doctor.txt');
  if not Exec(ExpandConstant('{cmd}'),
    '/C ""' + ExpandConstant('{app}\{#DaemonExe}') + '" doctor > "' + ReportPath + '""',
    '', SW_HIDE, ewWaitUntilTerminated, Code) then
    Exit;
  if Code <> 2 then
    Exit;
  if not LoadStringFromFile(ReportPath, Report) then
    Report := '';
  MsgBox('Game Studio Crew is installed, but there is nothing to code with yet.'
    + #13#10 + 'Install one coding CLI, put it on PATH, and it will pick it up.'
    + #13#10#13#10 + String(Report), mbError, MB_OK);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    ReportRequirements();
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  StudioData: String;
begin
  if CurUninstallStep <> usPostUninstall then
    Exit;
  if UninstallSilent() then
    Exit;
  StudioData := ExpandConstant('{localappdata}\GameStudioCrew');
  if not DirExists(StudioData) then
    Exit;
  if MsgBox('Also remove the studio''s own data at' + #13#10 + StudioData + '?'
    + #13#10#13#10 + 'That is the event log, the decision store, the daemon log and any crash reports.'
    + #13#10 + 'Your project folders live wherever you put them and are never touched.',
    mbConfirmation, MB_YESNO) = IDYES then
    DelTree(StudioData, True, True, True);
end;
