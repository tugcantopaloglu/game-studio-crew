#define AppName "Game Studio Crew"
#define AppVersion "1.0.0"
#define AppPublisher "tugcantopaloglu"
#define AppUrl "https://github.com/tugcantopaloglu/game-studio-crew"
#define ShellExe "game-studio.exe"
#define DaemonExe "studiod.exe"
#define DaemonPdb "studiod.pdb"
#define WebView2Client "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

#ifndef DaemonDir
  #define DaemonDir "..\target\release"
#endif
#ifndef ShellDir
  #define ShellDir "..\desktop\target\release"
#endif

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
Source: "{#ShellDir}\{#ShellExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#DaemonDir}\{#DaemonExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#DaemonDir}\{#DaemonPdb}"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#DaemonDir}\{#DaemonExe}"; DestName: "doctor-probe.exe"; Flags: dontcopy noencryption

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#ShellExe}"; Comment: "Open the studio floor"

[Run]
Filename: "{app}\{#ShellExe}"; Description: "Start {#AppName}"; Flags: nowait postinstall skipifsilent

[Code]
const
  ROW_HEIGHT = 19;
  MAX_ROWS = 40;

type
  TFinding = record
    Group: String;
    Title: String;
    State: String;
    Detail: String;
    Fix: String;
    Advice: String;
    Ticked: String;
  end;

var
  RequirementsPage: TWizardPage;
  Findings: array[0..MAX_ROWS - 1] of TFinding;
  FindingCount: Integer;
  FixBoxes: array[0..MAX_ROWS - 1] of TNewCheckBox;
  FixBoxCount: Integer;
  FixTargets: array[0..MAX_ROWS - 1] of Integer;
  DetailLabel: TNewMemo;

function Field(var Line: String): String;
var
  At: Integer;
