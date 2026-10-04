# Phase 25.6 — VOD 이어받기 안정화 및 분석 썸네일

상태: 구현 및 PR 검증 중. 실제 계정 수동 검증은 아직 완료하지 않았습니다.

## SOOP 인증 갱신과 이어받기

기존 Rust VOD는 다운로드 실패마다 `.part`와 `.ytdl`을 지워 `--continue`가 있어도 같은 PART를 처음부터 받았습니다. 이번 변경은 한 작업의 자동 재시도 동안 fragment checkpoint와 다운로드 조각을 보존합니다.

- 재시도 전 로그인/브라우저/파일 Cookie와 최신 메타데이터를 다시 읽습니다.
- 다운로드 HTTP 401/403이 확인되고 구독 영상의 재사용 가능한 로그인 Cookie가 있으면 `private_auth`도 다시 발급받습니다. FILE 모드의 signed Cookie만으로 로그인 권한을 새로 만들지는 않습니다.
- 새 프로세스는 동일 출력 경로, 동일 화질, `--continue`를 사용합니다. 서명 query 변경은 허용하지만 영상 경로·방송자·PART 수가 달라지면 기존 조각을 혼합하지 않고 실패 처리합니다.
- 실패한 최종 출력 파일을 완료 파일로 오인하지 않도록 제거하되 `.part`와 `.ytdl`은 재시도 동안 보존합니다.
- 취소, 재시도 소진, 인증/메타데이터 단계의 최종 실패에서는 해당 PART의 임시 파일을 정리합니다. 정상 완료한 다른 PART는 유지합니다.
- 앱 종료 후 재실행이나 별도 Queue 재시도 작업의 이어받기까지 보장하는 변경은 아닙니다. yt-dlp가 사용하는 다운로드 방식이 checkpoint를 지원해야 합니다.
- 권한 거부를 우회하거나 실패한 조각을 건너뛰지 않습니다. owned-process lifecycle과 취소 계약을 유지합니다.

## 분석 결과 썸네일

- SOOP: yt-dlp `thumbnail`/`thumbnails`, 필요할 때 SOOP API `thumb`를 사용합니다.
- CHZZK: API `thumbnailImageUrl`, 구형 응답의 `videoImageUrl`을 사용합니다.
- 분석 정보 카드 오른쪽에 원본 비율을 유지해 표시합니다. 없거나 실패하면 해당 플랫폼 로고를 표시합니다.
- UI는 `StreamArchiveCore.vod_thumbnail`을 호출합니다. UI에서 provider HTTP·SQLite·외부 프로세스를 직접 호출하지 않습니다.
- provider별 이미지 CDN allowlist, HTTPS, redirect 금지, 응답/디코딩 크기 제한, timeout을 유지합니다. Cookie나 로그인 정보는 이미지 요청에 보내지 않습니다.
- 이미지 캐시는 메모리의 현재 분석 한 장이며 디스크에 저장하지 않습니다. 한 번에 VOD 이미지 요청 하나만 수행합니다.
- URL 변경·동일 URL 재분석에서 이미지를 비우고 generation과 job ID로 오래된 응답을 거부합니다. 이미지 오류는 VOD 분석/다운로드 상태를 실패로 바꾸지 않습니다.

## 자동 검증과 실제 계정 검증의 구분

fixture는 20.9%에서 HTTP 403을 발생시키고 `.part`와 fragment checkpoint를 남깁니다. 다음 시도가 해당 파일과 `--continue`를 유지하고 기존 prefix를 포함해 완료하는지 확인합니다. fixture 성공은 실제 SOOP 장시간 구독 VOD의 만료 복구 성공을 의미하지 않습니다.

### 남은 수동 검증

- [ ] 공개 SOOP VOD 분석·썸네일·화질/PART 선택·다운로드·파일 재생
- [ ] 긴 구독 SOOP VOD에서 인증 만료 후 갱신 및 중단 지점 이어받기
- [ ] 이어받은 파일의 중단 지점 전후 영상/음성 확인
- [ ] 실제 구독 권한 거부 시 다운로드 실패 확인
- [ ] 취소 및 재시도 소진 시 임시 파일 정리 확인
- [ ] CHZZK VOD 썸네일·기존 다운로드·파일 재생
- [ ] 빠른 URL 변경/동일 URL 재분석에서 이전 썸네일이 붙지 않음
- [ ] 이미지 없는 VOD/이미지 요청 실패 시 로고 fallback과 분석/다운로드 유지
- [ ] Queue/History, LIVE 썸네일, 알림 및 트레이 기존 동작 확인

KICK, DB migration, renderer 변경, release publication은 이 Phase의 범위가 아닙니다.
