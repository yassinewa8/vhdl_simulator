param([int]$ProcessId, [string]$Name)
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::RootElement
$cond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $ProcessId)
$win = $root.FindFirst([System.Windows.Automation.TreeScope]::Children, $cond)
$nameCond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $Name)
if ($Name.EndsWith("*")) { $nameCond = [System.Windows.Automation.Condition]::TrueCondition }
# accesskit builds its tree lazily; retry for a moment
for ($i = 0; $i -lt 20; $i++) {
  if ($Name.EndsWith("*")) { $prefix = $Name.TrimEnd("*"); $el = $win.FindAll([System.Windows.Automation.TreeScope]::Descendants, $nameCond) | Where-Object { $_.Current.Name.StartsWith($prefix) } | Select-Object -First 1 } else { $el = $win.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $nameCond) }
  if ($el) { break }
  Start-Sleep -Milliseconds 250
}
if (-not $el) { "not found: $Name"; exit 1 }
$o = $null; if ($el.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$o)) { $o.Invoke() } else { $el.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle() }
"pressed $Name"