begin
  At := Pos(#9, Line);
  if At = 0 then begin
    Result := Line;
    Line := '';
  end else begin
    Result := Copy(Line, 1, At - 1);
    Line := Copy(Line, At + 1, Length(Line));
  end;
end;

function GroupHeading(Group: String): String;
begin
  if Group = 'cli' then
    Result := 'Coding CLI  (the studio needs one it can drive)'
  else if Group = 'toolchain' then
    Result := 'Also on this machine  (optional; install these yourself)'
  else if Group = 'engine' then
    Result := ''
  else
    Result := 'Art pipeline  (optional; the crew draws its own sprites with these)';
end;

procedure ProbeRequirements();
var
  ProbePath, ReportPath, Text, Line: String;
  Lines: TArrayOfString;
  At, Code: Integer;
begin
  FindingCount := 0;
  ExtractTemporaryFile('doctor-probe.exe');
  ProbePath := ExpandConstant('{tmp}\doctor-probe.exe');
  ReportPath := ExpandConstant('{tmp}\doctor.tsv');

  if not Exec(ExpandConstant('{cmd}'),
    '/C ""' + ProbePath + '" doctor --porcelain > "' + ReportPath + '""',
    '', SW_HIDE, ewWaitUntilTerminated, Code) then
    Exit;

  if not LoadStringsFromFile(ReportPath, Lines) then
    Exit;

  for At := 0 to GetArrayLength(Lines) - 1 do begin
    Line := Trim(Lines[At]);
    if (Line <> '') and (FindingCount < MAX_ROWS) then begin
      Findings[FindingCount].Group := Field(Line);
      Findings[FindingCount].Title := Field(Line);
      Findings[FindingCount].State := Field(Line);
      Findings[FindingCount].Detail := Field(Line);
      Findings[FindingCount].Fix := Field(Line);
      Findings[FindingCount].Advice := Field(Line);
      Findings[FindingCount].Ticked := Field(Line);
      FindingCount := FindingCount + 1;
    end;
  end;
end;

function Shorten(Text: String; Limit: Integer): String;
begin
  if Length(Text) <= Limit then
    Result := Text
  else
    Result := Copy(Text, 1, Limit - 1) + #$2026;
end;

procedure ShowDetailFor(Index: Integer);
var
  Says: String;
begin
  if DetailLabel = nil then
    Exit;
  Says := Findings[Index].Detail;
  if (Says = '') and (Findings[Index].Advice <> '') then
    Says := Findings[Index].Advice;
  if Says = '' then
    Says := Findings[Index].Title + ' is ready.';
  if Findings[Index].Fix <> '' then
    Says := Says + #13#10 + 'runs:  ' + Findings[Index].Fix;
  DetailLabel.Text := Says;
end;

procedure FixBoxClicked(Sender: TObject);
var
  At: Integer;
begin
  for At := 0 to FixBoxCount - 1 do
    if FixBoxes[At] = Sender then
      ShowDetailFor(FixTargets[At]);
end;

function SummaryOf(Group: String): String;
var
  At: Integer;
  Have, Missing: String;
begin
  Have := '';
  Missing := '';
  for At := 0 to FindingCount - 1 do begin
    if Findings[At].Group = Group then begin
      if Findings[At].State = 'ok' then begin
        if Have <> '' then Have := Have + ', ';
        Have := Have + Findings[At].Title;
      end else begin
        if Missing <> '' then Missing := Missing + ', ';
        Missing := Missing + Findings[At].Title;
      end;
    end;
  end;

  if Have = '' then
    Result := #$2715 + '  none found'
  else
    Result := #$2713 + '  ' + Have;
  if Missing <> '' then
    Result := Result + '      not found: ' + Missing;
end;

procedure AddRow(var Y: Integer; Index: Integer);
var
  RowH, NameW, Mark: Integer;
  Box: TNewCheckBox;
  Name, Detail: TNewStaticText;
begin
  RowH := ScaleY(18);
  NameW := ScaleX(150);

  if Findings[Index].Fix <> '' then begin
    Box := TNewCheckBox.Create(RequirementsPage);
    Box.Parent := RequirementsPage.Surface;
    Box.Top := Y;
    Box.Left := ScaleX(10);
    Box.Width := NameW;
    Box.Height := RowH;
    Box.Caption := Findings[Index].Title;
    Box.Checked := (Findings[Index].State <> 'ok') and (Findings[Index].Ticked = 'on');
    Box.OnClick := @FixBoxClicked;
    FixBoxes[FixBoxCount] := Box;
    FixTargets[FixBoxCount] := Index;
    FixBoxCount := FixBoxCount + 1;
  end else begin
    Name := TNewStaticText.Create(RequirementsPage);
    Name.Parent := RequirementsPage.Surface;
    Name.AutoSize := False;
    Name.Top := Y + ScaleY(2);
    Name.Left := ScaleX(10);
    Name.Width := NameW;
    Name.Height := RowH;
    if Findings[Index].State = 'ok' then
      Mark := 1
    else
      Mark := 0;
    if Mark = 1 then
      Name.Caption := #$2713 + '  ' + Findings[Index].Title
    else if Findings[Index].State = 'unusable' then
      Name.Caption := '!  ' + Findings[Index].Title
    else
      Name.Caption := #$2715 + '  ' + Findings[Index].Title;
  end;

  Detail := TNewStaticText.Create(RequirementsPage);
  Detail.Parent := RequirementsPage.Surface;
  Detail.AutoSize := False;
  Detail.Top := Y + ScaleY(2);
  Detail.Left := ScaleX(10) + NameW + ScaleX(8);
  Detail.Width := RequirementsPage.SurfaceWidth - Detail.Left;
  Detail.Height := RowH;
  if (Findings[Index].State = 'ok') and (Findings[Index].Fix <> '') then
    Detail.Caption := Shorten(Findings[Index].Detail, 48) + '   (reinstall)'
  else
    Detail.Caption := Shorten(Findings[Index].Detail, 60);

  Y := Y + RowH;
end;

procedure AddSummaryRow(var Y: Integer; Group: String);
var
  Row: TNewStaticText;
begin
  Row := TNewStaticText.Create(RequirementsPage);
  Row.Parent := RequirementsPage.Surface;
  Row.AutoSize := False;
  Row.Top := Y + ScaleY(2);
  Row.Left := ScaleX(10);
  Row.Width := RequirementsPage.SurfaceWidth - ScaleX(10);
  Row.Height := ScaleY(18);
  Row.Caption := Shorten(SummaryOf(Group), 86);
  Y := Y + ScaleY(18);
end;

procedure AddHeading(var Y: Integer; Group: String; First: Boolean);
var
  Heading: TNewStaticText;
begin
  if not First then
    Y := Y + ScaleY(9);
  Heading := TNewStaticText.Create(RequirementsPage);
  Heading.Parent := RequirementsPage.Surface;
  Heading.AutoSize := False;
  Heading.Top := Y;
  Heading.Left := 0;
  Heading.Width := RequirementsPage.SurfaceWidth;
  Heading.Height := ScaleY(20);
  Heading.Font.Style := [fsBold];
  Heading.Caption := GroupHeading(Group);
  Y := Y + ScaleY(20);
end;

procedure BuildRequirementsPage();
var
  At, Y, DetailTop, DetailHeight: Integer;
begin
  Y := 0;
  FixBoxCount := 0;

  AddHeading(Y, 'cli', True);
  for At := 0 to FindingCount - 1 do
    if Findings[At].Group = 'cli' then
      AddRow(Y, At);

  AddHeading(Y, 'art', False);
  for At := 0 to FindingCount - 1 do
    if Findings[At].Group = 'art' then
      AddRow(Y, At);

  AddHeading(Y, 'toolchain', False);
  AddSummaryRow(Y, 'toolchain');
  AddSummaryRow(Y, 'engine');

  DetailHeight := ScaleY(42);
  DetailTop := RequirementsPage.SurfaceHeight - DetailHeight;
  if DetailTop < Y + ScaleY(8) then begin
    DetailTop := Y + ScaleY(8);
    DetailHeight := RequirementsPage.SurfaceHeight - DetailTop;
  end;
  if DetailHeight < ScaleY(22) then
    DetailHeight := ScaleY(22);

  DetailLabel := TNewMemo.Create(RequirementsPage);
  DetailLabel.Parent := RequirementsPage.Surface;
  DetailLabel.Top := DetailTop;
  DetailLabel.Left := 0;
  DetailLabel.Width := RequirementsPage.SurfaceWidth;
  DetailLabel.Height := DetailHeight;
  DetailLabel.ReadOnly := True;
  DetailLabel.WordWrap := True;
  DetailLabel.ScrollBars := ssVertical;
  DetailLabel.TabStop := False;
  DetailLabel.Color := clBtnFace;
  DetailLabel.BorderStyle := bsNone;
  DetailLabel.Text := 'Tick a box to have the studio install it after setup finishes.'
    + #13#10 + 'Nothing here blocks the install; the studio runs without every optional line.';
end;

procedure RunTickedFixes();
var
  At, Code, Ran: Integer;
  Command, AlreadyRun: String;
begin
  Ran := 0;
  AlreadyRun := '';
  for At := 0 to FixBoxCount - 1 do begin
    if FixBoxes[At].Checked then begin
      Command := Findings[FixTargets[At]].Fix;
      if (Command <> '') and (Pos(#10 + Command + #10, AlreadyRun) = 0) then begin
        AlreadyRun := AlreadyRun + #10 + Command + #10;
        Exec(ExpandConstant('{cmd}'), '/C "' + Command + '"', '',
          SW_SHOW, ewWaitUntilTerminated, Code);
        Ran := Ran + 1;
      end;
    end;
  end;

  if Ran > 0 then
    MsgBox('Installed what you ticked.' + #13#10#13#10
      + 'Signing codex in is the one step only you can do:' + #13#10
      + '    codex login' + #13#10#13#10
      + 'Run studiod doctor afterwards to see what is left.', mbInformation, MB_OK);
end;

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

procedure InitializeWizard();
begin
  RequirementsPage := CreateCustomPage(wpSelectDir,
    'What the studio found on this machine',
    'Ticked boxes are installed after the studio is. Untick anything you would rather do yourself.');
  ProbeRequirements();
  BuildRequirementsPage();
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and not WizardSilent() then
    RunTickedFixes();
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
