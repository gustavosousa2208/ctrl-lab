<#
  Flash a ctrl-lab firmware build onto the board, from Windows.

  The build happens in WSL; the board's ST-Link enumerates on Windows. Rather
  than forwarding USB into WSL with usbipd, we just reach across the other way:
  WSL's filesystem is visible at \wsl.localhost, so Windows can flash a hex
  that Linux produced. One less moving part.

  Usage:  .\flash.ps1 [-App bringup] [-Board nucleo_f767zi] [-Distro Ubuntu]
#>
param(
    [string]$App    = 'bringup',
    [string]$Board  = 'nucleo_f767zi',
    [string]$Distro = 'Ubuntu',
    [string]$WslUser = 'gusta'
)

$ErrorActionPreference = 'Stop'

$cli = 'C:\Program Files\STMicroelectronics\STM32Cube\STM32CubeProgrammer\bin\STM32_Programmer_CLI.exe'
if (-not (Test-Path $cli)) { throw "STM32CubeProgrammer CLI not found at $cli" }

$hex = "\wsl.localhost\$Distro\home\$WslUser\ctrl-lab-build\$App\$Board\zephyr\zephyr.hex"
if (-not (Test-Path $hex)) {
    throw "No build at $hex - run firmware/scripts/build.sh $App $Board first"
}

# Stage locally. CubeProgrammer is unreliable reading straight off the 9p share.
$staged = Join-Path $env:TEMP "ctrl-lab-$App-$Board.hex"
Copy-Item $hex $staged -Force
Write-Host "staged $([math]::Round((Get-Item $staged).Length/1KB,1)) KB -> $staged"

& $cli -c port=SWD mode=UR -w $staged -v -rst
if ($LASTEXITCODE -ne 0) { throw "flash failed (exit $LASTEXITCODE)" }
Write-Host "`nFlashed. Read the console with: .\console.ps1"
