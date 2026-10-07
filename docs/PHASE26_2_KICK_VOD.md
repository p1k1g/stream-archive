# Phase 26.2 — KICK VOD

상태: 구현 및 자동 검증 진행 중. 실제 서비스 검증은 아래 수동 항목을 따로 확인합니다.

## 인증과 분석

- `https://kick.com/{channel}/videos/{UUID}` 형식만 지원합니다.
- `POST https://web.kick.com/api/v1/stream/{UUID}/playback`에서 `playback_url.vod`를 사용합니다. `live` 주소로 대체하지 않습니다.
- 설정의 `KICK_SESSION_TOKEN`은 기존 DPAPI / Secret Service / Keychain 경계에서 보호됩니다. UI는 저장 여부만 읽고 평문을 다시 표시하지 않습니다.
- 인증정보 삭제는 SQLite 설정을 비우면서 이전 native 참조를 정리 대기로 원자적으로 보관한 뒤 Linux Secret Service / macOS Keychain 항목을 삭제합니다. DB 전환 실패 시 native 항목을 삭제하지 않습니다. native 삭제 실패 시 정리 참조를 유지하고 오류를 반환하며, 같은 삭제 명령으로 재시도합니다. Unix CLI는 `providers clear-secret KICK_SESSION_TOKEN`을 사용합니다. Windows DPAPI는 별도 credential-store 항목이 없으므로 SQLite의 암호화 값만 제거합니다.
- `session_token` Cookie는 인코딩을 유지하고 Bearer만 percent-decode합니다. 요청마다 저장된 값을 읽으므로 교체 후 재시작할 필요가 없습니다.
- 미설정 상태에서는 인증 없이 조회합니다. 주소가 없으면 구독 인증 설정을 안내합니다. 설정된 인증정보도 권한을 보장하지 않습니다.
- Cookie/Bearer는 KICK playback 요청에만 전달합니다. CDN, yt-dlp와 FFmpeg에는 전달하지 않습니다. HTTP redirect를 자동으로 따라가지 않습니다.
- 영상 UUID·채널·VOD 상태를 확인하고 DRM, 허용되지 않은 CDN, 빈 재생 주소는 거부합니다. 401/403/404/429는 구분해 안내합니다.
- 제목·채널·길이는 playback 응답, 화질은 HLS master playlist에서 얻습니다. thumbnail sheet는 표지 사진이 아니므로 임의로 잘라 표시하지 않습니다. core에서 FFmpeg로 HLS 첫 프레임 한 장을 최대 480×270으로 추출하며, 실패하면 KICK 플랫폼 로고를 유지합니다.

## 파일과 중단 정책

- 화질 선택 후 해당 variant playlist를 yt-dlp native HLS downloader에 전달합니다. `--concurrent-fragments 4`, `--hls-use-mpegts`, `--fixup never`로 실제 `.ts`를 직접 저장하며 MP4 remux는 하지 않습니다.
- `--abort-on-unavailable-fragments`로 누락 조각을 성공 처리하지 않습니다. 사전 playlist ENDLIST·전체 길이와 조각 수, 정상 exit, 동일 조각 수의 전체 완료, 완료 marker, MPEG-TS sync byte를 확인한 뒤 COMPLETED / History / 완료 알림을 처리합니다.
- UUID가 포함된 고유한 `*.partial.ts`와 yt-dlp의 `.part` / `.ytdl` / fragment 파일을 사용합니다. 취소·실패 시 부분 파일들을 보존합니다. 자동 재시도는 `max_retries.max(1)`의 총 시도 횟수를 따릅니다(기본 5회, 0은 1회). 각 시도는 새 UUID로 처음부터 시작하며 이전 파일을 덮어쓰지 않습니다. 다운로드 실패 사이에는 최대 5초의 취소 가능한 대기를 적용하고, 취소 또는 최종 파일 게시 실패는 재시도하지 않습니다.
- 취소는 앱이 소유한 yt-dlp process tree만 종료합니다. 기존 SOOP/CHZZK 방식은 변경하지 않습니다.
- Windows 발행은 MoveFileW, Unix는 같은 디스크 hard-link 후 임시 이름 제거로 처리합니다. Unix 저장 파일시스템은 hard-link를 지원해야 합니다.
- TS의 Explorer 썸네일은 PC 환경에 따라 달라질 수 있습니다. 사용자의 독립 명령 테스트에서는 표시됐지만 새 앱 binary의 전체 영상 검증은 별도 수동 RC입니다.
- LIVE 이미지 요청에 KICK Origin / Referer를 전달합니다. 이미지 실패 시 현재 broadcast ID를 재확인하고 public playback URL에서 첫 프레임을 추출합니다. 실패 시 플랫폼 로고로 fallback하며 로그에는 고정 오류 분류만 남깁니다.
- VOD 다운로드에는 yt-dlp가 필요하고 LIVE/VOD 첫 프레임 썸네일에는 FFmpeg가 필요합니다. 외부 도구는 공식 패키지에 번들하지 않습니다.
- 내부 playback API는 공개 계약이 아니며 로그인 자동화·Cloudflare 우회는 추가하지 않습니다.

