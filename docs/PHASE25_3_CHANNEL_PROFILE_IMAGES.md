# Phase 25.3 — LIVE 채널 프로필 이미지

> 이 문서는 Phase 25.3 구현 당시의 정책입니다. Phase 25.4에서는 채널 프로필 이미지를 플랫폼 로고 / 방송 썸네일로 전환합니다. [현재 정책](PHASE25_4_LIVE_THUMBNAILS_FOLDER_PICKER.md)을 참고하세요.

상태: 구현 완료, PR 검토 중. 자동 검증 결과는 PR CI에서 확인하며 Windows 수동 QA는 미완료입니다. merge는 사용자가 결정합니다.

## 범위

LIVE 채널 행에 SOOP·CHZZK의 공개 프로필 이미지를 표시합니다. 이미지 아래의 작은 플랫폼 이름으로 SOOP / CHZZK를 구분합니다. 조회 중, 프로필 없음, 네트워크 오류, 잘못된 파일은 기존 플랫폼 표시로 대체합니다. 채널 관리 화면의 편집 구조는 유지합니다.

프로필 조회는 `StreamArchiveCore` → 공통 profile service → 플랫폼 provider 경계를 따릅니다. SQLite schema, 비밀정보 저장, LIVE watcher / 녹화 / VOD / Queue 동작은 변경하지 않습니다. UI에서 HTTP나 파일 저장을 직접 수행하지 않습니다.

## 이미지 규격

2026-10-03 공개 응답 샘플을 확인했습니다.

| 플랫폼 | 확인한 이미지 | 실제 규격 / 형식 |
| --- | --- | --- |
| SOOP | `https://stimg.sooplive.com/LOGO/10/1004ysus/1004ysus.jpg` | 200×200, GIF |
| CHZZK | 채널 `1a1dd9ce56fb61a37ffb6f69f6d5b978`의 `channelImageUrl` | 1002×1025, PNG |

이는 확인한 샘플 규격이며 플랫폼 전체의 최소·최대 규격을 보장하는 공식 명세는 아닙니다. 두 샘플 중 작은 SOOP 기준으로 **최대 200×200**의 정사각형 이미지를 사용합니다. 큰 이미지는 중앙 crop 후 축소하고, 더 작은 원본은 확대하지 않습니다. 화면 표시 영역은 40×40 logical pixel입니다. GIF / WebP 애니메이션은 첫 프레임만 사용합니다. 확장자 대신 파일 내용으로 PNG / JPEG / GIF / WebP 형식을 판별합니다.

CHZZK는 기존 채널 조회 endpoint의 `channelImageUrl`을 사용하며 응답 `channelId` 일치를 확인합니다. SOOP은 provider 내부에서 canonical 공개 프로필 경로를 구성합니다. 인증이나 쿠키는 사용하지 않습니다.

## 요청 및 메모리 제한

- 동시에 최대 2개의 프로필 요청만 수행합니다. 메타데이터 조회와 이미지 다운로드는 비동기이고, 이미지 decoding은 blocking worker에서 수행합니다.
- 전체 요청 timeout 12초, 개별 HTTP 요청 timeout 8초입니다.
- CHZZK 메타데이터 최대 256 KiB, 이미지 응답 최대 8 MiB입니다. `Content-Length`와 실제 수신량을 모두 검사합니다.
- decoding 최대 가로·세로 4096, 최대 allocation 64 MiB입니다. decoding 중인 blocking 작업은 timeout 뒤에도 자체 제한 안에서 끝날 수 있습니다.
- provider별 허용된 HTTPS host만 다운로드하며 redirect를 따라가지 않습니다. 그 외 주소는 fallback 처리합니다.
- LIVE 목록의 처음 128개 채널까지 프로필 캐시를 사용합니다. 그 이후 행은 플랫폼 표시를 유지하며 녹화 기능에는 제한이 없습니다.
- 정규화한 RGBA 캐시는 최대 약 19.5 MiB입니다. renderer가 관리하는 texture 및 일시적인 decoding 메모리는 별도입니다.
- 성공 캐시 1시간, 실패 캐시 5분입니다. LIVE 상태 갱신 때 만료된 항목을 다시 조회합니다. 사라진 채널의 캐시를 제거하고, 현재 목록에서 제외된 채널의 늦은 응답은 버립니다.
- 캐시는 메모리에만 두며 앱 종료 시 사라집니다. 이미지 파일·쿠키·credential을 package나 runtime data에 저장하지 않습니다.

## 자동 검증

자동 테스트는 정사각형 crop / 축소 / 작은 원본 유지, 손상된 파일·초과 해상도·초과 응답 크기·미지원 형식 거부, provider URL 제한, CHZZK identity 검증을 다룹니다. GUI 캐시 테스트는 동시 요청 제한, 실패 캐시, 삭제된 채널의 늦은 응답, 캐시 상한과 만료 후 재시도를 검증합니다.

Rust fmt / unit tests / check / clippy와 Windows Slint / RuntimeContracts / Windows·Linux·macOS package 검증은 PR CI 결과로 확인합니다. 실제 Windows 시각 검증은 아래 수동 항목으로 남깁니다.

## 수동 QA — 아직 미완료

- [ ] SOOP / CHZZK 프로필과 플랫폼 이름이 올바른 채널에 표시됨
- [ ] GIF 프로필이 정지된 첫 프레임으로 표시됨
- [ ] 이미지 없는 채널 / 네트워크 차단 / 삭제된 채널에서 플랫폼 fallback 유지
- [ ] 기본 창 크기 및 창 크기 변경 시 이미지·제목·상태·action 정렬 확인
- [ ] FHD / QHD 100% 및 가능한 경우 125% / 150% DPI 확인
- [ ] 채널 삭제·교체 중 늦게 도착한 이미지가 다른 채널에 적용되지 않음
- [ ] LIVE 녹화 / 감시 시작·중지 / 트레이 이동 동작 유지
- [ ] 장시간 실행 시 private memory / handles / threads가 지속 증가하지 않음
- [ ] 1시간 후 프로필 갱신과 실패 후 재시도 확인

renderer / font 변경이나 메모리 trim은 포함하지 않습니다. 프로필 표시가 실제 provider 녹화 성공을 의미하지는 않습니다.
