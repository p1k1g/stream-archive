# Phase 27.2 — 메모리·리소스 lifecycle 보완

상태: 구현 및 PR 검증 중. merge·tag·Release publication은 수행하지 않음.

Phase 27.1에서 재현한 background task의 중복 생성·종료 후 유지와, 정적 분석으로 발견한 Unix control 및 외부 도구 출력의 크기 경계를 보완한다. Windows에서 이전에 관찰한 Private Bytes 약 956MB의 원인이 해결됐다고 주장하지 않는다.

## 구현 범위

### core background task 소유 및 종료

- 자동 백업과 VOD History 동기화는 core clone이 공유하는 task 소유자에 등록한다.
- 같은 task의 시작을 반복 호출해도 추가 생성하지 않는다. shutdown 요청 이후에는 다시 시작하지 않는다.
- shutdown 시작 때 stop을 전달하고 Queue/VOD/Watcher의 기존 owned-process 정리를 수행한 뒤 background task를 join한다.
- 마지막 주기 writer가 종료된 후 terminal VOD 상태를 SQLite에 반영한다.
- shutdown을 기다리는 future가 취소되어도 JoinHandle을 보관하여 후속 shutdown 호출이 계속 기다릴 수 있게 한다.
- 정상 shutdown 없이 마지막 owner가 해제될 경우에도 보관 task를 abort한다. 동기 SQLite 백업의 transaction/정리는 실행 중인 poll을 중간에 선점하지 않는다.

### Unix control I/O 경계

- 기존 64KiB message 제한을 읽기 완료 후가 아니라 buffer 확장 전에 적용한다.
- server request read/response write와 client connect/write에는 5초 제한을 둔다.
- client response 대기는 owned-process의 정상 종료 시간을 고려해 120초로 제한한다. 이 제한은 서버에서 실행 중인 stop/cancel 작업을 abort하지 않는다.
- 미완성 요청 및 읽지 않는 client 때문에 shutdown이 I/O를 무기한 기다리지 않도록 stop 신호를 함께 확인한다.
- 이미 수락한 stop/cancel 명령은 완료까지 기다린다. 정리 중인 future를 버려 task/process 소유권을 잃는 우회를 사용하지 않는다.
- 64KiB를 넘는 response는 같은 wire format의 명시적 오류로 응답한다. 많은 로그 때문에 이 오류가 발생하면 `logs --tail N`의 `N`을 줄인다.
- 0600 Unix socket, command schema 및 비밀정보 전달 경계를 유지한다.

### 외부 도구 출력 및 런타임 로그

- LIVE recorder와 SOOP/CHZZK/KICK VOD의 streaming line reader를 공유 bounded reader로 통일한다.
- 한 줄의 wire 크기는 32KiB로 제한한다. 초과 줄은 newline/EOF까지 비우고 `[OUTPUT:WARN] oversized tool output line omitted`로 대체한다. 부분 credential이나 잘린 진행률을 새 로그로 노출하지 않는다.
- 읽기 future가 취소되어도 partial line과 overflow 상태를 유지한다. 특히 SOOP의 진행률/취소 select에서 다음 read가 이어진다.
- 정상 UTF-8/CRLF/EOF 동작과 provider별 redaction, retry, progress parsing은 유지한다.
- shared LogBuffer는 기존 최대 400줄에 더해 한 줄 8KiB, 전체 문자열 내용 1MiB를 제한한다. 초과 시 가장 오래된 로그부터 제거한다.
- 큰 String allocation을 짧게 잘라 보관하는 경우 원래의 큰 capacity까지 붙잡지 않도록 처리한다. allocator retention이나 renderer/GPU 메모리 전체에 대한 상한을 의미하지는 않는다.

## 변경하지 않는 범위

renderer/FemtoVG, allocator, memory trim, UI model/backpressure 정책, provider metadata/body 정책, DB schema, release version, Windows process cleanup의 fail-closed 대기 정책은 유지한다. process-name 기반 종료나 Slint의 direct SQLite/child-process/localhost API를 추가하지 않는다.

## 검증

- core clone을 포함한 background start 100회, concurrent shutdown, shutdown 후 재시작 거부, Drop capture 해제 및 취소된 shutdown의 후속 join.
- 4MiB newline 없는 control 입력의 조기 거부, exact wire limit, UTF-8/EOF, read/write stall의 timeout 및 shutdown.
- Unix integration: 미완성 socket client를 열어 둔 채 SIGTERM → 정상 종료·socket 제거·후속 status 확인. Linux/macOS CI에서 실행한다.
- 4MiB tool output 줄을 drain한 뒤 다음 progress/한국어 줄 처리, cancellation 중 partial/overflow 상태 보존, UTF-8 오류 및 경계 길이.
- LogBuffer의 count/byte budget, 정상 tail/알림, UTF-8 truncation 및 overallocated String 해제.
- 기존 provider/process/backup tests, fmt, cargo check, clippy 및 cross-platform CI를 유지한다.

로컬 Linux에서는 Unix socket bind와 PID namespace의 `/proc/<pid>/exe` 접근이 제한된다. Phase 27.1에서 확인한 tool discovery 테스트도 `/usr/bin/ffmpeg` 존재에 영향을 받는다. 이 테스트/guard를 삭제·완화하지 않고 CI의 실제 OS 검증과 구분한다.

## 남아 있는 수동 검증

- [ ] Windows에서 동일 binary/renderer/DPI로 8–24시간 idle 측정
- [ ] 실제 LIVE 녹화·중지·새로고침과 VOD 분석·완료·실패·취소·retry 반복
- [ ] 정상 tool output에서 Queue progress 및 인증 갱신이 유지되는지 확인
- [ ] dialog/navigation/tray/notification 반복 중 Private Bytes·Working Set·handles·threads·GDI/USER 비교
- [ ] 작업 완료 후 2분/5분 baseline과 heap allocation stack 비교
- [ ] 정상 종료 후 해당 앱이 소유한 child process 잔존 여부

인증정보 없는 fixture test를 실제 provider 성공으로 표시하지 않는다. 수동 검증 결과를 확보하기 전에는 장시간 memory leak이 없다고 보장하지 않는다.
