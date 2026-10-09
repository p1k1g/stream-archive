# Stream Archive 개발 이력 및 로드맵

이 문서는 Phase별 구현·절차 정리 이력을 보존합니다. 완료 표시는 실제 수동 QA 완료나 공개 승인을 뜻하지 않습니다. 현재 사용자 안내는 [README](../README.md)를 참고하세요.

<a id="roadmap"></a>

## 로드맵

### Phase 19 ✅ 런타임 안정화 및 정리

- 런타임 자원 소유권 및 정리 강화
- LIVE process-tree 소유권 강화
- CHZZK VOD reader의 처리 한도 및 취소 시 정리
- SQLite를 기준 데이터 저장소로 정리
- OS별 런타임 경계 통합
- 런타임 contract 및 CI 통합

### Phase 19.5 ✅ 이름 체계 및 legacy 정리

- 제품 이름 체계를 `Stream Archive`로 통일
- 공통 환경변수와 런타임 파일명을 `STREAM_ARCHIVE_*` / `stream-archive-*`로 정리
- INI/TXT 호환 경로와 사용하지 않는 코드 제거
- portable·build·릴리스 이름 정리

### Phase 20 ✅ 크로스플랫폼 런타임 준비

- ✅ Windows/Linux/macOS GitHub-hosted CI matrix 기반 확립
- ✅ Unix process-group 소유권 및 종료
- ✅ Linux Secret Service / macOS Keychain 비밀정보 저장 경계
- ✅ Unix/headless CLI 및 크로스플랫폼 Streamlink/yt-dlp/FFmpeg 탐색 기반
- ✅ Linux/macOS CLI 런타임·설정 명령 확장 — Phase 23.5에서 완료
- ✅ Linux/macOS 실제 binary 기반의 재현 가능한 CLI·도구 통합 검증 — Phase 23.5에서 완료
- ✅ Unix portable 압축 파일 패키징 및 설치 안내 — Phase 23.6에서 완료

Phase 20에서는 cross-platform native picker나 Linux/macOS GUI launcher를 추가하지 않습니다. Unix 계열은 CLI/headless 경로를 명확히 하고, Windows GUI 교체는 Phase 21로 분리합니다.

### Phase 21 ✅ Slint Native GUI

- ✅ 설정 / Native 경로 선택 / 진단
- ✅ Channels 및 LIVE 감시·녹화
- ✅ SOOP/CHZZK VOD 분석·다운로드
- ✅ VOD Queue + LIVE/VOD History
- ✅ Native Backup/Restore 및 Diagnostics/Runtime Logs
- ✅ Windows Native portable 패키징 및 실행 경로 전환
- ✅ Native UX 개선: LIVE 저장공간, 설정 내부 일반/관리 정리 및 Backup 관리 통합, 내부 claim sidecar 노출 개선 — Phase 23.1에서 일상 사용 UX 정리 완료
- Slint GUI는 shared Rust core를 직접 호출하며 localhost HTTP, direct SQLite, direct process control을 사용하지 않음
- ✅ Phase 22.3에서 legacy browser/Web UI, Axum presentation, Web launcher/fallback 제거
- Linux/macOS는 GUI를 복제하지 않고 Phase 20의 CLI/headless 인터페이스 유지

### Phase 22 ✅ 런타임 및 legacy 구조 정리

- ✅ legacy dependency 및 warning 정리
- ✅ browser/Web presentation 및 Axum application layer 제거
- ✅ 런타임·core 호환 경계 점검
- ✅ 공유 런타임 source 경로를 `rust-runtime/`로 통일
- ✅ CI/release workflow 이름을 `rust-runtime-*`로 통일
- ✅ 최종 legacy·호환성 점검 및 CHZZK 임시 런타임 경로 정리
- `stream-archive-server` package/headless binary 이름은 Linux/macOS CLI/headless 및 개발 호환성 경계로 유지하며 Windows 공식 ZIP에는 포함하지 않음

### Phase 23 🚧 제품 공개 준비 및 통합

