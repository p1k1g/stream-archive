# Phase 24.1 — Windows 닫기 동작 및 시스템 트레이

상태: 구현 PR 검증 중. 실제 Windows 수동 QA는 아래 목록에 남겨 둡니다.

## 사용 방법

`설정 → 일반 → 런타임 환경 → 창을 닫을 때`에서 동작을 선택하고 **변경사항 저장**을 누릅니다.

- **프로그램 종료**: 기본값입니다. 작업 상태를 확인한 뒤 종료합니다.
- **시스템 트레이로 이동**: 닫기 버튼과 `Alt+F4`는 창만 숨깁니다. 녹화·다운로드·채널 감시는 계속 실행됩니다. 최초 이동 시 안내를 표시합니다.

트레이 아이콘을 클릭하거나 메뉴에서 **Stream Archive 열기**를 선택하면 기존 창을 복원하고 활성화합니다. 메뉴와 tooltip에는 녹화·다운로드·대기 수 및 감시 상태를 표시합니다. 상태는 마지막 runtime 응답이며, 긴 작업 중에는 갱신이 늦을 수 있습니다.

트레이 메뉴의 **종료**는 설정과 관계없이 실제 앱 종료를 요청합니다. 녹화·다운로드·Watcher 또는 대기 중인 Queue가 있으면 확인창을 표시하며, 기본 선택은 취소입니다. 확인하면 앱 소유 작업을 정리한 뒤 종료합니다. 중단된 다운로드를 항상 이어받을 수 있다는 보장은 없습니다. 대기열·복구 동작은 기존 Queue 정책을 따릅니다.

## 설정 및 수명주기

`STREAM_ARCHIVE_CLOSE_ACTION`은 canonical SQLite에 `EXIT` 또는 `TRAY`로 저장합니다. 신규·기존 데이터의 기본값은 `EXIT`이며 Backup / Restore의 기존 설정 저장 경계를 사용합니다. 저장 전 편집값은 닫기 동작에 반영하지 않습니다. Linux/macOS CLI/headless 실행 방식은 바뀌지 않습니다.

트레이는 Windows presentation 모듈입니다. 상태 확인과 종료는 controller를 통해 `StreamArchiveCore`를 사용합니다. 트레이 이동은 core shutdown을 호출하지 않습니다. 실제 종료에서는 `core.shutdown()` 완료를 기다리고 native-runtime thread를 join합니다. 종료 확정 후 대기 중인 UI 요청은 새 작업을 시작하지 않습니다.

트레이 등록에 실패하면 창을 숨기지 않습니다. 초기 생성 실패 시 앱을 다시 실행하거나 설정을 **프로그램 종료**로 바꾸어 사용합니다. Explorer 재시작 시 아이콘 재등록을 시도하며, 복구가 실패하면 숨겨진 창을 다시 표시합니다. notification area의 숨겨진 아이콘 영역에 들어가는 것은 정상이며 사용자가 표시 위치를 선택할 수 있습니다.

트레이 아이콘은 Phase 23.11의 기존 EXE icon resource를 사용합니다. renderer, executable icon embedding, 외부 도구 번들 및 owned-process 정책은 변경하지 않습니다. Windows 자동 시작·알림·Kick 지원은 후속 범위입니다. 제품 버전 전환은 이번 PR에 포함하지 않습니다.

## 자동 검증

- Rust fmt / unit tests / cargo check / clippy `-D warnings`
- close action 허용값 및 SQLite 재실행 유지 테스트
- 종료 확정 후 대기 중인 요청이 실행되지 않는 worker 테스트
- 기존 Windows Slint compile, RuntimeContracts 및 패키지·백업·archive 검증
- 기존 Linux/macOS core 및 CLI/headless 패키지 검증

실행 결과는 PR의 최신 CI와 최종 보고를 기준으로 확인합니다. 자동 테스트 통과를 실제 서비스 QA 완료로 간주하지 않습니다.

## Windows 수동 QA — 미수행

- [ ] 기존 데이터 / 신규 데이터에서 기본값 프로그램 종료 확인
- [ ] 설정 저장 / 재실행 / Backup / Restore 후 선택 유지
- [ ] 저장하지 않은 선택이 닫기 동작에 반영되지 않는지 확인
- [ ] 닫기 버튼 / `Alt+F4`로 트레이 이동 및 최초 안내 확인
- [ ] 아이콘 클릭 / 열기 메뉴로 기존 창 복원·활성화
- [ ] 최소화된 창 복원 및 반복 닫기·복원
- [ ] 실제 LIVE 녹화 / VOD 다운로드 / Queue / 채널 감시가 숨김 중 유지
- [ ] 작업 중 종료 취소 후 작업 유지
- [ ] 종료 확인 후 owned process 정리 및 runtime thread 종료
- [ ] 관련 없는 Streamlink / FFmpeg 프로세스 유지
- [ ] 트레이 등록 실패 시 창 유지
- [ ] Explorer 재시작 후 복구 / 복구 실패 시 창 복원
- [ ] tooltip / 메뉴 작업 상태 확인
- [ ] Explorer / 제목 표시줄 / 작업표시줄 / Task Manager 아이콘 회귀 없음
- [ ] 작업표시줄 고정 → 해제 → 재고정
- [ ] repaint / resize / 100%·125%·150% DPI
- [ ] 장시간 숨김 및 반복 복원 후 private memory / handles / threads 안정화

credential이 필요한 실제 서비스 항목은 적절한 환경에서 사용자가 검증합니다.

## Phase 25.2 확장

닫기 동작의 `매번 확인`(`ASK`)과 선택 기억 기능은 [Phase 25.2](PHASE25_2_UI_USABILITY.md)를 참고하세요. 기존 `EXIT` / `TRAY` 설정은 유지합니다.
