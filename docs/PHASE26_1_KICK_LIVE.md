# Phase 26.1 — KICK 공개 LIVE

상태: 구현 및 PR 검증 중. 실제 Windows 녹화·장시간 재연결 수동 QA는 미수행입니다.

## 범위와 구조

- 채널 관리의 플랫폼 선택에 `KICK`을 추가합니다. 계정/채널 ID에는 `https://kick.com/xqc`의 `xqc` 같은 slug를 입력합니다. URL·경로·query는 허용하지 않으며 입력 길이는 방어적 상한 100자를 적용합니다(플랫폼의 공식 최대 길이를 뜻하지 않습니다).
- `platform/kick`가 채널 이름 조회, LIVE metadata/API 및 Streamlink 입력을 담당합니다. Slint UI는 기존 `StreamArchiveCore`를 호출합니다.
- 기존 watcher / recorder / owned-process lifecycle / 저장 공간 / LIVE History / Backup·Restore를 재사용합니다. KICK 전용 Web UI·DB·외부 프로세스 제어를 UI에 만들지 않습니다.
- `KICK`과 SOOP/CHZZK는 동일 계정 문자열이어도 서로 다른 채널입니다. 중지·재개·재확인은 플랫폼을 포함한 대상에 적용합니다.
- KICK은 공개 LIVE만 지원합니다. KICK VOD·Queue·클립·구독/로그인 제한 방송 인증은 이번 범위에 포함하지 않습니다. 기존 VOD 완료 알림 범위는 SOOP/CHZZK 그대로입니다.
- KICK API에서 받은 방송 ID·제목·표시 이름·썸네일만 사용합니다. 썸네일은 기존 제한된 HTTPS 이미지 로더와 cache를 사용하고, 실패/없음/오프라인은 플랫폼 식별 이미지로 대체합니다.
- 원본 API의 `livestream: null`과 일치하는 채널 identity가 확인된 경우만 오프라인으로 판단합니다. 401/403/404/429/잘못된 JSON·metadata는 오류로 처리하며 녹화 중 일시적인 API 오류만으로 recorder를 종료하지 않습니다.
- 실제 녹화는 canonical `https://kick.com/<slug>`를 Streamlink KICK 플러그인에 전달합니다. 최고 화질 `best`와 기존 `.ts` 출력/owned cancellation을 사용합니다. SOOP 로그인/Worker와 CHZZK Cookie·PTS remux 옵션을 KICK에 전달하지 않습니다.

## 도구 및 브라우저 요구 조건

Streamlink 공식 문서는 KICK LIVE/VOD 및 API JS challenge 처리를 위한 웹 브라우저 요구 조건을 안내합니다. KICK API와 플러그인 동작은 변경될 수 있으므로 설치한 최신 Streamlink로 확인해야 합니다.

- 외부 `streamlink` 및 필요할 때 Chromium 계열 브라우저를 사용자가 설치합니다. 공식 Stream Archive artifact에 미디어 도구나 브라우저를 번들하지 않습니다.
- Streamlink의 브라우저 사용은 외부 플러그인의 실행 조건이며 Stream Archive의 browser/Web UI 또는 localhost application API를 복구하는 기능이 아닙니다.
- Rust 감시 API가 403으로 차단되면 현 단계에서 이를 자동으로 해결하지 않습니다. Streamlink의 Cookie/cache를 Rust 감시 API에 복사하거나 browser session을 우회하는 bridge를 만들지 않습니다. API가 허용되는 환경에서 공개 LIVE를 감지합니다.
- 녹화 시작 시 Streamlink가 challenge 처리에 실패하거나 브라우저가 없으면 LIVE 오류/Runtime Logs를 확인하세요. 진단의 KICK 항목은 조건 안내이며 연결 성공·브라우저 설치 확인이 아닙니다.
- Linux/macOS는 CLI/headless 인터페이스를 유지합니다. headless Chromium 사용 가능 여부는 OS·설치 환경에서 별도로 검증해야 하며 모든 환경에서 브라우저 없이 동작한다고 보장하지 않습니다.

참고: [Streamlink KICK 지원](https://streamlink.github.io/plugins.html#kick), [공식 플러그인 source](https://github.com/streamlink/streamlink/blob/master/src/streamlink/plugins/kick.py).

## 데이터와 백업

기존 SQLite의 platform TEXT 및 `(platform, account)` identity를 사용하며 DB migration은 추가하지 않습니다. KICK 채널과 LIVE History가 Backup/Restore에 포함됩니다. KICK 데이터를 추가한 DB를 KICK을 모르는 이전 앱 버전으로 실행하는 호환성은 보장하지 않습니다. rollback이 필요하면 업그레이드 전 생성한 Backup과 기존 앱 버전을 함께 사용하세요.

## 검증 기록

- 구현 환경에서 `https://kick.com/api/v2/channels/xqc`와 공개 livestream endpoint가 HTTP 200으로 응답했고, `livestream: null`/`data: null`을 확인했습니다. 이것은 실제 녹화 성공을 뜻하지 않습니다.
- Streamlink 8.6.1의 `--can-handle-url https://kick.com/xqc` 성공을 확인했습니다. 실제 `--json` 호출은 API read timeout으로 종료되어 성공으로 기록하지 않습니다.
- 자동 fixture: 공개 LIVE metadata/명시적 오프라인/identity 불일치/접근 오류 구분, 썸네일 CDN 제한, KICK plugin argv/출력/owned cancel, 동일 ID 플랫폼 분리와 Backup/Restore/History 보존을 검증합니다.
- 최신 PR head의 Windows/Linux/macOS CI 및 Codex review 상태는 PR과 Actions를 기준으로 확인합니다. 실제 서비스 검증과 fixture 결과를 구분합니다.

## 남은 수동 QA

- [ ] Windows KICK 채널 등록·수정·삭제·이름 조회
- [ ] 실제 공개 LIVE 감지·녹화 시작·방송 종료·파일 재생/영상·음성 확인
- [ ] 방송 중 재시작·네트워크 중단·재연결·방송 ID 변경
- [ ] 채널 중지·재개·재확인 및 SOOP/CHZZK 동시 녹화 유지
- [ ] 실제 LIVE 썸네일/오프라인 로고/썸네일 cache 정리
- [ ] 실제 API 403/429 및 Streamlink challenge/브라우저 누락 오류 확인
- [ ] 트레이/종료 시 owned Streamlink 및 challenge browser 자식 정리, 다른 앱 유지
- [ ] LIVE History·저장 폴더·저장 공간·Backup/Restore
- [ ] Windows Chromium 설치 환경 실제 smoke
- [ ] Linux/macOS CLI/headless 실제 smoke 및 Chromium 요구 조건 확인

main 직접 commit/push, PR merge, tag 및 GitHub Release 생성은 수행하지 않습니다.