- ✅ 23.1 Native 일상 사용 UX 정리
- ✅ 23.2 Diagnostics 및 런타임 사전 점검
- ✅ 23.3 미디어 도구 통합 검증
- ✅ 23.4 서비스별 E2E 검증
- ✅ 23.5 Unix CLI 완성
- ✅ 23.6 패키징 및 릴리스 준비
- ✅ 23.7 RC 및 최종 QA 절차 정리
- ✅ 23.8 릴리스 정리 및 코드 축소
- ✅ 23.9 최종 수동 RC 절차 및 공개 준비
- ✅ 23.10 Windows 브랜딩 및 배포 마무리
- ✅ 23.11 Windows 아이콘 resource 수정
- ✅ 23.12 1.0.0 릴리스 마무리 (버전·문서·자동 검증 정리 및 공개 완료; 미완료 수동 RC는 별도 관리)

첫 공개 안정 버전 [v1.0.0 GitHub Release](https://github.com/p1k1g/stream-archive/releases/tag/v1.0.0)가 2026-10-08 공개됐습니다. [릴리스 노트](RELEASE_NOTES_1_0_0.md), [릴리스 마무리 절차](PHASE23_12_1_0_0_RELEASE_CLOSURE.md), [최종 수동 검증 체크리스트](MANUAL_RC_1_0_0.md)를 참고하세요. 로드맵의 완료 표시는 해당 구현·절차 정리를 뜻하며 실제 수동 QA 완료나 공개 승인을 의미하지 않습니다. `v1.0.0` tag / GitHub Release는 공개됐으며, 증빙이 없는 수동 QA 항목은 미완료로 유지합니다.

## 1.0.0 공개까지의 추가 개발

- Phase 24.1 ✅ Windows 닫기 동작 / 시스템 트레이 ([설계 및 수동 QA](PHASE24_1_WINDOWS_TRAY.md))
- Phase 24.2 ✅ Windows 다운로드 완료·실패 알림 ([알림 정책 및 수동 QA](PHASE24_2_WINDOWS_NOTIFICATIONS.md))
- Phase 24.3 예정: 트레이·알림 최종 수동 검증 및 릴리스 준비
- Phase 25 ✅ Windows UI/UX Refresh ([구현 범위 및 수동 QA](PHASE25_UI_UX_REFRESH.md))
- Phase 25.1 ✅ 저장 공간·플랫폼 선택·VOD 경로·아이콘 정리 ([범위 및 수동 QA](PHASE25_1_UI_POLISH.md))

- Phase 25.2 ✅ UI 사용성 및 안정화 ([범위 및 수동 QA](PHASE25_2_UI_USABILITY.md))
- Phase 25.3 ✅ LIVE 채널 프로필 이미지 ([범위 및 수동 QA](PHASE25_3_CHANNEL_PROFILE_IMAGES.md))

- Phase 25.4 ✅ LIVE 방송 썸네일 / 플랫폼 로고 및 채널 폴더 선택 ([범위 및 수동 QA](PHASE25_4_LIVE_THUMBNAILS_FOLDER_PICKER.md))
- Phase 25.5 ✅ SOOP VOD 분석 및 인증 흐름 정리 ([범위 및 수동 QA](PHASE25_5_SOOP_VOD_STABILIZATION.md))
- Phase 25.6 ✅ VOD 인증 만료 후 이어받기 및 분석 썸네일 ([범위 및 수동 QA](PHASE25_6_VOD_RESUME_THUMBNAILS.md))

- Phase 26.1 ✅ KICK 공개 LIVE 지원 ([사용 조건 및 수동 QA](PHASE26_1_KICK_LIVE.md))

- Phase 26.2 ✅ KICK VOD: playback 인증, 화질 선택, yt-dlp 4개 조각 병렬 다운로드 및 MPEG-TS 저장, FFmpeg 첫 프레임 썸네일. [범위 및 수동 RC](PHASE26_2_KICK_VOD.md)

- Phase 26.3 🚧 README 및 사용 가이드 정리: 최신 실제 화면, 한국어 메뉴 경로와 KICK 다운로드·인증정보 관리 안내 반영.
