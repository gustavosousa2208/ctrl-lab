<#
  Read the board console.

  On nucleo_f767zi the Zephyr console is usart3 (PD8/PD9), wired to the onboard
  ST-Link's virtual COM port - so this is a plain serial read, with no USB
  device stack running on the target and nothing lost before enumeration. That
  is the main practical win over the WeAct board's USB CDC ACM console.

  By default this RESETS the board after opening the port. The probe prints once
  at boot and then idles, so simply attaching to the port shows nothing at all -
  the output has already been and gone. Open first, then reset.

  Use mode=UR (connect under reset), not HOTPLUG: once main() returns the core
  idles in WFI and a hotplug attach fails with "Unable to read device id from
  ROM table". Under Reset holds NRST while connecting, so it always attaches.

  The default baud matches firmware/ctrl (CTRL_CONSOLE_BAUD). The bringup probe
  still runs at the board default, so read it with -Baud 115200.

  921600 was measured, not chosen: over five captures per rate, 115200 lost or
  mangled rows on most runs while 460800 and 921600 were clean every time. The
  loss is time-in-flight, not rate.

  Usage:  .\console.ps1 [-Port COM5] [-Baud 921600] [-Seconds 10] [-NoReset]
          Omit -Port to auto-pick the ST-Link VCP.
          -NoReset just listens, for firmware that prints continuously.
#>
param(
    [string]$Port,
    [int]$Baud    = 921600,
    [int]$Seconds = 10,
    [switch]$NoReset
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

$cli = 'C:\Program Files\STMicroelectronics\STM32Cube\STM32CubeProgrammer\bin\STM32_Programmer_CLI.exe'

$sp = New-Object System.IO.Ports.SerialPort $Port, $Baud, 'None', 8, 'One'
$sp.ReadTimeout = 400
$sp.Open()
try {
    if (-not $NoReset) {
        Start-Sleep -Milliseconds 400
        $sp.DiscardInBuffer()
        Write-Host "--- resetting board ---"
        # SWD and the VCP are separate USB interfaces on the ST-Link, so
        # resetting over one while reading the other is fine.
        Start-Process -FilePath $cli `
                      -ArgumentList '-c','port=SWD','mode=UR','-rst' `
                      -NoNewWindow -Wait `
                      -RedirectStandardOutput "$env:TEMP\ctrl-lab-reset.log"
    }

    Write-Host "--- reading $Port for ${Seconds}s ---"
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        try { Write-Host $sp.ReadLine() }
        catch [TimeoutException] { }
    }
} finally { $sp.Close() }
