param(
    [string]$MakeNsis = 'makensis.exe',
    [string]$InstallerHooks = (Join-Path $PSScriptRoot 'installer-hooks.nsh')
)

$ErrorActionPreference = 'Stop'
$productName = 'MangoDiskInstallerTest-' + [guid]::NewGuid().ToString('N')
$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) $productName
$urlKey = "Software\https://github.com/harry0703\$productName"
$originalKey = "Software\harry0703\$productName"
$utf8 = New-Object System.Text.UTF8Encoding($true)

# Compile and execute the production hook with isolated registry keys. These
# fixtures never read or modify real MangoDisk installation records.
function ConvertTo-NsisString([string]$Value) {
    return $Value.Replace('$', '$$').Replace('"', '$\"')
}

try {
    New-Item -ItemType Directory -Path $testRoot | Out-Null
    $current = Join-Path $testRoot 'current install'
    $legacyUrl = Join-Path $testRoot 'url publisher'
    $legacyOriginal = Join-Path $testRoot 'original publisher'
    $missing = Join-Path $testRoot 'missing executable'
    $deleted = Join-Path $testRoot 'deleted directory'
    foreach ($directory in @($current, $legacyUrl, $legacyOriginal, $missing)) {
        New-Item -ItemType Directory -Path $directory | Out-Null
    }
    foreach ($directory in @($current, $legacyUrl, $legacyOriginal)) {
        [System.IO.File]::WriteAllText((Join-Path $directory 'MangoDisk.exe'), 'fixture')
    }

    $cases = @(
        @{ Name = 'current-before-legacy'; Mode = 1; Selected = $current; Url = $legacyUrl; Original = $legacyOriginal; Expected = $current },
        @{ Name = 'current-without-registry'; Mode = 1; Selected = $current; Url = ''; Original = ''; Expected = $current },
        @{ Name = 'url-publisher-migration'; Mode = 1; Selected = $missing; Url = $legacyUrl; Original = $legacyOriginal; Expected = $legacyUrl },
        @{ Name = 'original-publisher-migration'; Mode = 1; Selected = $missing; Url = ''; Original = $legacyOriginal; Expected = $legacyOriginal },
        @{ Name = 'deleted-url-fallback'; Mode = 1; Selected = $missing; Url = $deleted; Original = $legacyOriginal; Expected = $legacyOriginal },
        @{ Name = 'url-without-executable-fallback'; Mode = 1; Selected = $missing; Url = $missing; Original = $legacyOriginal; Expected = $legacyOriginal },
        @{ Name = 'no-registry-records'; Mode = 1; Selected = $missing; Url = ''; Original = ''; Expected = $missing },
        @{ Name = 'both-legacy-paths-invalid'; Mode = 1; Selected = $missing; Url = $deleted; Original = $missing; Expected = $missing },
        @{ Name = 'interactive-custom-directory'; Mode = 0; Selected = $missing; Url = $legacyUrl; Original = $legacyOriginal; Expected = $missing },
        @{ Name = 'subsequent-update'; Mode = 1; Selected = $legacyUrl; Url = $legacyOriginal; Original = $current; Expected = $legacyUrl }
    )

    $source = @"
Unicode true
!include "LogicLib.nsh"
!define PRODUCTNAME "$productName"
!define MAINBINARYNAME "MangoDisk"
!include "$(ConvertTo-NsisString (Resolve-Path $InstallerHooks).Path)"
Name "$productName"
OutFile "$(ConvertTo-NsisString (Join-Path $testRoot 'test.exe'))"
RequestExecutionLevel user
SilentInstall silent
Var UpdateMode
Section
SetShellVarContext current
SetRegView 64
"@

    foreach ($case in $cases) {
        $source += @"

DeleteRegKey HKCU "$urlKey"
DeleteRegKey HKCU "$originalKey"
"@
        if ($case.Url) {
            $source += "`nWriteRegStr HKCU `"$urlKey`" `"`" `"$(ConvertTo-NsisString $case.Url)`""
        }
        if ($case.Original) {
            $source += "`nWriteRegStr HKCU `"$originalKey`" `"`" `"$(ConvertTo-NsisString $case.Original)`""
        }
        $source += @"

StrCpy `$UpdateMode $($case.Mode)
StrCpy `$INSTDIR "$(ConvertTo-NsisString $case.Selected)"
SetOutPath `$INSTDIR
!insertmacro NSIS_HOOK_PREINSTALL
FileOpen `$9 "$(ConvertTo-NsisString (Join-Path $testRoot ($case.Name + '.txt')))" w
FileWriteWord `$9 0xFEFF
FileWriteUTF16LE `$9 "`$INSTDIR`$\r`$\n`$OUTDIR`$\r`$\n"
FileClose `$9
"@
    }
    $source += @"

DeleteRegKey HKCU "$urlKey"
DeleteRegKey HKCU "$originalKey"
SectionEnd
"@
    $sourcePath = Join-Path $testRoot 'test.nsi'
    [System.IO.File]::WriteAllText($sourcePath, $source, $utf8)
    & $MakeNsis /V2 $sourcePath
    if ($LASTEXITCODE -ne 0) {
        throw "NSIS test compilation failed with exit code $LASTEXITCODE"
    }
    $process = Start-Process -FilePath (Join-Path $testRoot 'test.exe') -PassThru
    if (-not $process.WaitForExit(30000)) {
        $process.Kill()
        throw 'NSIS test execution exceeded 30 seconds'
    }
    if ($process.ExitCode -ne 0) {
        throw "NSIS test execution failed with exit code $($process.ExitCode)"
    }

    $failures = @()
    foreach ($case in $cases) {
        $result = @(Get-Content -LiteralPath (Join-Path $testRoot ($case.Name + '.txt')))
        if ($result.Count -ne 2 -or $result[0] -cne $case.Expected -or $result[1] -cne $case.Expected) {
            $failures += "$($case.Name): expected directory and output directory '$($case.Expected)', got '$($result -join ', ')'."
        } else {
            Write-Output "PASS $($case.Name)"
        }
    }
    if ($failures.Count -gt 0) {
        throw ($failures -join [Environment]::NewLine)
    }
    Write-Output "All $($cases.Count) NSIS installation directory scenarios passed."
} finally {
    foreach ($key in @($urlKey, $originalKey)) {
        if (Test-Path -LiteralPath "Registry::HKEY_CURRENT_USER\$key") {
            Remove-Item -LiteralPath "Registry::HKEY_CURRENT_USER\$key" -Recurse -Force
        }
    }
    if (Test-Path -LiteralPath $testRoot) {
        Remove-Item -LiteralPath $testRoot -Recurse -Force
    }
}
