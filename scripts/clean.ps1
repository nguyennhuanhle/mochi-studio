<#
.SYNOPSIS
  Dọn file build tạm của Mochi Studio để thư mục repo không phình to.

.DESCRIPTION
  Cargo tạo một bộ cache mới trong target\<profile>\incremental mỗi khi một crate được build theo
  cách khác (đổi -p, feature, test / check, nâng phiên bản…), và không bao giờ tự xóa bộ cũ. Sau vài
  ngày làm việc, thư mục này có thể lên tới hàng chục GiB.

  Mức dọn:
    (mặc định)  target\debug\incremental, target\release\incremental — chỉ là cache, xóa không mất gì,
                lần build sau chậm hơn một chút.
    -Deep       thêm toàn bộ kết quả build trong target\debug và target\release (cargo build lại từ
                đầu, mất vài chục phút), GIỮ các DLL ở gốc hai thư mục đó (FFmpeg, DXC, ONNX Runtime do
                setup.ps1 chép vào — cargo không tự tạo lại chúng).
    -Bundles    xóa installer cũ trong target\release\bundle, chỉ giữ bản của phiên bản hiện tại.

  KHÔNG BAO GIỜ đụng: target\native-deps (FFmpeg, cmake, ONNX Runtime — .cargo\config.toml trỏ vào
  đây), mã nguồn, recordings\, _deliverables\, docs\.

.PARAMETER IfLargerThanGB
  Chỉ dọn khi target\ lớn hơn ngưỡng này (GiB). 0 = luôn dọn.

.PARAMETER DryRun
  Chỉ in những gì sẽ bị xóa và dung lượng, không xóa.

.EXAMPLE
  pwsh scripts\clean.ps1 -DryRun
  powershell -File scripts\clean.ps1                     # dọn cache incremental
  powershell -File scripts\clean.ps1 -IfLargerThanGB 40  # chỉ dọn khi target\ > 40 GiB
  powershell -File scripts\clean.ps1 -Deep -Bundles
#>
param(
  [switch]$Deep,
  [switch]$Bundles,
  [double]$IfLargerThanGB = 0,
  [switch]$DryRun
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$Target = Join-Path $Root 'target'

function Get-SizeGiB([string]$Path) {
  if (-not (Test-Path -LiteralPath $Path)) { return 0 }
  $sum = (Get-ChildItem -LiteralPath $Path -Recurse -File -Force -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
  return [math]::Round(($sum / 1GB), 2)
}

if (-not (Test-Path -LiteralPath $Target)) { Write-Host 'Chưa có thư mục target\ — không có gì để dọn.'; exit 0 }

# Không dọn khi đang build: cargo / rustc / rust-analyzer đang giữ file trong target\.
$busy = Get-Process cargo, rustc, rust-analyzer -ErrorAction SilentlyContinue
if ($busy -and -not $DryRun) {
  Write-Warning ("Đang có tiến trình build chạy ({0}) — để lần sau." -f (($busy.ProcessName | Sort-Object -Unique) -join ', '))
  exit 2
}

$before = Get-SizeGiB $Target
Write-Host ("target\ hiện chiếm {0} GiB" -f $before)
if ($IfLargerThanGB -gt 0 -and $before -lt $IfLargerThanGB) {
  Write-Host ("Dưới ngưỡng {0} GiB — không dọn." -f $IfLargerThanGB)
  exit 0
}

$items = New-Object System.Collections.Generic.List[string]
foreach ($p in 'debug', 'release') {
  $inc = Join-Path $Target "$p\incremental"
  if (Test-Path -LiteralPath $inc) { $items.Add($inc) }
}

if ($Deep) {
  foreach ($p in 'debug', 'release') {
    $dir = Join-Path $Target $p
    if (-not (Test-Path -LiteralPath $dir)) { continue }
    # Giữ DLL ở gốc (FFmpeg / DXC / ONNX Runtime) và thư mục bundle (installer, xử lý bởi -Bundles).
    Get-ChildItem -LiteralPath $dir -Force | Where-Object {
      -not ($_.PSIsContainer -eq $false -and $_.Extension -eq '.dll' -and $_.Name -notlike 'mochi*') -and
      $_.Name -ne 'bundle' -and $_.Name -ne 'incremental'
    } | ForEach-Object { $items.Add($_.FullName) }
  }
}

if ($Bundles) {
  $conf = Get-Content -Raw -LiteralPath (Join-Path $Root 'apps\desktop\src-tauri\tauri.conf.json') | ConvertFrom-Json
  $version = $conf.version
  foreach ($kind in 'nsis', 'msi') {
    $dir = Join-Path $Target "release\bundle\$kind"
    if (-not (Test-Path -LiteralPath $dir)) { continue }
    Get-ChildItem -LiteralPath $dir -File | Where-Object { $_.Name -notlike "*_$($version)_*" } |
      ForEach-Object { $items.Add($_.FullName) }
  }
}

# Chốt an toàn: chỉ xóa đường dẫn nằm trong target\ và không bao giờ là native-deps.
$native = Join-Path $Target 'native-deps'
$safe = $items | Where-Object {
  $_.StartsWith($Target + '\', [StringComparison]::OrdinalIgnoreCase) -and
  -not $_.StartsWith($native, [StringComparison]::OrdinalIgnoreCase)
}

if (-not $safe) { Write-Host 'Không có gì để dọn.'; exit 0 }

$total = 0
$loose = @{}  # file lẻ ở gốc thư mục debug / release: gộp một dòng mỗi thư mục
foreach ($p in $safe) {
  $item = Get-Item -LiteralPath $p -Force
  if ($item.PSIsContainer) {
    $size = Get-SizeGiB $p
    Write-Host ("  {0,8:N2} GiB  {1}" -f $size, $p.Substring($Root.Length + 1))
  } else {
    $size = $item.Length / 1GB
    $key = $item.DirectoryName.Substring($Root.Length + 1)
    if (-not $loose.ContainsKey($key)) { $loose[$key] = @(0, 0.0) }
    $loose[$key] = @(($loose[$key][0] + 1), ($loose[$key][1] + $size))
  }
  $total += $size
  if (-not $DryRun) { Remove-Item -LiteralPath $p -Recurse -Force }
}
foreach ($k in $loose.Keys) {
  Write-Host ("  {0,8:N2} GiB  {1}\ ({2} file)" -f $loose[$k][1], $k, $loose[$k][0])
}

if ($DryRun) {
  Write-Host ("Chạy thử: sẽ giải phóng khoảng {0:N1} GiB. Bỏ -DryRun để xóa thật." -f $total)
} else {
  Write-Host ("Đã dọn: target\ {0} GiB → {1} GiB." -f $before, (Get-SizeGiB $Target))
}
