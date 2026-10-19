# Phase 27.4 — LIVE model 수명과 리소스 계측

상태: 구현·PR 검증 중. merge 및 릴리스 변경은 수행하지 않음.

1.0.1 장시간 기록에서 Private Bytes가 184.3MiB에서 926.6MiB로 증가했고, 증가와 정체가 반복됐다. 이 변경이 전체 증가 원인을 해결했다고 주장하지 않는다. 실제 Windows heap 비교와 장시간 실행은 수동 검증으로 남긴다.

## 화면과 runtime의 역할 분리

- 트레이 또는 다른 탭에서는 LIVE 상태와 썸네일 무효화를 계속 추적하되 화면 model을 반복 갱신하지 않는다.
- 숨김/탭 이동 때 model 행을 한 번 비워 model이 보관하던 이미지 참조를 해제한다. 활성 방송의 첫 썸네일 cache는 유지한다.
- 다시 LIVE를 표시하면 즉시 상태를 요청하고 새 snapshot을 받은 뒤 화면을 반영한다. 숨긴 동안의 중간 snapshot은 쌓지 않고 최신 하나만 보관한다.
- 표시 중에는 같은 VecModel을 사용한다. 채널 순서/개수가 같으면 변경된 행만 갱신하고, 채널 구조가 바뀐 경우에만 reset한다.
- 저장공간 자동 조회도 숨긴 상태에서는 요청하지 않는다. 이미 도착한 저장공간 결과는 최신 하나만 보관하고, LIVE 재표시 때 다시 조회해 반영한다.
- 녹화·감시·owned-process lifecycle과 provider의 재시도는 그대로 유지한다.

## 완료되거나 조회할 수 없는 방송의 썸네일

- 녹화 완료 시 이전 썸네일 URL을 제거한다.
- 녹화가 없는 채널의 조회/인증 오류는 이전 썸네일을 재사용하지 않는다. 수동 새로고침과 늦게 도착한 요청으로 이전 이미지가 복구되지 않도록 검증한다.
- 실제 recorder를 소유한 상태의 오류는 구분한다. 일시적인 LIVE 조회 오류만으로 녹화를 종료하지 않는다.
- 정상 LIVE 정보가 다시 확인되고 요청 조건이 충족되면 해당 방송의 썸네일을 표시한다.

## 진단 계측 켜기

기본값은 꺼짐이다. 적용 binary를 종료한 뒤 PowerShell에서 다음처럼 실행한다. 기존 실행 파일을 새 프로세스로 바꾸는 시점은 녹화 상태를 확인하고 사용자가 결정한다.

```powershell
$env:STREAM_ARCHIVE_RESOURCE_TRACE = '1'
try {
    & (Join-Path $env:USERPROFILE 'Downloads\stream-archive-windows-x64\StreamArchive.exe')
} finally {
    Remove-Item Env:STREAM_ARCHIVE_RESOURCE_TRACE -ErrorAction SilentlyContinue
}
```

환경변수는 새 프로세스가 시작할 때 상속된다. 이미 실행 중인 프로세스에는 적용되지 않는다. GUI 프로세스가 반환되더라도 해당 프로세스가 상속한 값은 유지된다.

기록 위치는 canonical SQLite 데이터 디렉터리 하위 `logs`다. 기본 portable 경로에서는 `data\logs\resource-lifecycle-v1.csv`이며 `STREAM_ARCHIVE_DATA_DIR` 사용 시 해당 경로를 따른다.

