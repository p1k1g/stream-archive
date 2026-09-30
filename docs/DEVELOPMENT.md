# Stream Archive 개발 및 빌드 안내

일반 사용자의 실행 방법은 [README](../README.md)를 참고하세요. 이 문서는 source build, architecture 및 CI를 설명합니다.

## 런타임 구조

```text
Windows Slint Native GUI ─┐
Unix CLI/headless ─────────┼──> StreamArchiveCore / shared Rust runtime
Headless runtime binary ───┘             │
                                         ├─ SQLite: data/stream-archive.db
                                         ├─ NativeWatcherManager / RecorderManager
                                         ├─ VOD / Queue / History / Backup
                                         └─ Streamlink / yt-dlp / FFmpeg
```

Windows portable의 기본 진입점은 Slint Native GUI이며 localhost HTTP를 애플리케이션 API로 사용하지 않습니다. Windows Native UI와 Unix CLI/headless runtime은 모두 `StreamArchiveCore`와 같은 SQLite/runtime 서비스를 직접 사용합니다.

Phase 22.3에서 기존 Axum/browser presentation, browser launcher와 Web fallback package 경로를 제거했습니다. 이전 WinUI/PowerShell 런타임과 INI/TXT 설정 mirror도 제거된 상태를 유지합니다.

Linux/macOS는 Phase 23.5에서 완성한 `stream-archive-cli` 기반 headless daily-use 인터페이스를 사용합니다.

## 빌드

### 개발 / 로컬 실행

```powershell
.\RUN_DEV.bat
```

### Windows portable package

Windows에서 실제 실행·테스트·배포에 사용하는 단일 빌드 진입점은 다음입니다.

```powershell
.\BUILD_PORTABLE.bat
```

`BUILD_PORTABLE.bat`은 Slint Native GUI를 tracked `Cargo.lock`으로 release build합니다. GUI build가 shared Rust core를 dependency로 함께 컴파일하며, Windows 공식 package에는 별도의 headless server binary를 넣지 않습니다.

```text
rust-gui release build + shared Rust core dependency
                    ↓
             dist\stream-archive
```

기본 출력 위치:

```text
dist\stream-archive
```

주요 패키지 구성:

```text
StreamArchive.exe
RUN.bat
BACKUP_DATA.bat
RESTORE_DATA.bat
RELEASE_INFO.txt
SHA256SUMS.txt
backend\
data\
maintenance\...
docs\...
```

`RUN.bat`과 `StreamArchive.exe`가 Windows 공식 portable package의 Native 실행 경로입니다. Windows release ZIP은 별도의 headless launcher/server를 포함하지 않습니다.

별도의 `BUILD_RELEASE.bat` wrapper는 사용하지 않습니다. 컴파일 결과만 확인해야 하는 개발 작업에서는 Cargo를 직접 실행할 수 있습니다.

```powershell
cargo build --locked --release --manifest-path .\rust-runtime\Cargo.toml
cargo build --locked --release --manifest-path .\rust-gui\Cargo.toml
```

각 Cargo `target\release` 디렉터리는 raw build output이며 배포 패키지 기준이 아닙니다. 실제 실행·배포 검증은 `dist\stream-archive`를 기준으로 합니다.

### Linux / macOS package

Phase 23.6의 Unix release artifact는 source checkout 없이 사용할 수 있는 portable TAR.GZ입니다. PR CI에서 현재 검증된 native artifact는 `stream-archive-linux-x64.tar.gz`와 `stream-archive-macos-arm64.tar.gz`입니다.

```bash
tar -xzf stream-archive-linux-x64.tar.gz
cd stream-archive

./bin/stream-archive-cli init
./bin/stream-archive-cli tools
./bin/stream-archive-cli tools configure
./bin/stream-archive-cli doctor --active-tools
./bin/stream-archive-cli status --json
```

macOS도 archive 이름만 `stream-archive-macos-arm64.tar.gz`로 바꾸고 같은 package-local `./bin/stream-archive-cli` 경로를 사용합니다. package root의 `backend/`와 `data/`를 기본 layout으로 사용하므로 archive root에서 실행하거나 `STREAM_ARCHIVE_BACKEND_DIR` / `STREAM_ARCHIVE_DATA_DIR`를 명시하세요.

개발/source build가 필요하면 `./BUILD_UNIX_PACKAGE.sh`가 locked release build → clean staging → metadata/checksum → package verify → archive/checksum → fresh-extract smoke를 한 흐름으로 수행합니다. `tools configure`는 Streamlink/yt-dlp/FFmpeg를 SQLite 설정 → backend layout → `PATH` → 일반적인 Unix 설치 경로 순서로 찾습니다. 외부 media tool은 archive에 포함되지 않습니다.

Phase 23.5의 daily-use CLI, runtime owner lock, non-recovering observer, Unix-domain runtime control, SIGINT/SIGTERM semantics는 그대로 유지됩니다. 자세한 command tree와 운영 제약은 `docs/UNIX_CLI.md`를 참고하세요.

## 개발 / CI

Pull Request의 런타임 검증은 `.github/workflows/rust-runtime-check.yml`에서 수행합니다.

주요 검증 항목:

- Windows / Linux / macOS 전체 crate의 `cargo fmt --check`
- Windows / Linux / macOS Rust unit tests 및 재현 가능한 미디어 도구 subprocess 검증
- Windows / Linux / macOS native compile 검증
- Windows / Linux / macOS strict Clippy (`-D warnings`)
- Windows 런타임 contract guard
- source archive에서 릴리스 metadata 생성 smoke (Windows/Linux/macOS)
- Windows portable 패키지 및 ZIP build·검증
- Linux/macOS TAR.GZ build, 패키지 내부·압축 파일 체크섬 및 새 압축 해제 CLI smoke

릴리스 workflow는 `.github/workflows/rust-runtime-release.yml`의 수동 `workflow_dispatch` 방식입니다. GitHub Release/tag를 만들지 않고 검증한 Actions artifact만 업로드합니다. artifact job 전에 런타임·패키징 contract gate가 통과해야 하며, 업로드 전 정확한 1.0.0 metadata와 Windows EXE 버전도 확인합니다. artifact 보관 기간은 7일입니다.

PR CI는 각 플랫폼의 GitHub-hosted runner에서 `windows-x64`, `linux-x64`, `macos-arm64` 패키지를 실제 조립하고 체크섬·새 압축 해제 smoke까지 검증합니다. 실제 인증정보가 필요한 로그인·세션 smoke와 공개 판정은 자동 CI 범위 밖입니다. 남은 항목은 [1.0.0 수동 검증 체크리스트](MANUAL_RC_1_0_0.md)에서 관리합니다.
