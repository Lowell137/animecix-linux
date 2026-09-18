' AnimeciX gizli baslatici - konsol penceresi acmaz
' Installer tarafindan {app}\AnimeciX.vbs olarak kurulur.
Dim sh, appDir, msysBin
Set sh = CreateObject("WScript.Shell")
appDir = CreateObject("Scripting.FileSystemObject").GetParentFolderName(WScript.ScriptFullName)
msysBin = "C:\msys64\ucrt64\bin"
Dim fso
Set fso = CreateObject("Scripting.FileSystemObject")
If fso.FolderExists(msysBin) Then
  sh.Environment("PROCESS")("PATH") = msysBin & ";" & sh.Environment("PROCESS")("PATH")
End If
sh.CurrentDirectory = appDir
sh.Run """" & appDir & "\animecix.exe""", 0, False
