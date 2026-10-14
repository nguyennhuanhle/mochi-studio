<#
.SYNOPSIS
  Chuẩn bị môi trường build Mochi Studio trên Windows (chạy một lần, chạy lại vô hại).

.DESCRIPTION
  - Tải FFmpeg 7.1 full_build-shared (GyanD) → target/native-deps/{lib,include,bin}
  - Tải DXC (dxcompiler.dll, dxil.dll) → target/native-deps/dxc  (tùy chọn, renderer tự lùi về FXC nếu thiếu)
  - Tìm libclang.dll (LLVM) cho bindgen của ffmpeg-sys
  - Ghi .cargo/config.toml: FFMPEG_DIR, LIBCLANG_PATH
  - Copy DLL FFmpeg + DXC vào target/debug và target/release để exe chạy được

  Yêu cầu trước: Rust (rustup), Visual Studio Build Tools (MSVC), LLVM (`winget install LLVM.LLVM`).
#>
[CmdletBinding()]
param(
  [switch]$SkipDxc
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest nhanh hơn nhiều khi tắt progress bar

$Root = Split-Path -Parent $PSScriptRoot
$Target = Join-Path $Root 'target'
$NativeDeps = Join-Path $Target 'native-deps'
New-Item -ItemType Directory -Force $NativeDeps | Out-Null

function Get-Download($Url, $OutFile) {
  if (Test-Path $OutFile) { Write-Host "  (đã có) $OutFile"; return }
  Write-Host "  tải $Url"
  Invoke-WebRequest -Uri $Url -OutFile "$OutFile.partial" -UseBasicParsing
  Move-Item "$OutFile.partial" $OutFile
}

# ---------- FFmpeg 7.1 shared ----------
$FfmpegVersion = '7.1'
$FfmpegName = "ffmpeg-$FfmpegVersion-full_build-shared"
$FfmpegZip = Join-Path $Target "$FfmpegName.zip"
$FfmpegDir = Join-Path $Target $FfmpegName
Write-Host "[1/4] FFmpeg $FfmpegVersion"
if (-not (Test-Path (Join-Path $NativeDeps 'lib\avcodec.lib'))) {
  Get-Download "https://github.com/GyanD/codexffmpeg/releases/download/$FfmpegVersion/$FfmpegName.zip" $FfmpegZip
  if (-not (Test-Path $FfmpegDir)) { Expand-Archive -Path $FfmpegZip -DestinationPath $Target -Force }
  foreach ($sub in 'lib', 'include', 'bin') {
    Copy-Item -Recurse -Force (Join-Path $FfmpegDir $sub) $NativeDeps
  }
  Copy-Item -Force (Join-Path $FfmpegDir 'LICENSE') (Join-Path $NativeDeps 'FFMPEG-LICENSE.txt') -ErrorAction SilentlyContinue
} else { Write-Host '  (đã có) target/native-deps/lib' }

# ---------- DXC ----------
$DxcDir = Join-Path $NativeDeps 'dxc'
if (-not $SkipDxc) {
  Write-Host '[2/4] DXC 1.9.2607'
  if (-not (Test-Path (Join-Path $DxcDir 'dxcompiler.dll'))) {
    $DxcPkg = Join-Path $Target 'dxc.1.9.2607.13.zip'
    Get-Download 'https://api.nuget.org/v3-flatcontainer/microsoft.direct3d.dxc/1.9.2607.13/microsoft.direct3d.dxc.1.9.2607.13.nupkg' $DxcPkg
    $DxcExtract = Join-Path $Target 'dxc-pkg'
    Expand-Archive -Path $DxcPkg -DestinationPath $DxcExtract -Force
    New-Item -ItemType Directory -Force $DxcDir | Out-Null
    foreach ($dll in 'dxcompiler.dll', 'dxil.dll') {
      Copy-Item -Force (Join-Path $DxcExtract "build\native\bin\x64\$dll") $DxcDir
    }
  } else { Write-Host '  (đã có) DXC' }
} else { Write-Host '[2/4] DXC: bỏ qua (-SkipDxc)' }

# ---------- ONNX Runtime (CPU) cho xóa / làm mờ nền webcam (E08) ----------
# Bản CPU (MIT, không kèm DirectML.dll): model tách người 256x256 chạy CPU vài ms / khung.
$OrtDir = Join-Path $NativeDeps 'ort'
Write-Host '[2b/4] ONNX Runtime 1.24.4 (CPU)'
if (-not (Test-Path (Join-Path $OrtDir 'onnxruntime.dll'))) {
  New-Item -ItemType Directory -Force $OrtDir | Out-Null
  $OrtPkg = Join-Path $Target 'ort-cpu.1.24.4.zip'
  Get-Download 'https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime/1.24.4/microsoft.ml.onnxruntime.1.24.4.nupkg' $OrtPkg
  $OrtExtract = Join-Path $Target 'ort-cpu-pkg'
  Expand-Archive -Path $OrtPkg -DestinationPath $OrtExtract -Force
  Copy-Item -Force (Join-Path $OrtExtract 'runtimes\win-x64\native\onnxruntime.dll') $OrtDir
  Copy-Item -Force (Join-Path $OrtExtract 'LICENSE') (Join-Path $OrtDir 'ONNXRUNTIME-LICENSE.txt') -ErrorAction SilentlyContinue
} else { Write-Host '  (đã có) ONNX Runtime' }

# ---------- libclang ----------
Write-Host '[3/4] libclang'
$ClangCandidates = @(
  "$env:ProgramFiles\LLVM\bin",
  "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\VC\Tools\Llvm\x64\bin",
  "$env:ProgramFiles\Microsoft Visual Studio\2022\Community\VC\Tools\Llvm\x64\bin"
)
$LibclangDir = $ClangCandidates | Where-Object { Test-Path (Join-Path $_ 'libclang.dll') } | Select-Object -First 1
if (-not $LibclangDir) { throw 'Không tìm thấy libclang.dll. Cài LLVM 19: winget install LLVM.LLVM --version 19.1.7' }
# bindgen 0.70 (ffmpeg-sys-next) sinh struct rỗng với libclang >= 20 → bắt buộc LLVM 17–19.
$ClangExe = Join-Path $LibclangDir 'clang.exe'
if (Test-Path $ClangExe) {
  $ClangVer = (& $ClangExe --version | Select-Object -First 1)
  if ($ClangVer -match 'version (\d+)\.' -and [int]$Matches[1] -ge 20) {
    throw "Cần LLVM 17–19 (đang có: $ClangVer). Chạy: winget uninstall LLVM.LLVM; winget install LLVM.LLVM --version 19.1.7"
  }
  Write-Host "  $ClangVer"
}
Write-Host "  $LibclangDir"

# ---------- .cargo/config.toml ----------
Write-Host '[4/4] .cargo/config.toml + copy DLL'
$CargoDir = Join-Path $Root '.cargo'
New-Item -ItemType Directory -Force $CargoDir | Out-Null
$LibclangToml = $LibclangDir -replace '\\', '/'
@"
# GENERATED by scripts/setup.ps1 - do not edit by hand
[env]
FFMPEG_DIR = { relative = true, force = true, value = "target/native-deps" }
LIBCLANG_PATH = "$LibclangToml"
"@ | Out-File -Encoding ascii (Join-Path $CargoDir 'config.toml')

foreach ($profile in 'debug', 'release') {
  $dst = Join-Path $Target $profile
  New-Item -ItemType Directory -Force $dst | Out-Null
  Copy-Item -Force (Join-Path $NativeDeps 'bin\*.dll') $dst
  if (Test-Path $DxcDir) { Copy-Item -Force (Join-Path $DxcDir '*.dll') $dst }
  if (Test-Path $OrtDir) { Copy-Item -Force (Join-Path $OrtDir '*.dll') $dst }
}

Write-Host "`nXong. Thử: cargo run -p mochi-cli -- targets"
