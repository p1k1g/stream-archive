# Phase 25 — Windows UI/UX 정리

상태: 구현 PR 검증 중. Windows 실제 환경의 최종 수동 QA는 미수행입니다.

## 구현 범위

기존 Slint presentation과 `StreamArchiveCore`의 데이터·action 경계를 유지하면서 화면 구성을 정리합니다. 제품 버전은 `1.0.0`이며 DB migration이나 provider 기능 추가는 없습니다.

- charcoal surface, mint primary action, 공통 색상·버튼·상태 pill, 동일한 line icon 체계를 사용합니다.
- sidebar에는 LIVE / Channels, VOD / Queue / History, Settings / Diagnostics를 배치합니다. 로고는 모든 화면에서 canonical `rust-gui/assets/stream-archive-icon.png`를 사용합니다. 별도의 mockup 로고로 제품 아이콘을 교체하지 않습니다.
- LIVE는 방송 감시 상태와 현재 녹화 목록을 우선 표시합니다. **방송 감시 시작 / 방송 감시 중지**를 하나의 상태별 버튼으로 통합하며, 처리 중에는 비활성화합니다. 개별 **현재 방송 녹화 중지**는 기존 action으로 구분합니다.
- 저장 공간은 LIVE 하단에 볼륨별로 표시합니다. 기존 `StorageDisplayRow`의 volume / roles / paths / capacity / used / status / detail을 사용하고, 여러 볼륨은 제한된 높이의 목록에서 스크롤합니다. 동일 볼륨 집계는 기존 storage service를 따릅니다.
- 채널 이미지는 provider badge로 표현합니다. 채널 프로필 이미지를 조회하거나 저장하지 않습니다.
- Channels에서는 플랫폼·방송 감시 사용 여부·이름·ID·저장 폴더를 편집합니다. 기본 창 크기에서 고정 폭 입력들이 잘리지 않도록 두 줄로 배치합니다. 추가·이름 조회·삭제·저장·다시 불러오기 동작을 유지합니다.
- VOD는 URL 분석 → 실제 metadata → 화질 / PART / 저장 경로 / 병합 → 다운로드 또는 Queue 추가 순서입니다. 실제 단일 다운로드 상태·취소·새로고침을 유지합니다.
- Queue는 제목·provider·상태·progress·PART·시도 횟수·기존 cancel/retry/remove 조건을 compact row로 표시합니다. URL·경로·시각은 상세에서 확인하며 오류 메시지는 항상 보입니다. 상세 펼침은 작업 ID를 기준으로 유지해 polling snapshot이 목록을 교체해도 닫히지 않습니다. 작업은 기존 자동 실행 방식입니다.
- History의 ALL / LIVE / VOD, 검색·상태·날짜·달력·조회 건수·읽기 전용 기록을 유지합니다. VOD 분석 기록도 조회 범위에 남습니다.
- Settings는 기존 runtime/provider 설정과 Backup/Restore를 담당합니다. 닫기 동작·다운로드 결과 알림의 저장 후 적용 원칙과 secret 입력칸의 비워두기 동작을 유지합니다.
- Diagnostics는 기존 읽기 전용 preflight와 런타임 로그를 표시합니다. Worker 네트워크 성공이나 실제 credential 검증을 새로 주장하지 않습니다. 로그 auto-refresh는 이 화면을 보고 있을 때만 기존 bounded 주기로 실행합니다.

## 유지한 제한사항

속도·ETA·출력 크기 등 Queue model에 없는 값을 만들어 표시하지 않습니다. scheduler, activity feed, 새 플랫폼, history 삭제/재생/내보내기, Queue 수동 시작·일시정지·재정렬은 추가하지 않습니다.

Slint의 localhost HTTP / direct SQLite / child process 제어는 추가하지 않습니다. renderer, Windows icon resource embedding, owned-process lifecycle, native secret protection, package contract와 외부 도구 비번들 정책을 유지합니다. Backup은 설정·채널·Queue·History 등 DB를 대상으로 하며 미디어 파일을 포함하지 않습니다.

## 검증

Slint compile, 기존 Rust fmt/unit/check/clippy, RuntimeContracts 및 Windows/Linux/macOS package 검증은 [PR #112](https://github.com/p1k1g/stream-archive/pull/112)의 최신 CI 결과로 확인합니다. `ui_smoke_tests`는 기존 software renderer의 headless window에서 감시 버튼 분기·busy 차단·Space 입력·Diagnostics/Logs/Backup navigation, Queue 목록 갱신 후 상세 유지와 최소 창 크기의 7개 화면 렌더링을 검증합니다. 로컬 GUI 테스트는 41개 통과했습니다. Slint 1.18 interpreter의 예시 데이터 화면과 입력 smoke도 Linux 가상 디스플레이에서 확인했습니다. 이 검증은 실제 Windows shell·DPI·서비스 QA를 대신하지 않습니다. 시각 검증용 예시 데이터는 제품 코드에 포함하지 않으며 실제 provider 성공을 뜻하지 않습니다. 아래 항목은 Windows에서 따로 확인해야 합니다.

## Windows 최종 수동 QA — 미수행

- [ ] 기본 1120×720 및 최소 1000×650 창에서 모든 화면 표시
- [ ] 100% / 125% / 150% DPI, 긴 제목·ID·경로·오류 메시지 확인
- [ ] Tab / Shift+Tab / Enter / Space, focus 표시 및 disabled action 확인
- [ ] LIVE 감시 시작·중지, 개별 녹화 중지·재개·다시 확인·보호된 방송 입력
- [ ] 서로 다른 두 개 이상 볼륨과 같은 볼륨의 복수 저장 경로 표시
- [ ] 저장 공간 경고·오류 및 다수 볼륨 스크롤
- [ ] Channels 추가·수정·삭제·이름 조회·저장·다시 불러오기
- [ ] SOOP LIVE / VOD, CHZZK LIVE / VOD 실제 서비스 smoke
- [ ] VOD PART·화질·병합·경로·직접 다운로드·취소·Queue 추가
- [ ] Queue 자동 실행·진행률·실패 메시지·상세·취소·재시도·삭제
- [ ] History 검색·상태·날짜 입력·달력·ALL / LIVE / VOD·분석 기록
- [ ] 설정 변경·저장 전후·재실행 유지, secret 미노출 및 빈 입력 유지
- [ ] Backup 생성·무결성·복원 확인·취소·유휴 조건·pre_restore 안전 백업
- [ ] Diagnostics의 필수 도구 누락·경고·로컬 읽기 전용 결과
- [ ] 로그 수동·자동 새로고침, 화면 이동 시 polling 제한
- [ ] 닫기 → 트레이, 복원·종료 확인, 다운로드 완료·실패 알림 클릭
- [ ] Explorer / title bar / taskbar / Task Manager 아이콘 및 pin / unpin / re-pin
- [ ] minimize / restore / resize / repaint 및 장시간 memory / handles / threads 안정화

이 문서와 자동 검증은 수동 QA 또는 공개 릴리스 승인을 대신하지 않습니다. PR merge, tag 및 GitHub Release publication은 사용자가 결정합니다.
