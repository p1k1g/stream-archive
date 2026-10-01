# Phase 24.2 — Windows 다운로드 결과 알림

상태: 구현 PR 검증 중. 아래 Windows 수동 QA는 미수행입니다.

## 사용 방법

`설정 → 일반 → 런타임 환경 → 다운로드 완료·실패 알림`에서 **알림 사용** 또는 **알림 끄기**를 선택하고 **변경사항 저장**을 누릅니다. 기본값은 알림 사용이며 저장하지 않은 편집값은 적용하지 않습니다.

Windows Native GUI를 실행한 동안 SOOP / CHZZK VOD 다운로드의 완료·실패 결과를 Windows notification area 알림으로 표시합니다. 창을 트레이로 숨겨도 동작합니다. 분석 완료, 사용자 취소, LIVE 녹화 종료는 이번 알림 범위에 포함하지 않습니다. 즉시 거부된 직접 다운로드 요청은 기존 UI 오류 안내를 사용합니다. Queue에서 작업을 시작하지 못한 실패는 알림 대상입니다.

알림을 클릭하면 기존 창을 복원·활성화하고 검색·기간 필터를 초기화한 VOD History를 새로 조회합니다. Queue 시작 실패처럼 VOD job이 만들어지지 않은 결과는 기존 정책대로 Queue에서 확인합니다. 파일이나 외부 프로그램을 자동으로 실행하지 않습니다.

## 알림 정책

- 새 실행 세션의 runtime 결과만 구독합니다. 재실행 또는 Backup / Restore로 불러온 과거 History를 다시 알리지 않습니다. Restore 성공 시 event epoch를 무효화하므로 복원 전 job이 뒤늦게 완료 이벤트를 발행해도 표시하지 않습니다.
- 같은 작업은 한 번만 처리합니다. Queue 재시도는 새로운 시도로 취급합니다.
- 짧은 시간에 완료된 결과를 묶어 완료·실패 건수를 표시합니다. 개별 알림과 별도의 Queue 전체 완료 알림을 중복 생성하지 않습니다.
- 최소 2초 동안 모으고, 전송 간격은 최소 10초입니다. 이전 알림이 닫히기 전에는 다음 결과를 모읍니다. Windows에서 닫힘 응답이 없으면 45초 후 다음 묶음을 시도합니다.
- 알림 설정 저장 및 Backup / Restore 시 이전 epoch의 event buffer를 구독 경계에서 걸러 냅니다. 전환 후 완료된 현재 epoch 결과는 버리지 않습니다. 설정을 끄면 표시 대기 결과와 현재 앱 알림도 정리합니다. 꺼져 있는 동안 끝난 결과는 다시 켜도 재생하지 않습니다. 설정 전환 시 이미 idle인 job의 epoch를 무효화하고 진행 중인 job은 새 epoch로 갱신하므로, 전환 후 완료하는 작업은 새 설정을 따릅니다. 실제 종료 중에는 새 알림을 보내지 않습니다.
- 알림 본문에는 완료·실패 건수만 포함합니다. URL, 인증정보, 파일 경로, 영상 제목, 오류 원문을 노출하지 않습니다.

## Windows 및 portable 제한

기존 트레이의 `Shell_NotifyIconW` / `NIF_INFO`를 사용합니다. 이는 tooltip과 별개의 Windows native notification area 알림이며, 관리자 권한·Start Menu shortcut·COM activator·설치 프로그램을 추가하지 않습니다. 기존 portable 패키지 contract를 유지합니다.

**알림 센터의 지속 보관이나 앱 종료 후 알림 클릭은 보장하지 않습니다.** Windows 알림 설정, 방해 금지, 조직 정책 또는 Shell 상태에 따라 표시되지 않을 수 있습니다. `NIIF_RESPECT_QUIET_TIME`을 사용하며 사용자의 Windows 알림 제한을 우회하지 않습니다. 지속 보관 및 종료 후 재실행을 지원하는 등록형 toast는 별도 범위입니다.

트레이 등록 또는 알림 전송에 실패해도 다운로드·History·Queue 처리를 변경하지 않습니다. modal 오류창을 띄우지 않으며 설정의 알림 항목에 전송 실패 상태를 표시합니다. Explorer 재시작 시 기존 24.1 아이콘 복구 경로를 사용하고 이미 전송한 알림을 재생하지 않습니다.

## 구현 및 자동 검증

`StreamArchiveCore`에서 bounded session event stream을 제공하고, provider가 최종 다운로드 결과를 발행합니다. Windows presentation은 이 결과를 표시할 뿐 SQLite나 child process를 직접 제어하지 않습니다. event buffer는 128개, UI 중복 추적은 최근 256개 작업으로 제한합니다. UI가 오래 응답하지 않아 buffer가 넘치면 오래된 알림을 건너뛰고 다운로드 자체는 계속 진행합니다.

설정 `STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS=true/false`는 canonical SQLite에 저장되며 기존 Backup / Restore 경계를 따릅니다. Linux/macOS CLI/headless에는 native 알림을 추가하지 않습니다. renderer, icon embedding, owned-process 정리, 제품 버전 `1.0.0` 및 공식 패키지 구성을 유지합니다.

자동 검증 항목은 결과 이벤트의 성공·실패/분석·취소 구분, 과거 결과 replay 방지, bounded buffer, Queue 시작 실패·재시도, 설정 검증·재실행 유지, UI 중복 방지·비활성화·묶음·전송 간격입니다. 기존 fmt/unit/check/clippy/Slint/RuntimeContracts 및 Windows/Linux/macOS 패키지 검증도 유지합니다. 실제 통과 여부는 최신 PR CI 결과를 기준으로 확인합니다.

## Windows 수동 QA — 미수행

- [ ] 신규·기존 데이터에서 기본 알림 사용 확인
- [ ] 설정 저장·재실행·Backup / Restore 후 선택 유지
- [ ] 저장 전 편집값이 적용되지 않는지 확인
- [ ] SOOP VOD 실제 완료·실패 알림
- [ ] CHZZK VOD 실제 완료·실패 알림
- [ ] 분석 완료·취소 시 알림 없음
- [ ] 직접 요청의 즉시 오류와 Queue 시작 실패 안내 확인
- [ ] Queue 연속 완료·실패 묶음 및 재시도 알림 확인
- [ ] 같은 결과 중복 및 과거 History 재알림 없음
- [ ] 알림 끄기·재활성화 후 대기 결과 재생 없음
- [ ] 표시 중 알림 끄기 및 종료 중 알림 억제 확인
- [ ] 트레이 숨김·최소화 상태에서 알림 클릭 → 기존 창 복원·VOD History 조회
- [ ] Queue 시작 실패는 Queue에서 상세 확인 가능
- [ ] Windows 알림 끄기·방해 금지·조직 정책 환경에서 작업 계속 진행
- [ ] Explorer 재시작·트레이 등록 실패·알림 전송 실패 시 다운로드 영향 없음
- [ ] 한국어 문구·100% / 125% / 150% DPI 확인
- [ ] 장시간 Queue 처리 후 private memory / handles / threads 안정화
- [ ] 기존 24.1 트레이 닫기·복원·종료 취소·owned-process 정리 회귀 없음

credential이 필요한 실제 서비스 항목은 적절한 환경에서 수동 확인해야 합니다. 자동 검증 성공을 실제 서비스 QA 완료로 표시하지 않습니다.
