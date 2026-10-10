param(
    [int]$ProcessId,
    [string]$Action,          # click | double | rclick | wheel | ctrlwheel | drag | move | down | up
    [int]$X, [int]$Y,         # window-relative pixels (same frame as shot.ps1)
    [int]$X2 = 0, [int]$Y2 = 0,
    [int]$Amount = 120        # wheel delta (120 = one notch; negative = down)
)
Add-Type @"
using System; using System.Runtime.InteropServices;
public class M {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, int data, IntPtr extra);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, IntPtr extra);
  public struct RECT { public int L, T, R, B; }
}
"@
[M]::SetProcessDPIAware() | Out-Null
$h = (Get-Process -Id $ProcessId).MainWindowHandle
[M]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 200
$r = New-Object M+RECT; [M]::GetWindowRect($h, [ref]$r) | Out-Null
$sx = $r.L + $X; $sy = $r.T + $Y
$LEFTDOWN = 0x2; $LEFTUP = 0x4; $RIGHTDOWN = 0x8; $RIGHTUP = 0x10; $WHEEL = 0x800; $CTRL = 0x11; $KEYUP = 0x2
[M]::SetCursorPos($sx, $sy) | Out-Null
Start-Sleep -Milliseconds 150
switch ($Action) {
  "click"  { [M]::mouse_event($LEFTDOWN,0,0,0,[IntPtr]::Zero); [M]::mouse_event($LEFTUP,0,0,0,[IntPtr]::Zero) }
  "rclick" { [M]::mouse_event($RIGHTDOWN,0,0,0,[IntPtr]::Zero); [M]::mouse_event($RIGHTUP,0,0,0,[IntPtr]::Zero) }
  "double" { for ($i = 0; $i -lt 2; $i++) { [M]::mouse_event($LEFTDOWN,0,0,0,[IntPtr]::Zero); [M]::mouse_event($LEFTUP,0,0,0,[IntPtr]::Zero); Start-Sleep -Milliseconds 60 } }
  "wheel"  { [M]::mouse_event($WHEEL,0,0,$Amount,[IntPtr]::Zero) }
  "move"   { }
  "down"   { [M]::mouse_event($LEFTDOWN,0,0,0,[IntPtr]::Zero) }
  "up"     { [M]::mouse_event($LEFTUP,0,0,0,[IntPtr]::Zero) }
  "ctrlwheel" {
      [M]::keybd_event($CTRL,0,0,[IntPtr]::Zero)
      Start-Sleep -Milliseconds 50
      [M]::mouse_event($WHEEL,0,0,$Amount,[IntPtr]::Zero)
      Start-Sleep -Milliseconds 100
      [M]::keybd_event($CTRL,0,$KEYUP,[IntPtr]::Zero)
  }
  "drag" {
      [M]::mouse_event($LEFTDOWN,0,0,0,[IntPtr]::Zero)
      $steps = 12
      for ($i = 1; $i -le $steps; $i++) {
          [M]::SetCursorPos($sx + [int](($X2 - $X) * $i / $steps), $sy + [int](($Y2 - $Y) * $i / $steps)) | Out-Null
          Start-Sleep -Milliseconds 30
      }
      [M]::mouse_event($LEFTUP,0,0,0,[IntPtr]::Zero)
  }
}
Start-Sleep -Milliseconds 300
"$Action at $X,$Y"