- UTC timestamp, StreamArchive PID, 창 표시 여부, 탭, 채널/녹화 수, 썸네일 entry/pending/raw RGBA 크기, 누적 LIVE polling/model reset/행 갱신 횟수를 기록한다.
- UI와 watcher 표본은 약 60초 간격이다. 창 표시/숨김·탭 변경, 채널 저장/적용, 녹화 시작/완료는 별도 event로 기록한다. 창 상태는 UI timer가 관찰한 시각이며 UI가 정지한 동안의 정확한 Win32 이벤트 시각을 보장하지 않는다.
- runtime event는 해당 event에 필요한 숫자 필드만 채운다. 빈 필드는 0을 의미하지 않는다. `ui_sample`의 채널/녹화 수는 마지막 UI snapshot이고 실제 background 상태 비교에는 `watcher_sample`을 사용한다.
- `owned_pid`는 앱이 소유한 recorder PID다. 빠른 PID 재사용을 구분하려면 timestamp/event 순서도 함께 본다.
- 계정, 방송 제목, 파일 경로, URL, 쿠키, 토큰, 인증값은 필드에 넣지 않는다. 지원하지 않는 탭 문자열은 `Other`로 기록한다.
- writer queue는 64개로 제한한다. 주기 표본은 종류별 최신 하나로 합치고 lifecycle 이벤트를 우선 보관한다. 포화 상태에서 새 lifecycle 이벤트는 주기 표본부터 제거하고, lifecycle 이벤트만 64개가 쌓이면 가장 오래된 이벤트를 제거해 최신 완료/중지 경계를 보관한다. queue의 짧은 메모리 갱신 동안만 lock을 사용하고 파일 I/O나 빈 queue 공간을 기다리지 않는다.
- `dropped`는 합쳐지거나 유실된 항목의 누적 횟수다. 증가하면 별도 `trace_gap` 행을 기록하며 마지막 owner 종료 때도 남은 유실 수를 기록한다. gap 또는 archive rotation이 있는 구간에서는 시작 이벤트에 대응하는 완료 행이 없다는 이유만으로 프로세스 잔존을 판정하지 않는다. I/O 실패나 강제 종료 시 로그 완전성은 보장하지 않는다.
- 파일은 1MiB, archive 3개로 총 약 4MiB 상한이다. `.1.csv`가 가장 최근 archive다. 용량을 초과하면 오래된 기록은 삭제되므로 비교 후 파일을 복사해 보관한다.
- I/O 실패 시 계측 writer만 비활성화한다. 마지막 서비스 owner가 해제될 때 queue를 비우고 writer thread를 join한다. read-only observer는 writer를 시작하지 않는다.
- 메모리 Working Set/Private Bytes/handles/threads는 기존 외부 BAT와 함께 기록한다. `rgba_bytes`는 CPU 원시 픽셀 기준이고 renderer/GPU 전체 크기가 아니다.

## 자동 회귀 검증

- 변경 없는 행과 진행률 행 갱신 반복 중 model identity 유지, 변경된 행만 통지. Slint 1.18의 빈 Image 비교를 명시적으로 처리하고 이미지 교체/제거도 검증한다.
- 숨긴 동안 1,000개 snapshot을 받아도 화면 갱신 없음, 최신 snapshot 하나만 보관.
- 재표시 후 새 snapshot을 기다리고 변경 상태 반영.
- 완료/오류/metadata 제거 후 수동 refresh 및 late response 차단, recorder 소유 상태의 이미지 유지.
- 완료 revision 갱신과 runtime URL 제거.
- 진단 파일 rotation/CSV 경계/주기 표본 병합/queue 포화 시 완료 이벤트 우선 보관/최종 유실 표시/마지막 owner의 thread 종료/기본 비활성화.
- 기존 fmt/unit/check/clippy, RuntimeContracts 및 Windows package 검증을 유지한다.

## Windows 비교 검증

- [ ] 같은 renderer/DPI/채널 목록으로 시작하고 외부 메모리 BAT와 opt-in trace를 함께 실행
- [ ] LIVE 표시 2~3시간 → 트레이 2~3시간 → 재표시 반복
- [ ] 채널 추가/수정, 녹화 시작/완료와 메모리 증가 시각 비교
- [ ] 숨긴 동안 model reset/row update가 반복 증가하지 않는지 확인
- [ ] 숨긴 동안 정상 녹화·감시 지속, 재표시 후 현재 상태/저장공간 반영
- [ ] 정지·조회 오류 채널의 이전 썸네일이 새로고침 후 되살아나지 않는지 확인
- [ ] 방송 전환·녹화 중 조회 오류·창 최소화/복원·트레이 복구 확인
- [ ] 동일 작업량에서 작업 완료 후 2분/5분 baseline, 필요 시 allocation stack과 USER/GDI/GPU 측정

renderer/FemtoVG, allocator, memory trim, DB schema, 설정 authority, 공식 package/release version은 변경하지 않는다. 실제 provider 인증 검증과 장시간 Windows QA를 수행한 것으로 표시하지 않는다.
