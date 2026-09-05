<#
  Read the board console.

  On nucleo_f767zi the Zephyr console is usart3 (PD8/PD9), wired to the onboard
  ST-Link's virtual COM port - so this is a plain serial read, with no USB
  device stack running on the target and nothing lost before enumeration. That
  is the main practical win over the WeAct board's USB CDC ACM console.

  Usage:  .\console.ps1 [-Port COM5] [-Baud 115200] [-Seconds 15]
          Omit -Port to auto-pick the ST-Link VCP.
#>
param(
    [string]$Port,
    [int]$Baud    = 115200,
    [int]$Seconds = 15
)

$ErrorActionPreference = 'Stop'

if (-not $Port) {
    $vcp = Get-CimInstance Win32_PnPEntity |
           Where-Object { $_.Name -match 'STLink|ST-Link|STMicroelectronics' -and $_.Name -match '\(COM\d+\)' } |
           Select-Object -First 1
    if (-not $vcp) { throw "No ST-Link virtual COM port found. Is the board plugged into CN1?" }
    $Port = [regex]::Match($vcp.Name, '\((COM\d+)\)').Groups[1].Value
    Write-Host "using $Port  ($($vcp.Name))"
}

$sp = New-Object System.IO.Ports.SerialPort $Port, $Baud, 'None', 8, 'One'
$sp.ReadTimeout = 500
$sp.Open()
try {
    Write-Host "--- reading $Port for ${Seconds}s (reset the board to see boot output) ---"
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        try { $line = $sp.ReadLine(); Write-Host $line }
        catch [TimeoutException] { }
    }
} finally { $sp.Close() }