## native credential 교체와 정리

KICK 토큰 교체는 새 참조와 이전 native 참조의 정리 대기 기록을 기존 SQLite `settings` 테이블에 함께 commit합니다. commit 실패 시 새로 만든 KICK native credential을 회수합니다. commit 후 이전 항목을 삭제하고, 삭제 실패 시 opaque 참조만 정리 대기로 보존하여 다음 저장/삭제에서 재시도합니다. 정리 대기에는 평문 토큰을 저장하지 않으며 DB schema migration은 없습니다.

Linux/macOS에서 Backup을 Restore할 때 `KICK_SESSION_TOKEN`과 native 정리 대기 참조는 복원 직전 canonical DB의 현재 상태를 유지합니다. 토큰을 교체했다면 현재 토큰을 사용하고, 삭제했거나 설정하지 않았다면 미설정을 유지합니다. 오래된 백업의 native 참조로 되돌리지 않습니다. 나머지 설정·채널·History 등은 기존대로 복원하며 Windows DPAPI 복원 방식은 변경하지 않습니다.

## 오류 로그

진단의 런타임 로그에 `[VOD:KICK:ERR]`와 job ID, 실패 단계, HTTP 상태 또는 안전한 오류 분류를 남깁니다. `playback.request` / `playback.http` / `playback.json` / `playback.vod_missing`, `cdn.hls.request` / `cdn.hls.http`, `download.ytdlp.exit` / `download.incomplete`로 조회·CDN·파일 기록 실패를 구분합니다. yt-dlp 실패는 exit 상태, 기록 바이트와 진행 시간을 함께 남깁니다.

Cookie의 expiry가 길어도 세션 무효화·시청 권한·API 변경은 별개의 문제이므로 HTTP 401이나 빈 재생 주소를 만료로 단정하지 않습니다. Cookie/Bearer, 재생 URL, 원본 응답 JSON과 FFmpeg stderr는 로그에 출력하지 않습니다. 런타임 로그는 메모리의 제한된 최근 기록이며 자동 영구 파일 로그로 간주하지 않습니다.

## 자동 검증

- URL/영상 identity, 인증 헤더 인코딩·주입 거부, DRM/CDN 거부, HLS 화질 선택·목록 순서 변경, yt-dlp 명령의 병렬/TS/누락 거부/인증 미전달을 검증합니다.
- 공통 notification epoch 전환에 KICK을 포함하며 기존 SOOP/CHZZK 검증을 유지합니다.
- 이전 MP4 방식의 검증 기록(2026-10-06): 로컬 FFmpeg에서 생성한 H.264/AAC HLS의 직접 fragmented MP4 저장, ffprobe 컨테이너·코덱, JPEG 프레임 추출을 확인했습니다. 15초 테스트 입력을 다운로드 중 강제 종료했을 때 기록이 끝난 fragment는 ffprobe로 읽을 수 있었습니다. 첫 fragment 완료 전 종료는 재생을 보장하지 않습니다. 실제 provider 테스트로 간주하지 않습니다.

## 수동 RC

- [ ] 공개 KICK VOD 분석·다운로드
- [ ] 유효한 구독의 KICK VOD 분석·다운로드 및 전체 길이·소리 확인
- [ ] 만료 토큰 / 비구독 계정 / 삭제 영상 안내
- [ ] 토큰 저장·교체·삭제·재실행 후 유지, native secret store 확인
- [ ] 화질 선택 / 저장 폴더 선택 / Queue / History / 완료 알림
- [ ] 다운로드 중 취소, 별도 FFmpeg 프로세스가 영향받지 않는지 확인
- [ ] 강제 종료 후 partial.ts / .part 파일 보존, 재시도 시 새 파일 시작 확인
- [ ] Windows Explorer 썸네일·seek·Windows 기본 플레이어 호환성
- [ ] 디스크 부족 시 실패 안내 및 부분 파일 보존
- [ ] SOOP/CHZZK LIVE/VOD와 KICK LIVE regression

