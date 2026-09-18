; AnimeciX Windows Setup - Inno Setup 6 script
; README uyumlu: AppName/Version/Publisher/URL/LICENSE aynidir.
; Kaynak: https://github.com/Lowell137/animecix-linux
; Bu script windows-port branch'indedir, asset v1.3.1 release'ine eklenir.
; Gereksinim: MSYS2 UCRT64 runtime (libgtk-4, libadwaita, libmpv).

#define MyAppName "AnimeciX"
#define MyAppVersion "1.3.1"
#define MyAppPublisher "Lowell137"
#define MyAppURL "https://github.com/Lowell137/animecix-linux"
#define MyAppExeName "animecix.exe"

[Setup]
AppId={{8E2A4F1A-4B3C-4D5E-9F1A-ANIMECIX131}}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}/releases/latest
DefaultDirName={autopf}\AnimeciX
DefaultGroupName=AnimeciX
DisableProgramGroupPage=yes
LicenseFile=..\..\LICENSE
OutputDir=..\..\dist
OutputBaseFilename=AnimeciX-1.3.1-Windows-x86_64-Setup
Compression=lzma2/max
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
WizardStyle=modern
UninstallDisplayName=AnimeciX 1.3.1 (Windows port)

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "turkish"; MessagesFile: "compiler:Languages\Turkish.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "AnimeciX.vbs"; DestDir: "{app}"; Flags: ignoreversion
Source: "AnimeciX.bat"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\assets\*"; DestDir: "{app}\assets"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion isreadme

[Icons]
Name: "{group}\AnimeciX"; Filename: "{sys}\wscript.exe"; Parameters: """{app}\AnimeciX.vbs"""; WorkingDir: "{app}"; Comment: "AnimeciX - Turkce anime izleme (Windows port)"
Name: "{group}\AnimeciX (Konsollu)"; Filename: "{app}\AnimeciX.bat"; WorkingDir: "{app}"; Comment: "AnimeciX hata ayiklama (konsol acar)"
Name: "{autodesktop}\AnimeciX"; Filename: "{sys}\wscript.exe"; Parameters: """{app}\AnimeciX.vbs"""; WorkingDir: "{app}"; Tasks: desktopicon; Comment: "AnimeciX - Turkce anime izleme (Windows port)"

[Run]
Filename: "{sys}\wscript.exe"; Parameters: """{app}\AnimeciX.vbs"""; Description: "{cm:LaunchProgram,AnimeciX}"; Flags: nowait postinstall skipifsilent shellexec

[Code]
function MsysRuntimeOK(): Boolean;
begin
  Result := FileExists('C:\msys64\ucrt64\bin\libgtk-4-1.dll');
end;

function InitializeSetup(): Boolean;
var
  Res: Integer;
begin
  Result := True;
  if not MsysRuntimeOK() then
  begin
    Res := SuppressibleMsgBox(
      'MSYS2 UCRT64 runtime bulunamadi.' + #13#10 + #13#10 +
      'AnimeciX Windows surumu GTK4/Libadwaita/libmpv icin su dosyalari ister:' + #13#10 +
      'C:\msys64\ucrt64\bin\libgtk-4-1.dll' + #13#10 + #13#10 +
      'Kurulum adimlari (README Kurulum > Windows bolumune bakin):' + #13#10 +
      '1) https://www.msys2.org adresinden MSYS2 kurun' + #13#10 +
      '2) UCRT64 terminalde calistirin:' + #13#10 +
      'pacman -S mingw-w64-ucrt-x86_64-gtk4 mingw-w64-ucrt-x86_64-libadwaita mingw-w64-ucrt-x86_64-mpv' + #13#10 + #13#10 +
      'Simdi kuruluma devam edilsin mi?',
      mbConfirmation, MB_YESNO, IDYES);
    if Res <> IDYES then
      Result := False;
  end;
end;
