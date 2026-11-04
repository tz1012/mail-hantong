# Windows 업데이트 복구와 프로그램 재설치

**Beta 21 scope.** This recovery guide applies to the Windows x64 release. Mac/Linux remain support targets, but their product mail operations, installation, update, and native validation are deferred until Ponytail integration. Unix foundation code is experimental and inactive in the Windows release.


## `update hantong`에서 `Join-Path` 오류가 날 때

`drive` 인수가 null이라는 메시지는 설치 폴더가 없다는 뜻으로 단정할 수 없습니다.
구버전 런처의 `\\?\C:\...` 경로를 Windows PowerShell 5.1에 넘기면 같은
오류가 재현됩니다. 해당 런처 수정은 beta.10 이후 버전에 포함돼 있지만,
구버전의 업데이트 명령 자체가 실패하면 수정본까지 도달하지 못할 수 있습니다.

먼저 **삭제 없이 설치기를 직접 실행**하세요. 같은 Windows 사용자 계정의 일반
PowerShell에서 아래를 실행합니다. 관리자 권한이나 계정 재등록은 요구하지 않습니다.

```powershell
irm https://raw.githubusercontent.com/tz1012/mail-hantong/66d28d6deb0f3dadc7d3591a7fb34350e510753f/install.ps1 | iex
hantong --version
hantong
```

이 명령은 고정된 소스의 설치기를 사용해 최신 호환 공개 패키지를 내려받고 체크섬과
파일을 검증합니다. 설치 중에는 진행 막대를 확인하고 완료될 때까지 기다리세요.
기존 계정·메일·키는 보존합니다. 설치기가 필요한 경우 검증된 해당 설치의 Core만
중지·교체하므로, 이 방법에서는 Core를 따로 종료할 필요가 없습니다.
설치 완료 후 기존 한통 화면을 종료하고 새로 실행하세요.

## 프로그램만 제거하고 새로 설치하기

위 방법 대신 프로그램 파일을 제거하고 다시 설치하려면 다음 순서를 사용하세요.
이는 **계정 데이터를 보존하는 재설치**입니다. 계정 삭제·인증 해제·메일 삭제는
수행하지 않습니다. 먼저 한통 CLI/TUI와 한통에서 연 AI 대화를 종료하세요.

같은 Windows 사용자 계정의 PowerShell에서 아래 전체를 실행합니다.
현재 설치의 소유 표시와 버전을 확인하고, 그 설치 폴더 안의 Core만 종료한 뒤
설치에 포함된 제거기를 실행합니다. 제거기가 실패하면 재설치를 진행하지 않습니다.

```powershell
$r = Join-Path $env:LOCALAPPDATA 'Programs\mail-hantong-cli'
if ([IO.File]::ReadAllText((Join-Path $r '.mail-hantong-owner')) -cne 'mail-hantong-cli-v1') { throw '한통 설치 소유 표시를 확인하지 못했습니다.' }
$v = [IO.File]::ReadAllText((Join-Path $r 'current.txt')).Trim()
if ($v.Length -gt 64 -or $v -notmatch '^\d+\.\d+\.\d+(?:-[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*)?$') { throw '설치 버전을 확인하지 못했습니다.' }
$u = Join-Path (Join-Path (Join-Path $r 'versions') $v) 'uninstall.ps1'
if (-not (Test-Path -LiteralPath $u -PathType Leaf)) { throw '설치된 제거기를 찾지 못했습니다.' }
foreach ($p in @(Get-CimInstance Win32_Process -Filter "Name='mail-hantong-core.exe'")) {
    if ($p.ExecutablePath -and $p.ExecutablePath.StartsWith($r + '\versions\', [StringComparison]::OrdinalIgnoreCase)) {
        Stop-Process -Id $p.ProcessId -ErrorAction Stop
    }
}
& "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File $u -InstallRoot $r
if ($LASTEXITCODE -ne 0) { throw '제거가 완료되지 않았습니다. 파일을 수동 삭제하지 말고 표시된 오류를 확인하세요.' }
if (Test-Path -LiteralPath $r) {
    Rename-Item -LiteralPath $r -NewName ('mail-hantong-cli-backup-' + [guid]::NewGuid().ToString('N')) -ErrorAction Stop
}
irm https://raw.githubusercontent.com/tz1012/mail-hantong/66d28d6deb0f3dadc7d3591a7fb34350e510753f/install.ps1 | iex
hantong --version
hantong
```

프로그램 폴더는 `%LOCALAPPDATA%\Programs\mail-hantong-cli`이며,
계정·메일·키 저장소는 별도의 `%LOCALAPPDATA%\mail-hantong`입니다.
**계정을 보존하려면 뒤의 저장소 폴더를 삭제하지 마세요.**
제거기는 소유권과 파일 해시를 검사하며 알 수 없는 파일은 남깁니다.
남은 프로그램 폴더는 같은 `Programs` 폴더의 `mail-hantong-cli-backup-...`으로
이름을 바꿔 보관합니다. 계정 저장소는 이동하지 않습니다.

## Mac과 Linux의 현재 상태

Mac과 Linux는 지원 계획에 포함되어 있습니다. 현재 배포된 beta.21 설치 패키지는
Windows x64용이며, Mac·Linux의 제품용 계정 연결·메일 이용·설치·업데이트는
아직 구현과 검증이 완료되지 않았습니다. Windows 설치 명령은 두 OS에서 사용할 수 없습니다.

- Mac: 승인된 후속 목표로, 코어·CLI를 먼저 제공하는 방향입니다.
- Linux: 기준 요건 `HOST-02`의 크로스플랫폼 코어 대상입니다.
- Linux에서 일부 Rust 검사에 통과한 결과를 Linux 제품 지원 완료로 보지 않습니다.

현재 상태는 [지원 계획](FEATURES.md)과 [진행 기록](PROGRESS.md)을 함께 확인하세요.
