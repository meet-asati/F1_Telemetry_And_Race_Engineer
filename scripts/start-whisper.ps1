# Starts the local whisper.cpp speech-to-text server used for push-to-talk.
# Matches the default [stt] config: engine = "whispercpp", url = "http://127.0.0.1:8178".
#
#   powershell -ExecutionPolicy Bypass -File scripts\start-whisper.ps1
#   powershell -ExecutionPolicy Bypass -File scripts\start-whisper.ps1 -Model ggml-small.en.bin -Threads 6

param(
    [string]$Model = "ggml-base.en.bin",
    [int]$Port = 8178,
    [int]$Threads = 4
)

$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root "sidecars\whisper\bin\Release\whisper-server.exe"
$modelPath = Join-Path $root "sidecars\whisper\models\$Model"

if (-not (Test-Path $exe)) {
    Write-Error "whisper-server.exe not found at $exe. Download whisper-bin-x64.zip from the whisper.cpp releases and extract it to sidecars\whisper\bin."
    exit 1
}
if (-not (Test-Path $modelPath)) {
    Write-Error "Model not found at $modelPath. Download it from https://huggingface.co/ggerganov/whisper.cpp (e.g. ggml-base.en.bin) into sidecars\whisper\models."
    exit 1
}

Write-Host "whisper.cpp server on http://127.0.0.1:$Port  (model $Model, $Threads threads)"
& $exe -m $modelPath --host 127.0.0.1 --port $Port -t $Threads -l en