사용자가 수행한 Cookie+Bearer playback 성공과 Cookie 없는 HLS 다운로드는 설계 근거입니다. 새 앱 binary에서의 성공으로 기록하지 않습니다.

KICK native credential은 생성 전에 opaque cleanup 참조를 SQLite에 기록합니다. 새 값 commit 시 해당 참조를 cleanup 목록에서 원자적으로 제거하므로 DB가 이후 쓰기 불가능해져도 rollback 정리 대상을 복구할 수 있습니다. KICK 등록·교체·삭제는 DB별 파일 잠금으로 CLI observer 간 충돌을 막으며, 잠금 실패 시 재시도를 안내합니다.

## playback / CDN 요청 호환성 보완

KICK VOD playback과 HLS 조회는 기존 provider 조회와 같은 User-Agent를 사용하고 HTTP/1.1로 요청합니다. playback에는 `Accept: application/json`을 명시합니다. FFmpeg에도 동일 User-Agent와 KICK Origin / Referer를 전달하며 Cookie / Bearer는 전달하지 않습니다. proxy 동작은 변경하지 않습니다.

HTTP 403은 `cf-mitigated: challenge`가 확인되면 `reason=cloudflare_challenge`, 그 외에는 `reason=access_denied`로 구분합니다. 원본 응답이나 인증값을 로그에 남기지 않으며, `access_denied`만으로 토큰 만료 또는 Cloudflare 차단을 단정하지 않습니다.

2026-10-06 공개 VOD `nnabi/videos/01a1016e-7aa0-79f2-91cb-c0739f2466da`를 인증정보 없이 독립 요청으로 확인했습니다. playback HTTP 200 / `VIEWER_TIER_FREE`, HLS 조회와 FFmpeg 30초 샘플 MP4 저장을 확인했습니다. 결과는 1920×1080 H.264 + AAC, 30.018초, 30,250,489바이트이며 FFmpeg 종료 코드는 0입니다. 전체 18,846초 다운로드, Windows 앱 binary 및 구독자 전용 계정 검증은 수행하지 않았으며 위 수동 RC 항목을 완료 처리하지 않습니다.

회귀 테스트는 실제 loopback HTTP 요청의 User-Agent / HTTP version / playback 인증 헤더와 동일 client의 CDN 요청에 Cookie / Bearer가 없음을 검증합니다. 테스트의 proxy 비활성화는 loopback fixture에만 적용합니다.

## KICK LIVE 썸네일 확인

26.1 이후 LIVE 조회·썸네일 경로의 코드 변경은 없었습니다. 2026-10-06 `kaneljoseph` API는 방송 ID `130830473`과 기존 thumbnail URL을 정상 반환했지만, 해당 이미지 CDN은 현재 요청 헤더 및 Origin / Referer 추가 요청 모두 HTTP 403 / XML AccessDenied를 반환했습니다. 이 사실만으로 모든 계정이나 환경의 썸네일 문제가 같은 원인이라고 단정하지 않습니다.

썸네일 실패는 플랫폼 로고로 대체하며, 진단 런타임 로그에 `[LIVE:THUMBNAIL:ERR] platform=KICK reason=http_403` 등의 안전한 분류를 남깁니다. 인증정보·썸네일 URL·원본 오류 본문은 기록하지 않습니다. 기존 LIVE 새로 고침 재시도와 방송 종료 시 캐시 정리를 유지합니다. 임의 URL 변경, 구독 토큰의 이미지 CDN 전달, Cloudflare 우회는 추가하지 않습니다. 실제 PC에서 새로 고침 후 결과와 이 로그를 확인해야 하며, 이번 관측으로 썸네일 복구 완료를 주장하지 않습니다.

