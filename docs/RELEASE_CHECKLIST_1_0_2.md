# Stream Archive 1.0.2 공개 체크리스트

상태: **버전 전환 및 공개 준비 중**. 미수행 수동 QA는 미완료로 유지합니다.

기능 변경 근거: [Phase 27.4 PR #132](https://github.com/p1k1g/stream-archive/pull/132) · [최종 기능 CI](https://github.com/p1k1g/stream-archive/actions/runs/39254608178). 아래 1.0.2 공개 검증은 별도로 확인합니다.

## 버전 및 자동 검증

- [ ] 1.0.2 버전 전환 PR의 core-check (windows/linux/macos), windows-check 전체 통과
- [ ] Codex review 확인 및 실제 지적사항 처리
- [ ] 공개 대상 main commit SHA 확정
- [ ] 해당 commit에서 `Stream Archive release artifacts` 수동 실행
- [ ] release-contracts, windows-package, Linux/macOS package job 성공 및 동일 commit 확인
- [ ] Windows EXE ProductVersion, RELEASE_INFO.txt, Unix CLI version = 1.0.2
- [ ] package-local 및 archive-level checksum, fresh extract smoke, corrupted archive 거부 검증
- [ ] credentials/cookies/logs/DB/media/process-state 및 외부 미디어 도구 binary 미포함 확인

PR package smoke는 merge 후 공개 artifact 빌드를 대신하지 않습니다. canonical workflow는 read-only·수동 실행으로 유지하며 Release/tag를 자동 publish하지 않습니다.

## Windows 및 장시간 수동 검증

- [ ] 신규 압축 해제·실행·재실행
- [ ] LIVE 표시 → 트레이 → 재표시 및 다른 탭 왕복
- [ ] 재표시 시 LIVE 상태·저장공간이 새 조회 결과를 표시하는지 확인
- [ ] 녹화 완료/조회 오류 후 이전 썸네일 제거 및 새로고침 후 재출현하지 않는지 확인
- [ ] 녹화 중 일시적인 조회 오류에서도 녹화 유지
- [ ] Explorer 재시작 후 tray 복구, 최소화/복원/resize 및 notification
- [ ] 정상 종료 후 owned child process 잔존 여부
- [ ] 같은 채널/renderer에서 기존 메모리 CSV와 opt-in trace를 함께 며칠간 기록
- [ ] 작업 완료 후 baseline과 Working Set·Private Bytes·handles·threads 및 필요 시 heap/GDI/USER 비교

## 실제 서비스·데이터·비밀정보 저장

- [ ] Worker 및 외부 도구 Diagnostics
- [ ] SOOP LIVE / VOD 시작·완료·취소·재시도
- [ ] CHZZK LIVE / VOD 시작·완료·취소·재시도
- [ ] KICK 공개 LIVE 및 공개·구독 VOD
- [ ] SOOP VOD 인증 갱신·이어받기
- [ ] Queue / History와 Backup / Restore
- [ ] 대표적인 1.0.1 데이터로 upgrade 및 재실행 후 설정 유지
- [ ] 이전 패키지·업그레이드 전 백업을 사용한 rollback 절차
- [ ] Windows DPAPI / Linux Secret Service / macOS Keychain 저장·조회

fixture test를 실제 provider 성공으로 기록하지 않습니다. 장시간 메모리 개선 효과를 아직 확정하지 않습니다.

## 공개 및 다운로드 연결

- [ ] 최종 artifact의 archive와 `.sha256` 확보·hash 재검증
- [ ] 기존 v1.0.0/v1.0.1 tag·Release·assets·checksum 보존
- [ ] 검증한 공개 commit에 v1.0.2 tag 생성
- [ ] 새 GitHub Release에 세 플랫폼 archive와 대응 `.sha256` 첨부
- [ ] 한국어 릴리스 노트와 미완료 수동 QA 범위를 반영하여 공개
- [ ] 로그인 없는 Release/다운로드 접근 및 공개 asset hash 확인
- [ ] 실제 공개된 checksum을 `docs/releases/v1.0.2/`에 보존
- [ ] 공개 확인 후 README의 최신 Release·다운로드·checksum 링크 갱신

아직 없는 주소나 이전 버전의 checksum을 1.0.2 다운로드로 연결하지 않습니다.
