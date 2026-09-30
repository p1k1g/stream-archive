# SOOP LIVE용 Cloudflare Worker 설정

현재 SOOP LIVE는 [backend/worker.js](../backend/worker.js)를 Cloudflare Workers에 배포해 playlist URL을 조회합니다. 활성 SOOP 채널이 있으면 HTTPS Worker URL과 Worker API key가 필요합니다. 파일명은 `worker.js`이며 `workers.js`가 아닙니다.

CHZZK LIVE/VOD와 SOOP VOD 다운로드 경로에는 이 Worker를 사용하지 않습니다. Worker 설정 없이 CHZZK만 사용할 경우 SOOP 채널을 활성화하지 마세요. 이 문서는 설정 절차이며 실제 Cloudflare 배포나 SOOP 세션 검증 완료를 뜻하지 않습니다.

## 준비할 것

- 직접 관리하는 Cloudflare 계정과 Workers 사용 환경
- 사용하는 Stream Archive revision의 `backend/worker.js`
- Worker와 앱에 함께 설정할 비밀값(아래 `API_SECRET`)
- SOOP LIVE를 사용하는 데 필요한 계정·설정

Worker는 LIVE URL 조회용이며 녹화 파일을 저장하는 서버가 아닙니다. 실제 미디어 처리는 로컬 Streamlink/FFmpeg가 수행합니다. URL 조회 과정에 필요한 SOOP cookie·방송 비밀번호가 Worker에 전달될 수 있으므로 본인이 관리하고 신뢰하는 Worker를 사용합니다.

## 1. Worker 생성 및 코드 배포

Cloudflare dashboard에서 새 Worker를 만들고 `backend/worker.js` 전체를 코드 편집기에 넣어 배포합니다. 이 파일은 `export default { async fetch(request, env) ... }` 형태의 JavaScript Worker입니다. 기본 예제 코드 위에 덧붙이지 말고 해당 entry 코드를 교체합니다.

Cloudflare의 메뉴와 생성 흐름은 변경될 수 있으므로 [공식 dashboard 시작 안내](https://developers.cloudflare.com/workers/get-started/dashboard/)를 기준으로 진행하세요. 배포한 Worker의 HTTPS 주소를 기록합니다.

## 2. `API_SECRET` 설정

배포한 Worker의 **Settings → Variables and Secrets**에서 다음 secret을 추가하고 배포에 반영합니다. 자세한 메뉴는 [Cloudflare Secrets 안내](https://developers.cloudflare.com/workers/configuration/secrets/)를 참고하세요.

| 구분 | 값 |
|---|---|
| 종류 | Secret |
| 이름 | `API_SECRET` |
| 값 | 직접 생성한 충분히 긴 무작위 비밀값 |

비밀값은 Worker source에 직접 쓰지 않습니다. Worker는 요청의 `X-API-Key` header와 `env.API_SECRET`을 비교하며, secret이 없거나 일치하지 않으면 `401 Unauthorized`를 반환합니다.

**이 값은 Cloudflare 계정의 Global API Key나 API Token이 아닙니다.** Stream Archive와 이 Worker 사이의 요청 인증에만 쓰는 별도 비밀값입니다.

## 3. Stream Archive에 연결

Windows Native의 **설정 → 일반**에서 다음을 설정하고 저장합니다.

| 앱 항목 | 저장 키 | 입력할 값 |
|---|---|---|
| Cloudflare Worker URL | `CLOUDFLARE_WORKER_URL` | `https://<worker-name>.<subdomain>.workers.dev/soop/url` |
| Worker API key | `CLOUDFLARE_API_KEY` | Worker의 `API_SECRET`과 정확히 같은 값 |
| SOOP 계정 ID | `SOOP_USERNAME` | 사용하는 SOOP 계정 ID |
| SOOP 비밀번호 | `SOOP_PASSWORD` | 사용하는 SOOP 비밀번호 |

**Worker URL에는 `/soop/url`까지 포함합니다.** 앱은 입력한 URL에 그대로 POST하며 경로를 자동으로 덧붙이지 않습니다. Worker root나 `/health`를 입력하면 URL 조회가 정상 동작하지 않습니다. 사용자 지정 domain을 사용해도 같은 `/soop/url` 경로를 지정합니다.

Linux/macOS에서는 CLI의 provider 설정을 사용합니다.

```bash
./bin/stream-archive-cli providers set CLOUDFLARE_WORKER_URL https://example-worker.example-subdomain.workers.dev/soop/url
./bin/stream-archive-cli providers set SOOP_USERNAME my-account
./bin/stream-archive-cli providers secret CLOUDFLARE_API_KEY --stdin
./bin/stream-archive-cli providers secret SOOP_PASSWORD --stdin
```

위 예시 URL을 본인의 실제 Worker URL로 바꾼 뒤 실행합니다. 비밀값은 `--stdin`을 통해 입력하며 command 인자로 붙이지 않습니다. 자세한 사용법은 [Unix CLI 가이드](UNIX_CLI.md)를 따릅니다.

## 4. 연결 및 실제 서비스 확인

1. Windows 설정을 저장한 뒤 **SOOP + Worker 테스트**를 실행합니다.
2. 인증 테스트 결과를 확인합니다. 이 테스트는 SOOP 로그인 및 Worker endpoint 인증 확인이며 녹화 성공을 보장하지 않습니다.
3. 실제 SOOP 채널을 등록하고 저장한 뒤 LIVE Watcher를 시작합니다.
4. 방송 중인 채널에서 URL 조회, 녹화 시작·진행·종료, 출력 파일 및 History를 확인합니다.
5. 최종 결과는 [수동 RC 체크리스트](MANUAL_RC_1_0_0.md)에 기록합니다.

Worker의 endpoint는 다음과 같습니다.

| 요청 | 역할 | 확인 시 주의사항 |
|---|---|---|
| `GET /health` | Worker 응답 확인 | `X-API-Key` 인증 필요. 성공해도 SOOP 녹화 검증은 아님 |
| `POST /soop/url` | SOOP LIVE playlist URL 조회 | `X-API-Key` 인증과 유효한 방송 정보 필요 |

앱의 인증 테스트는 빈 JSON을 POST하므로 인증이 성공해도 방송 정보가 없어 `400`이 나올 수 있으며 현재 앱은 이를 endpoint 확인 결과로 허용합니다. 실제 방송 URL 조회·녹화는 별도로 검증해야 합니다.

## 문제 해결

| 증상 | 확인할 사항 |
|---|---|
| `401 Unauthorized` | 배포 환경의 `API_SECRET` 존재 여부, 앱 Worker API key와 일치 여부 |
| `404 Not Found` | 앱 URL이 HTTPS 주소와 `/soop/url`을 포함하는지, `backend/worker.js`가 배포됐는지 |
| `400` / `account_bno_rmd_required` | 실제 URL 조회 요청에 방송 정보가 있는지. 빈 JSON 인증 테스트와 구분 |
| `502`, `stage: aid` / `stage: assign` | SOOP 응답·방송 상태·필요한 인증 조건 확인. Worker 배포 성공과 서비스 URL 조회 성공은 별개 |
| 활성 SOOP 채널에서 Watcher 시작 실패 | HTTPS Worker URL, Worker API key, 필요한 SOOP 설정과 외부 도구 확인 |

비밀값·cookie·token 또는 `aid`가 들어간 playlist URL을 Issue나 공개 로그에 올리지 않습니다. secret을 바꾸면 앱의 Worker API key도 함께 갱신하고 다시 저장·테스트합니다.
