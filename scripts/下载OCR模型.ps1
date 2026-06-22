param(
  [ValidateSet('tiny', 'small', 'medium')]
  [string]$Tier = 'medium',
  [string]$TargetDir = "$PSScriptRoot\..\models\ocr\pp-ocrv6"
)

$ErrorActionPreference = 'Stop'
$targetItem = New-Item -ItemType Directory -Force -Path $TargetDir
$target = $targetItem.FullName
$repos = @(
  "PaddlePaddle/PP-OCRv6_${Tier}_det",
  "PaddlePaddle/PP-OCRv6_${Tier}_rec"
)
$pythonCommand = Get-Command py -ErrorAction SilentlyContinue

if ($null -eq $pythonCommand) {
  $pythonCommand = Get-Command python -ErrorAction Stop
}

& $pythonCommand.Source -c "import huggingface_hub" 2>$null
if ($LASTEXITCODE -ne 0) {
  & $pythonCommand.Source -m pip install --upgrade huggingface_hub
}

try {
  foreach ($repo in $repos) {
    $name = ($repo -split '/')[1]
    $out = Join-Path $target $name
    $env:HF_REPO_ID = $repo
    $env:HF_LOCAL_DIR = $out
    & $pythonCommand.Source -c "import os; from huggingface_hub import snapshot_download; snapshot_download(repo_id=os.environ['HF_REPO_ID'], local_dir=os.environ['HF_LOCAL_DIR'], local_dir_use_symlinks=False)"
    if ($LASTEXITCODE -ne 0) {
      throw "OCR 模型下载失败: $repo"
    }
  }
}
finally {
  Remove-Item Env:\HF_REPO_ID -ErrorAction SilentlyContinue
  Remove-Item Env:\HF_LOCAL_DIR -ErrorAction SilentlyContinue
}

Write-Host "OCR 模型已下载到: $target"
