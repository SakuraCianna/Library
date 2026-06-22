param(
  [ValidateSet('tiny', 'small', 'medium')]
  [string]$Tier = 'medium',
  [string]$ModelDir = "$PSScriptRoot\..\models\ocr\pp-ocrv6",
  [string]$PythonPath = '',
  [string]$SmokeFile = '',
  [string]$SmokePdf = '',
  [int]$MaxPdfPages = 12,
  [int]$MaxImagePixels = 25000000,
  [switch]$SkipRuntime
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

function Resolve-PythonPath {
  param([string]$RequestedPath)

  if ($RequestedPath.Trim()) {
    if (-not (Test-Path -LiteralPath $RequestedPath -PathType Leaf)) {
      throw "Python path does not exist: $RequestedPath"
    }

    return (Resolve-Path -LiteralPath $RequestedPath).Path
  }

  $venvPython = Join-Path $root '.venv\Scripts\python.exe'
  if (Test-Path -LiteralPath $venvPython -PathType Leaf) {
    return $venvPython
  }

  $pyCommand = Get-Command py -ErrorAction SilentlyContinue
  if ($null -ne $pyCommand) {
    return $pyCommand.Source
  }

  return (Get-Command python -ErrorAction Stop).Source
}

$python = Resolve-PythonPath -RequestedPath $PythonPath
$modelPath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($ModelDir)
$checker = Join-Path $root 'sidecars\ocr\check_ocr_environment.py'
$arguments = @(
  $checker,
  '--model-dir',
  $modelPath,
  '--tier',
  $Tier,
  '--max-pdf-pages',
  $MaxPdfPages,
  '--max-image-pixels',
  $MaxImagePixels
)

if (-not $SkipRuntime) {
  $arguments += '--require-runtime'
}

$smokeInput = if ($SmokeFile.Trim()) { $SmokeFile } else { $SmokePdf }
if ($smokeInput.Trim()) {
  $arguments += '--smoke-file'
  $arguments += $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($smokeInput)
}

Write-Host "Python: $python"
Write-Host "Model dir: $modelPath"

& $python @arguments
if ($LASTEXITCODE -ne 0) {
  throw "OCR environment check failed"
}
