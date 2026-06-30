param (
    [string]$TargetDir = "models\vision\moondream2",
    [string]$PythonPath = "python",
    [switch]$SkipExisting
)

$ErrorActionPreference = "Stop"

# Convert relative path to absolute
if (-not [System.IO.Path]::IsPathRooted($TargetDir)) {
    $TargetDir = Join-Path (Get-Location) $TargetDir
}

if ($SkipExisting -and (Test-Path -Path $TargetDir)) {
    $hasConfig = Test-Path -Path (Join-Path $TargetDir "config.json")
    if ($hasConfig) {
        Write-Host "模型已存在于 $TargetDir，跳过下载。" -ForegroundColor Green
        exit 0
    }
}

if (-not (Test-Path -Path $TargetDir)) {
    New-Item -ItemType Directory -Force -Path $TargetDir | Out-Null
}

Write-Host "开始下载 Moondream2 (vikhyatk/moondream2) 到 $TargetDir" -ForegroundColor Cyan
Write-Host "这需要下载约 3.8GB 数据，请耐心等待..." -ForegroundColor Yellow

# Use a temporary python script to download using huggingface_hub
$tempScript = Join-Path $env:TEMP "download_moondream.py"
@"
import sys
import os
try:
    from huggingface_hub import snapshot_download
except ImportError:
    import subprocess
    subprocess.check_call([sys.executable, "-m", "pip", "install", "huggingface_hub"])
    from huggingface_hub import snapshot_download

target_dir = sys.argv[1]

print("开始从 HuggingFace 下载...")
snapshot_download(
    repo_id="vikhyatk/moondream2",
    local_dir=target_dir,
    revision="main",
    ignore_patterns=["*.msgpack", "*.h5", "*.ot", "*.safetensors.index.json"] # Only download safetensors
)
print("下载完成！")
"@ | Out-File -FilePath $tempScript -Encoding utf8

& $PythonPath $tempScript $TargetDir

if ($LASTEXITCODE -ne 0) {
    Write-Error "模型下载失败！"
    exit $LASTEXITCODE
}

Remove-Item -Path $tempScript -Force -ErrorAction Ignore

Write-Host "Moondream2 模型下载完成：$TargetDir" -ForegroundColor Green
