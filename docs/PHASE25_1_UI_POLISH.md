# Phase 25.1 — UI 사용 편의 및 아이콘 정리

상태: 구현 PR 검증 중. 실제 Windows 수동 QA는 미수행입니다.

- LIVE 저장 공간에서 SQLite 전용 볼륨과 SQLite 역할·DB 크기·긴 경로의 중복 표기를 숨깁니다. 녹화용 볼륨의 용량·사용률·상태와 오류는 유지합니다. 사용량 아래의 채널명·사용 목적 줄도 표시하지 않습니다. core storage snapshot과 Diagnostics는 변경하지 않습니다.
- sidebar와 화면 제목은 LIVE / 채널 / VOD 다운로드 / 대기열 / 기록 / 설정 / 진단으로 표시합니다. 내부 navigation ID는 기존 값을 유지합니다.
- Channels의 플랫폼 버튼을 SOOP / CHZZK 선택 드롭다운으로 바꿉니다. 기존 저장·busy 조건과 채널 데이터는 유지합니다.
- VOD 저장 경로는 저장된 기본 설정 `OUTPUT_DIR`을 적용합니다. 직접 입력하거나 찾아보기로 선택한 경로는 현재 앱 세션의 VOD draft에만 적용하며 설정을 변경하지 않습니다. 개별 지정 후 설정을 다시 불러와도 덮어쓰지 않습니다. 기본 설정이 비어 있으면 저장 경로를 직접 선택해야 합니다.
- charcoal / mint 색상의 archive + play 아이콘을 사용합니다. 편집 원본은 `rust-gui/assets/stream-archive-icon.svg`, canonical build 입력은 동일한 이름의 PNG입니다. PNG는 원본 SVG를 1024×1024로 렌더링합니다. build.rs의 16/24/32/48/64/128/256 ICO frame 및 winresource embedding은 그대로 유지합니다.

## Windows 수동 QA — 미수행

- [ ] DB 전용 드라이브가 LIVE 저장 공간에 나오지 않는지 확인
- [ ] 동일 볼륨의 LIVE/DB 경로와 여러 녹화용 볼륨 표시 확인
- [ ] 저장 공간 부족·오류 표시 및 다수 볼륨 스크롤 확인
- [ ] 플랫폼 메뉴 열기·취소·SOOP/CHZZK 선택·채널 저장·재실행 확인
- [ ] 저장된 기본 경로로 VOD 화면 초기화 확인
- [ ] 직접 입력·폴더 선택·선택 취소·URL 변경·설정 새로고침 시 개별 경로 유지 확인
- [ ] 직접 다운로드·Queue 추가의 출력 경로 확인
- [ ] sidebar / title bar / Explorer / taskbar / Task Manager / tray의 새 아이콘 확인
- [ ] 100% / 125% / 150% DPI에서 드롭다운과 경로 입력 확인

장시간 메모리 증가 보고는 별도의 원인 조사 대상이며 이 작업에서 leak을 해결했다고 주장하지 않습니다. renderer 변경이나 강제 memory trim은 추가하지 않습니다. merge/tag/Release publication은 사용자가 결정합니다.
