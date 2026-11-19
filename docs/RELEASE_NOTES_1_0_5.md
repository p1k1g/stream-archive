# Stream Archive 1.0.5 릴리스 노트

상태: **PowerShell 유지보수·패키징·검증 도구의 순수 Rust 전환 — 2026-11-19**

1.0.4 이후 저장소와 Windows 배포 패키지에 남아 있던 모든 PowerShell(`.ps1`) 유지보수·패키징·계약 검증 스크립트를 순수 Rust 구현으로 통합하고, CI 및 릴리스 워크플로에서 버전 하드코딩과 PowerShell 의존성을 제거했습니다.

## 주요 변경사항

- **오프라인 백업·복구 및 Windows 패키징 도구의 Rust 통합:** `StreamArchive.exe` 및 `stream-archive-cli`에 오프라인 백업·복구(`--backup`, `--restore`), 릴리스 메타데이터(`--write-release-metadata`), 체크섬 생성(`--write-sha256sums`), Windows 패키지·PE 아이콘/버전 검증(`--verify-windows-package`), 릴리스 ZIP 생성(`--new-windows-release-archive`) 및 `--version` CLI를 내장했습니다.
- **기존 PowerShell 백업 메타데이터 호환성 유지:** 이전 Windows 패키지의 `Backup-StreamArchiveData.ps1`이 UTF-8 BOM을 포함해 생성한 `.db.json` 백업 메타데이터도 새 Rust `--restore`에서 정상 복원·SHA-256 검증할 수 있도록 처리했습니다.
- **런타임 계약 가드(12종) 크로스플랫폼 Rust 테스트 전환:** 기존 `maintenance/Test-RuntimeContracts.ps1` 및 `maintenance/guards/*.ps1` 전체를 `cargo test --test runtime_contracts` 통합 테스트로 이관해 Windows·Linux·macOS 모두에서 동일하게 검증합니다.
- **PowerShell 스크립트 제거 및 동적 버전 검증:** 저장소의 `.ps1` 파일 19개를 모두 제거하고 `BUILD_PORTABLE.bat`과 GitHub Actions 워크플로가 `Cargo.toml` 기준으로 버전을 동적 확인하도록 정리했습니다. Windows 배포 패키지에서도 `maintenance\*.ps1` 없이 `BACKUP_DATA.bat`·`RESTORE_DATA.bat`가 `StreamArchive.exe`를 직접 호출합니다.

## 지원 범위

- Windows Native Slint GUI 및 Linux/macOS CLI·headless
- SOOP/CHZZK LIVE·VOD, KICK 공개 LIVE 및 공개·구독 VOD
- Queue / History, Backup / Restore, Diagnostics / Runtime Logs

## 패키지 및 외부 도구

| 플랫폼 | 배포 파일 |
|---|---|
| Windows x64 | `stream-archive-windows-x64.zip` |
| Linux x64 | `stream-archive-linux-x64.tar.gz` |
| macOS arm64 | `stream-archive-macos-arm64.tar.gz` |

`RELEASE_INFO.txt`, Windows EXE `ProductVersion`, Unix CLI version은 1.0.5여야 합니다. Streamlink, yt-dlp, FFmpeg는 별도로 설치하며 공식 패키지에 번들하지 않습니다.

## 업그레이드 안내 (upgrade)

1. 녹화·다운로드를 정리하고 Backup을 생성하여 무결성을 확인합니다.
2. 앱을 정상 종료하고 이전 패키지와 백업을 보관합니다.
3. 1.0.5 archive를 새 디렉터리에 압축 해제하고 기존 `data` 경로를 유지해 실행한 뒤 설정·Channels·Queue·History를 확인합니다.
