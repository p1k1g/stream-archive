# Phase 27.3 — Windows tray 복구 및 창 표시 보완

상태: PR 검증 중. merge·tag·Release 변경은 수행하지 않음.

## 관찰한 증상과 확인 범위

Phase 26 완료 빌드를 약 5일 실행한 뒤 흰 화면이 관찰됐다. tray에서 열었을 때 `Windows 트레이를 복구하지 못해 창을 다시 표시했습니다.` 팝업이 나타났고 확인 후 UI가 표시됐다. 동일 PID 37176의 Private Bytes는 복구 전 1,658.5MiB, 복구 후 1,640.7MiB였다. 이 측정만으로 leak이나 renderer 결함을 확정하지 않는다.

기존 자동 복구 경로는 Slint timer callback에서 숨겨진 창을 표시한 직후 동기 `MessageBoxW`를 실행했다. callback이 반환되기 전에 사용자의 확인을 기다리는 구조여서 Slint의 정상 화면 갱신을 지연시킬 수 있다. 사용자의 흰 화면이 반드시 이 경로 하나로 발생했다는 재현 결과는 아직 없다.

## 수정 범위

- tray 등록 실패가 연속 3회 발생한 경우에만 숨겨진 창을 복원한다. 정상 2초 poll 기준 약 4초 이상 재시도하며, `TaskbarCreated`도 재시도를 유발한다.
- 등록 성공 또는 이미 표시된 창은 실패 횟수를 초기화한다. exit pending 상태에서는 자동으로 창을 복원하지 않는다.
- 자동 복구 안내를 비차단 Slint banner로 표시한다. 확인 버튼은 banner만 닫으며 UI navigation과 녹화·다운로드를 막지 않는다.
- tray 열기·History 이동·복구 시 기존 show/focus에 이어 redraw를 요청한다. renderer 교체나 Win32 repaint hack은 사용하지 않는다.
- 자동 복구 경로는 modal `MessageBoxW`를 호출하지 않는다. 사용자가 요청한 종료 확인 및 기존 최초 tray 사용 안내의 동작은 유지한다.
- 반복 실패에 따른 창 복원은 stderr에 `[DESKTOP:WARN]`을 남긴다. GUI release에서 stderr가 수집되지 않을 수 있어 영구 runtime log 기록을 보장하지 않는다.

## 자동 검증

- 일시적인 tray 실패 뒤 성공하면 fallback하지 않고 실패 횟수가 초기화되는지 확인
- 연속 실패 시 hidden window만 fallback 대상으로 삼는지 확인
- 복구 banner를 표시한 채 navigation이 동작하고 확인 버튼이 접근 가능한지 headless software UI test로 확인
- Windows Slint compile, 기존 UI/adapter tests, clippy, RuntimeContracts 및 release package CI 유지

## Windows 수동 검증

- [ ] tray 이동·열기를 반복하고 첫 화면이 확인 대기 없이 그려지는지 확인
- [ ] Explorer 재시작 후 tray 재등록 또는 접근 가능한 창 복원 확인
- [ ] tray 등록이 잠시 실패했다 회복되는 경우 불필요하게 창이 열리지 않는지 확인
- [ ] 지속 실패 fallback banner가 표시돼도 화면 전환·최소화/복원·크기 변경이 가능한지 확인
- [ ] 실제 녹화/다운로드가 복원 전후 유지되는지 확인
- [ ] 종료 확인·notification·History 이동 regression 확인
- [ ] 동일 binary/renderer/PC로 여러 날 실행하며 Private Bytes·Working Set·handles·threads 추세와 흰 화면 재발 여부 확인

Windows desktop/Explorer 및 실제 provider 검증은 CI의 headless UI test와 구분한다. Phase 27.2 task lifecycle 수정과 이번 tray 수정으로 장시간 메모리 증가 전체가 해결됐다고 주장하지 않는다.
