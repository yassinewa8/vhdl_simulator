# GUI test helpers (Windows, PowerShell)

Small scripts for checking the app's window by hand or from Claude Code:
start the app, act on it, take a screenshot, look at the screenshot.
They were used to verify every GUI feature in Phases 2 and 4.

| Script | What it does |
|---|---|
| `shot.ps1 -Out file.png -ProcessId N` | Screenshot of the app window (DPI-aware). |
| `press.ps1 -ProcessId N -Name "▶ Run"` | Presses a button through Windows UI Automation (egui exposes widgets via AccessKit), without moving the mouse. A name ending in `*` matches by prefix. Selectable labels are "toggled" instead of invoked. |
| `mouse.ps1 -ProcessId N -Action click -X 400 -Y 200` | Real mouse input at window-relative pixels (same coordinates as the screenshot): `click`, `double`, `rclick`, `wheel`, `ctrlwheel` (zoom), `drag` (to `-X2 -Y2`), `move`, `down`, `up`. Moves the user's pointer, so warn them first. |

Typical use:

```powershell
$p = Start-Process -FilePath .\target\debug\app.exe -ArgumentList "tests\fixtures\waves\types_tb.fst" -PassThru
Start-Sleep 4
.\scripts\gui-test\shot.ps1 -Out shot.png -ProcessId $p.Id
```

Notes:
- `app <file.fst>` opens a waveform directly; `app <project folder>` opens a project.
- Typing: `[System.Windows.Forms.SendKeys]::SendWait("text{ENTER}")` after
  clicking into a text field. Keys with modifiers: `keybd_event` (see
  `mouse.ps1`, e.g. hold Ctrl = 0x11 around a click).
- Double-clicks, drags and Ctrl+scroll need `mouse.ps1`; UI Automation
  can only press and toggle.
