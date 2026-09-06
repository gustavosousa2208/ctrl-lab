<#
  Flash a ctrl-lab firmware build onto the board, from Windows.

  The build happens in WSL; the board's ST-Link enumerates on Windows. Rather
  than forwarding USB into WSL with usbipd, we just reach across the other way:
  WSL's filesystem is visible to Windows over a UNC share, so Windows can flash
  a hex that Linux produced. One less moving part.

  The Windows path comes from `wslpath -w` rather than being built here. That is
  not fussiness: a UNC prefix is two backslashes, and a literal pair of them does
  not survive every way this file might be generated or edited. Asking WSL is
  both shorter and immune to that.

  Usage:  .\flash.ps1 [-App bringup] [-Board nucleo_f767zi] [-Distro Ubuntu]
#>
param(
    [string]$App     = 'bringup',
    [string]$Board   = 'nucleo_f767zi',
    [string]$Variant = '',        # matches VARIANT= passed to build.sh
    [string]$Distro  = 'Ubuntu',
    [string]$WslUser = 'gusta'
)

$ErrorActionPreference = 'Stop'

$cli = 'C:\Program Files\STMicroelectronics\STM32Cube\STM32CubeProgrammer\bin\STM32_Programmer_CLI.exe'
if (-not (Test-Path $cli)) { throw "STM32CubeProgrammer CLI not found at $cli" }

$dir = if ($Variant) { "$Board-$Variant" } else { $Board }
$linuxHex = "/home/$WslUser/ctrl-lab-build/$App/$dir/zephyr/zephyr.hex"
$hex = (& wsl -d $Distro -- wslpath -w $linuxHex 2>$null | Select-Object -First 1)
if ($hex) { $hex = $hex.Trim() }
if (-not $hex -or -not (Test-Path $hex)) {
    throw "No build at ${Distro}:$linuxHex - run firmware/scripts/build.sh $App $Board first"
}

# Stage locally. CubeProgrammer is unreliable reading straight off the 9p share.
$staged = Join-Path $env:TEMP "ctrl-lab-$App-$Board.hex"
Copy-Item $hex $staged -Force
Write-Host "staged $([math]::Round((Get-Item $staged).Length/1KB,1)) KB -> $staged"

& $cli -c port=SWD mode=UR -w $staged -v -rst
if ($LASTEXITCODE -ne 0) { throw "flash failed (exit $LASTEXITCODE)" }
Write-Host "`nFlashed. Read the console with: .\console.ps1"
