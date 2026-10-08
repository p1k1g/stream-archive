# Phase 23.12 — 1.0.0 릴리스 마무리

상태: **첫 공개 안정 버전 공개 — 2026-10-08; 미완료 수동 검증 별도 관리**

운영자가 [v1.0.0 GitHub Release](https://github.com/p1k1g/stream-archive/releases/tag/v1.0.0)를 공개했습니다. 배포 패키지 생성은 [run 37741196303](https://github.com/p1k1g/stream-archive/actions/runs/37741196303)의 네 job이 모두 성공한 기록을 참조합니다. 공개된 패키지의 체크섬은 [버전별 디렉터리](releases/v1.0.0/)에 보존합니다. 이 공개 기록은 아래 수동 RC 항목의 완료 증빙을 대신하지 않습니다.

## 기준 revision과 작업 범위

Phase 23.12의 시작 기준은 `main`의 `fbd4461d61dd9a79aef18c1a7b14a5562ae88e32`입니다(Phase 23.11 / PR #106). 당시 `Stream Archive check` #306과 `Stream Archive release artifacts` #3은 성공했고, #3에서 Windows/Linux/macOS artifact를 생성했습니다. 당시 artifact는 0.5.2 검증 근거이며 1.0.0 검증을 대신하지 않습니다. Actions artifact 보관 기간은 7일이므로 다운로드 시 만료 여부를 다시 확인합니다.

제품 버전의 기준은 `rust-runtime/Cargo.toml`과 `rust-gui/Cargo.toml`입니다. 두 lockfile에서는 제품 package 항목만 1.0.0으로 변경하고 dependency 버전 및 과거 Phase 문서는 유지했습니다. `RELEASE_INFO.txt` 생성은 Cargo metadata를 읽고, Windows `build.rs` / `winresource`는 GUI package 버전을 사용합니다. 과거 링크를 위해 0.5.2 릴리스 노트는 보존하며 현재 안내는 `docs/RELEASE_NOTES_1_0_0.md`입니다.

renderer, allocator, working-set trim, 아이콘 cache 우회 및 architecture 변경은 포함하지 않습니다. `StreamArchiveCore` 공유 경계와 애플리케이션이 만든 프로세스만 종료하는 수명주기 contract를 유지합니다.

## 자동 검증

최종 commit의 `.github/workflows/rust-runtime-check.yml` 결과를 기준으로 판단합니다.

- 세 플랫폼의 runtime fmt, unit tests, cargo check 및 strict clippy
- Windows Slint fmt, compile, adapter tests 및 strict clippy
- RuntimeContracts, 아이콘 frame 디코딩 및 Native-only Windows package verifier
- Windows 오프라인 백업·복구와 손상된 백업 거부
- 런타임 데이터 유출 거부, 패키지 내부·압축 파일 체크섬, 새 디렉터리 압축 해제 smoke 및 손상된 압축 파일 거부
- Linux/macOS 공식 스크립트 기반 package build 및 CLI·백업·패키지 교체 smoke
- Cargo·패키지 metadata의 정확한 1.0.0 버전과 Windows PE `ProductVersion` 확인

초기 Phase 23.12 최종 commit `badd2c11e26f038363ef7b27f1c2968c2298e018`은 [CI run 36672598519](https://github.com/p1k1g/stream-archive/actions/runs/36672598519)에서 `core-check (windows)`, `core-check (linux)`, `core-check (macos)`, `windows-check`가 모두 통과했습니다. 후속 문서 변경의 검증은 해당 PR의 최신 commit 결과로 별도 확인합니다. 로컬 환경에는 cargo와 PowerShell이 없어 로컬 Rust/Windows 테스트 성공을 주장하지 않습니다.

## 수동 검증용 artifact 생성 절차

`.github/workflows/rust-runtime-release.yml`은 수동 `workflow_dispatch` 방식이고 `contents: read` 권한을 사용합니다. Actions artifact만 업로드하며 GitHub Release를 공개하지 않습니다.

1. 최종 선택한 branch/revision의 필수 PR CI가 모두 통과했는지 확인합니다.
2. GitHub Actions의 `Stream Archive release artifacts`에서 해당 ref를 명시적으로 선택하고 실행합니다.
3. `release-contracts`, `windows-package`, `unix-package (linux)`, `unix-package (macos)`가 모두 성공했는지 확인합니다.
4. 아래 압축 파일과 각각의 `.sha256` 파일을 다운로드합니다.
5. 실제 공개 대상으로 선택한 파일의 SHA-256, package metadata, 내부 체크섬 및 새 압축 해제 결과를 기록합니다.
6. [최종 수동 검증 체크리스트](MANUAL_RC_1_0_0.md)를 해당 파일로 수행합니다.

| 플랫폼 | 압축 파일 |
|---|---|
| Windows x64 | `stream-archive-windows-x64.zip` |
| Linux x64 | `stream-archive-linux-x64.tar.gz` |
| macOS arm64 | `stream-archive-macos-arm64.tar.gz` |

artifact 생성 workflow도 업로드 전에 Cargo·패키지 버전과 Windows PE `ProductVersion`을 독립적으로 확인합니다. 과거 ref의 workflow는 현재 guard의 검증 대상이 아니므로 오래된 ref를 선택해 생성한 파일을 최신 1.0.0으로 간주하면 안 됩니다. 이 문서는 tag나 GitHub Release 공개를 자동으로 실행하지 않습니다.

## 수동 RC 검증 — 미완료

최종 1.0.0 artifact의 수동 검증은 [한국어 체크리스트](MANUAL_RC_1_0_0.md)에서 관리합니다. Windows 아이콘·GUI 조작, 실제 SOOP/CHZZK LIVE/VOD, 외부 도구, 신규 설치, 업그레이드·rollback, 백업·복구 및 OS별 비밀정보 저장 검증이 필요합니다. 인증정보 없는 fixture는 실제 세션이나 native secret store 검증을 대신하지 않습니다.

기존 [Phase 23.9 수동 RC 문서](PHASE23_9_FINAL_MANUAL_RC_RELEASE_PREP.md)의 5절 및 7–14절은 모든 항목이 그대로 필수 조건입니다. 한국어 체크리스트는 해당 세부 항목과 제한을 옮기고 Phase 23.11 아이콘 검증을 추가했습니다. 기존 미완료 gate를 폐기하거나 면제하지 않습니다. 이전 RC는 업그레이드·rollback의 출발점으로 보관하고, 검증 대상은 최종 1.0.0입니다. 기존 PASS를 새 artifact에 자동 적용하지 않으며 모든 해당 항목에 증빙이 있어야 공개할 수 있습니다.

수동 결과에는 artifact SHA-256, OS, 확인 날짜, 실제 결과 및 제한사항을 기록합니다. 임의 schema downgrade 호환성을 가정하지 않고 검증한 업그레이드 전 백업을 보관합니다.

## 기존 RC 관찰 결과

운영자가 보고한 0.5.2 artifact #3의 EXE 내장 아이콘 resource는 정상입니다. 같은 binary가 다른 PC·새 경로에서 정상 표시됐고, 일부 기존 PC에는 이전 아이콘이 남았습니다. 이 결과를 최종 1.0.0의 수동 아이콘 검증 완료로 처리하지 않습니다.

메모리는 동일 PC에서 안정적이었습니다(집 약 45 MB, 회사 약 75 MB; 두 renderer 모두 비슷한 수준). private memory, handles, threads도 안정화되어 지속 증가하는 leak 패턴은 확인되지 않았습니다. 숫자만 줄이는 메모리 최적화의 근거로 삼지 않습니다.

## 공개 차단 조건 및 릴리스 판정

필수 CI 실패, 수동 gate 실패 또는 필요한 증빙 미확보 상태에서는 공개하지 않습니다. Ready for review는 코드 검토 준비 상태이며 수동 RC 완료나 공개 승인을 뜻하지 않습니다. 환경·인증정보가 없어 검증하지 못한 경우 미검증으로 기록하고, 완료로 표시하지 않습니다.

운영자가 최종 merge 및 `v1.0.0` tag / GitHub Release 공개를 수행했습니다. 현재 공개 사실은 [v1.0.0 GitHub Release](https://github.com/p1k1g/stream-archive/releases/tag/v1.0.0)에서 확인합니다. 아래 체크리스트에 증빙이 없는 수동 검증은 공개 이후에도 미완료로 남기며, 이 문서 갱신으로 기존 gate를 완료하거나 면제하지 않습니다.