KICK 토큰 분석은 security 경계에서 저장·삭제와 동일한 DB별 잠금을 획득한 뒤 cache refresh와 native secret 조회를 함께 수행합니다. 조회 중 참조 삭제를 막고 HTTP 요청 전에 잠금을 해제합니다. 저장·삭제는 기존 즉시 try-lock 계약을 유지하고, 분석의 읽기 잠금은 shared lock으로 최대 5초 기다립니다. 짧은 credential 갱신 때문에 Queue를 즉시 실패 처리하지 않으며, 동시 읽기는 허용합니다. Linux/macOS 조회는 동일 CLI/headless executable의 내부 helper에서 수행하여 async executor를 막지 않습니다. helper는 기존 DB 잠금과 native secret store를 사용하며, 잠금 대기·DB 열기·native 조회·응답 수신을 합쳐 최대 10초로 제한합니다. 취소나 시간 초과 시 platform_runtime을 통해 앱이 소유한 helper process group과 하위 프로세스를 종료하고 root를 회수하여 잠금을 해제합니다. 기존 process group 종료 유예 시간이 추가될 수 있습니다. 토큰은 pipe로만 전달하고 argv·환경변수·임시 파일·오류 로그에 기록하지 않습니다. Windows는 기존 DPAPI 경로를 유지합니다. 시간 초과나 다른 I/O 오류는 실패로 안내합니다. 회귀 테스트는 writer 경합, 교체·삭제 반영, 복호화 중 잠금 유지와 복호화 실패 뒤 잠금 해제를 검증합니다.

멈춘 native 조회의 시간 초과/취소 후 잠금 해제, 무관한 프로세스 생존, helper 응답 실패·크기 초과·UTF-8 오류의 안전한 처리, Linux fake secret-tool을 이용한 실제 Queue 취소 및 runtime 종료를 회귀 테스트로 검증합니다. 실제 Secret Service / Keychain 세션 수동 QA는 위 RC 항목으로 남깁니다.

내부 helper는 OS가 확인한 직접 부모가 동일 실행 이미지인 경우에만 native 조회를 허용합니다. Linux는 /proc의 실행 이미지 device/inode, macOS는 실행 경로와 커널 CDHash를 비교하며, 확인 실패 시 DB/native 조회나 stdout 토큰 전달을 수행하지 않습니다. 내부 명령을 외부 프로세스에서 직접 실행하는 경우는 거부합니다.

## KICK 인증정보와 Restore 정책

사용자 선택에 따라 Linux/macOS Restore는 현재 KICK 인증정보와 cleanup journal을 유지합니다. 동일 DB별 인증정보 잠금 아래에서 최신 canonical 행을 읽고, SQLite 내부 임시 DB에 복원 대상과 현재 두 행을 먼저 구성한 뒤 canonical DB로 복원합니다. 원본 백업과 체크섬은 변경하지 않고 native secret을 복호화하거나 다시 저장하지 않습니다. 준비 실패나 인증정보 잠금 경합 시 복원하지 않으며, 복원 도중 오래된 토큰/cleanup 참조가 되살아나는 중간 상태를 만들지 않습니다. 기존 safety backup과 runtime-owner/LIVE/VOD/Queue 사전 조건을 유지합니다.

자동 테스트는 현재 토큰 교체·삭제·미설정, 오래된 owning-store cache와 최신 observer 값, cleanup journal 유지, 다른 provider 설정의 정상 복원, 원본 백업 불변, 준비 실패/잠금 경합 시 live DB 보존, managed Restore와 safety backup을 검증합니다. 실제 native store 세션 수동 QA는 완료 처리하지 않습니다.

## 2026-10-07 다운로드 방식 변경 검증

사용자의 독립 PowerShell 테스트에서 FFmpeg MP4와 앱 MP4의 전체 길이가 같고 Explorer 썸네일이 표시됐습니다. yt-dlp TS 테스트도 진행 중 썸네일 표시와 빠른 전송을 확인했지만, 최종 완료·끝부분 재생은 아직 별도 확인 대상입니다. 이 관측을 새 앱 binary의 수동 QA 완료로 기록하지 않습니다.

- [ ] 새 앱에서 KICK LIVE CDN 실패 → 첫 프레임 fallback 확인
- [ ] 새 앱에서 공개/구독 VOD 분석 썸네일 확인
- [ ] TS 전체 다운로드, 선택 화질, 영상 길이와 끝부분 재생·탐색 확인
- [ ] 취소, 조각 실패, 디스크 부족 시 성공 알림이 나오지 않는지 확인
