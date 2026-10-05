# Phase 26.2 — KICK VOD

상태: 구현 및 자동 검증 진행 중. 실제 서비스 검증은 아래 수동 항목을 따로 확인합니다.

## 인증과 분석

- `https://kick.com/{channel}/videos/{UUID}` 형식만 지원합니다.
- `POST https://web.kick.com/api/v1/stream/{UUID}/playback`에서 `playback_url.vod`를 사용합니다. `live` 주소로 대체하지 않습니다.
- 설정의 `KICK_SESSION_TOKEN`은 기존 DPAPI / Secret Service / Keychain 경계에서 보호됩니다. UI는 저장 여부만 읽고 평문을 다시 표시하지 않습니다.
- `session_token` Cookie는 인코딩을 유지하고 Bearer만 percent-decode합니다. 요청마다 저장된 값을 읽으므로 교체 후 재시작할 필요가 없습니다.
- 미설정 상태에서는 인증 없이 조회합니다. 주소가 없으면 구독 인증 설정을 안내합니다. 설정된 인증정보도 권한을 보장하지 않습니다.
- Cookie/Bearer는 KICK playback 요청에만 전달합니다. CDN과 FFmpeg에는 전달하지 않습니다. HTTP redirect를 자동으로 따라가지 않습니다.
- 영상 UUID·채널·VOD 상태를 확인하고 DRM, 허용되지 않은 CDN, 빈 재생 주소는 거부합니다. 401/403/404/429는 구분해 안내합니다.
- 제목·채널·길이는 playback 응답, 화질은 HLS master playlist에서 얻습니다. thumbnail sheet는 표지 사진이 아니므로 시안 이미지로 잘라 표시하지 않습니다. KICK 플랫폼 로고를 유지합니다.

## 파일과 중단 정책

- FFmpeg `-c copy`로 다운로드하면서 실제 fragmented MP4를 기록합니다. H.264/AAC 입력에서 `aac_adtstoasc`로 AAC 컨테이너 형식을 맞춥니다.
- 별도의 TS 원본과 전체 MP4 복사본을 만들지 않습니다. 최종 발행도 같은 디스크의 hard-link 후 임시 이름 제거로 처리합니다.
- 정상 종료, 비어 있지 않은 출력, 영상 끝까지의 진행률을 확인한 뒤 COMPLETED / History / 완료 알림을 처리합니다.
- 실행 중에는 고유한 `*.partial.mp4`에 기록합니다. 취소·실패 시 부분 파일을 보존하며 기존 파일을 덮어쓰지 않습니다.
- 취소는 소유한 FFmpeg에 `q`를 보내고 5초 내 끝나지 않으면 소유한 process tree만 종료합니다.
- **이번 단계는 FFmpeg MP4 이어 쓰기를 지원하지 않습니다. Queue 재시도는 새 파일로 처음부터 시작합니다.** 중단된 fragment를 자동 병합하거나 이어받았다고 표시하지 않습니다. `max_retries` 설정에 맞춰 파일을 반복 삭제·재다운로드하지 않습니다.
- fragmented MP4의 Windows Explorer 썸네일과 강제 종료 후 부분 재생은 실제 PC에서 확인해야 합니다. 일반 MP4의 `+faststart` 후처리나 전체 remux를 자동 추가하지 않습니다.
- 내부 web playback API와 player version 필드는 공개 API 계약이 아니며, KICK 변경 시 분석이 실패할 수 있습니다. 로그인 자동화·Cloudflare 우회는 추가하지 않습니다.

## 자동 검증

- URL/영상 identity, 인증 헤더 인코딩·주입 거부, DRM/CDN 거부, HLS 화질 선택·목록 순서 변경, FFmpeg 명령의 MP4/stream-copy/인증 미전달을 검증합니다.
- 공통 notification epoch 전환에 KICK을 포함하며 기존 SOOP/CHZZK 검증을 유지합니다.
- 로컬 FFmpeg에서 생성한 H.264/AAC HLS의 직접 fragmented MP4 저장, ffprobe 컨테이너·코덱, JPEG 프레임 추출을 확인했습니다. 실제 provider 테스트로 간주하지 않습니다.

## 수동 RC

- [ ] 공개 KICK VOD 분석·다운로드
- [ ] 유효한 구독의 KICK VOD 분석·다운로드 및 전체 길이·소리 확인
- [ ] 만료 토큰 / 비구독 계정 / 삭제 영상 안내
- [ ] 토큰 저장·교체·삭제·재실행 후 유지, native secret store 확인
- [ ] 화질 선택 / 저장 폴더 선택 / Queue / History / 완료 알림
- [ ] 다운로드 중 취소, 별도 FFmpeg 프로세스가 영향받지 않는지 확인
- [ ] 강제 종료 후 partial.mp4 재생, 재시도 시 새 파일 시작 확인
- [ ] Windows Explorer 썸네일·seek·Windows 기본 플레이어 호환성
- [ ] 디스크 부족 시 실패 안내 및 부분 파일 보존
- [ ] SOOP/CHZZK LIVE/VOD와 KICK LIVE regression

사용자가 수행한 Cookie+Bearer playback 성공과 Cookie 없는 HLS 다운로드는 설계 근거입니다. 새 앱 binary에서의 성공으로 기록하지 않습니다.
