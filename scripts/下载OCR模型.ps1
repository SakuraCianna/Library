param(
  [ValidateSet('tiny', 'small', 'medium')]
  [string]$Tier = 'medium',
  [string]$TargetDir = "$PSScriptRoot\..\models\ocr\pp-ocrv6",
  [string]$PythonPath = '',
  [switch]$SkipExisting
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$targetItem = New-Item -ItemType Directory -Force -Path $TargetDir
$target = $targetItem.FullName
$requiredModelFiles = @('inference.json', 'inference.pdiparams', 'inference.yml')
$repos = @(
  "PaddlePaddle/PP-OCRv6_${Tier}_det",
  "PaddlePaddle/PP-OCRv6_${Tier}_rec"
)

function Resolve-PythonPath {
  param([string]$RequestedPath)

  if ($RequestedPath.Trim()) {
    if (-not (Test-Path -LiteralPath $RequestedPath -PathType Leaf)) {
      throw "指定的 Python 不存在: $RequestedPath"
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

function Test-ModelComplete {
  param([string]$ModelPath)

  if (-not (Test-Path -LiteralPath $ModelPath -PathType Container)) {
    return $false
  }

  foreach ($fileName in $requiredModelFiles) {
    if (-not (Test-Path -LiteralPath (Join-Path $ModelPath $fileName) -PathType Leaf)) {
      return $false
    }
  }

  return $true
}

$python = Resolve-PythonPath -RequestedPath $PythonPath

& $python -c "import huggingface_hub" 2>$null
if ($LASTEXITCODE -ne 0) {
  & $python -m pip install --upgrade huggingface_hub
}

try {
  foreach ($repo in $repos) {
    $name = ($repo -split '/')[1]
    $out = Join-Path $target $name
    if ($SkipExisting -and (Test-ModelComplete -ModelPath $out)) {
      Write-Host "已存在完整模型，跳过下载: $name"
      continue
    }

    $env:HF_REPO_ID = $repo
    $env:HF_LOCAL_DIR = $out
    & $python -c "import os; from huggingface_hub import snapshot_download; snapshot_download(repo_id=os.environ['HF_REPO_ID'], local_dir=os.environ['HF_LOCAL_DIR'], local_dir_use_symlinks=False)"
    if ($LASTEXITCODE -ne 0) {
      throw "OCR 模型下载失败: $repo"
    }
  }
}
finally {
  Remove-Item Env:\HF_REPO_ID -ErrorAction SilentlyContinue
  Remove-Item Env:\HF_LOCAL_DIR -ErrorAction SilentlyContinue
}

foreach ($repo in $repos) {
  $name = ($repo -split '/')[1]
  $out = Join-Path $target $name
  if (-not (Test-ModelComplete -ModelPath $out)) {
    throw "OCR 模型文件不完整: $name"
  }
}

Write-Host "OCR 模型已下载到: $target"
