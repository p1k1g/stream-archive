# Phase 25.6 작업 체크포인트

이 문서는 사용량 제한이나 세션 변경 후 이어서 검증하기 위한 작업 기록입니다. 사용량 비율은 assistant에서 조회할 수 없으므로 1% 자동 감지는 하지 않습니다.

## 기준 상태

- repository: `p1k1g/stream-archive`
- branch: `phase25-6-vod-resume-thumbnails`
- base main: `9d34a172dc061bb5fcd1885e1e63cffeed21aae6` (Phase 25.5 PR #121 merge)
- main 직접 commit/push, PR merge, tag 및 GitHub Release 생성 금지

## 구현한 내용

- SOOP 자동 재시도에서 `.part` / `.ytdl` 보존; 취소·재시도 소진·인증/metadata 최종 실패는 PART guard에서 정리
- 다운로드 401/403 오류를 stderr drain 후 확인하고 구독·로그인 session의 `private_auth` 갱신
- 실패한 final output은 제거하되 fragment checkpoint는 유지; 서명 query 외 영상 경로/방송자/PART 수 변경 시 혼합 이어받기 거부
- 20.9% 403 fixture와 fragment checkpoint/prefix 보존 테스트
- SOOP / CHZZK VOD analysis의 optional thumbnail URL 및 core 이미지 로딩
- Slint 분석 정보 카드 오른쪽 썸네일/플랫폼 로고, 비율 유지, 한 요청 제한
- URL 변경/동일 URL 재분석의 generation + job ID stale response 차단
- provider 이미지 CDN 제한, 기존 bounded decode/timeout 재사용, DB migration 없음

## 현재 검증 기록

- Runtime SOOP 관련 테스트 통과; strict clippy 통과
- Runtime 전체 unit test는 UTC에서 221개 통과, 기존 도구 탐색 테스트 1개 실패
- 기존 FFmpeg 탐색 테스트는 시스템 FFmpeg 때문에 원본 main에서도 실패를 재현함
- 환경 `TZ=America/Anchorage`에서는 기존 History 날짜 테스트도 실패하며 원본 main에서 재현함. UTC에서는 통과함
- 로컬 GUI check는 `pkg-config`/fontconfig 개발 의존성 부족으로 차단됨; product 소스 문제가 확인된 것은 아님
- Windows compile / GUI tests / 패키지 / RuntimeContracts / Linux·macOS release smoke는 CI 확인이 남음
- 실제 계정 장시간 인증 만료 및 이어받기, 실제 썸네일 UI 수동 검증은 미수행

## 이어서 할 작업

1. remote branch / PR head / `git status`부터 확인한다. 이전 브랜치나 main에서 수정하지 않는다.
2. fmt/check/clippy와 변경된 테스트를 확인하고 PR CI의 failing job/step/log를 조사한다.
3. Windows Slint compile 및 UI 테스트, RuntimeContracts, 각 OS 패키지 검증 전체 통과를 확인한다.
4. CI 통과 후 Ready for review 및 `@codex review`; 실제 지적은 수정하고 재검증·답변·재리뷰한다.
5. 결과에 맞게 이 체크포인트와 PR 본문을 갱신한다. PR은 merge하지 않는다.

세부 정책과 수동 QA: [Phase 25.6 문서](PHASE25_6_VOD_RESUME_THUMBNAILS.md).

## 장시간 PART 재시도 보완

PR #122 첫 구현 commit `627727b52b`는 모든 CI와 Codex review를 통과했습니다. 이후 사용자 실측(49분 영상에서 인증 만료 2회)에 따라 같은 PART의 누적 시도 제한을 연속 실패 제한으로 보완 중입니다. `.part` 실제 보존 크기 high-water mark를 넘은 경우만 횟수를 초기화하며, 반복 만료 7회 시도 성공 및 진척 없는 5회 종료 fixture를 추가합니다. 보완 commit의 CI/재리뷰는 PR 최신 head를 기준으로 확인해야 합니다.
