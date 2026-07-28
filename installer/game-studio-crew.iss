#define AppName "Game Studio Crew"
#define AppVersion "1.0.0"
#define AppPublisher "tugcantopaloglu"
#define AppUrl "https://github.com/tugcantopaloglu/game-studio-crew"
#define ShellExe "game-studio.exe"
#define DaemonExe "studiod.exe"
#define DaemonPdb "studiod.pdb"
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
SetupIconFile=..\desktop\assets\icon.ico
WizardImageFile=assets\wizard-page.bmp,assets\wizard-page-2x.bmp
WizardSmallImageFile=assets\wizard-badge.bmp,assets\wizard-badge-2x.bmp
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\{#ShellExe}
ChangesEnvironment=no
ChangesAssociations=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "..\desktop\target\release\{#ShellExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\{#DaemonExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\{#DaemonPdb}"; DestDir: "{app}"; Flags: ignoreversion

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

procedure OfferToFixTheArtPipeline(Report: String);
var
  Code: Integer;
begin
  if MsgBox('Game Studio Crew is installed and ready to code.'
    + #13#10#13#10 + 'What it cannot do yet is generate its own art: the crew draws sprites and'
    + #13#10 + 'textures with codex and turns them into rigged models, and some of what that'
    + #13#10 + 'needs is missing. The studio works without it; every task just builds art by hand.'
    + #13#10#13#10 + Report
    + #13#10 + 'Install the missing pieces now? The studio will run the commands listed above'
    + #13#10 + 'and show you what happens. You can also do it later with: studiod doctor --fix',
    mbConfirmation, MB_YESNO) <> IDYES then
    Exit;

  if not Exec(ExpandConstant('{cmd}'),
    '/C ""' + ExpandConstant('{app}\{#DaemonExe}') + '" doctor --fix --yes & pause"',
    '', SW_SHOW, ewWaitUntilTerminated, Code) then
    MsgBox('The studio could not start its own installer step.'
      + #13#10 + 'Open a terminal and run: studiod doctor --fix', mbError, MB_OK)
  else
    MsgBox('Done. Signing codex in is the one step only you can do:'
      + #13#10#13#10 + '    codex login'
      + #13#10#13#10 + 'Run studiod doctor afterwards to see what is left.', mbInformation, MB_OK);
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
  if (Code <> 2) and (Code <> 3) and (Code <> 4) then
    Exit;
  if not LoadStringFromFile(ReportPath, Report) then
    Report := '';
  if Code = 2 then
    MsgBox('Game Studio Crew is installed, but there is nothing to code with yet.'
      + #13#10 + 'Install one coding CLI, put it on PATH, and it will pick it up.'
      + #13#10#13#10 + String(Report), mbError, MB_OK)
  else if Code = 3 then
    MsgBox('Game Studio Crew is installed and it found a coding CLI, but it cannot drive'
      + #13#10 + 'any of the ones you have, so no worker can start yet. The report below says'
      + #13#10 + 'why for each. Install Claude Code and put claude on PATH to fix it.'
      + #13#10#13#10 + String(Report), mbError, MB_OK)
  else
    OfferToFixTheArtPipeline(Report);
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
