# Stream Archive 1.0.1 공개 체크리스트

상태: 버전 전환 PR 단계. 1.0.1 tag·Release·공개 패키지는 아직 없음.

## 버전 전환 및 자동 검증

- [ ] 버전 전환 PR의 core-check (windows/linux/macos), windows-check 전체 성공 확인
- [ ] Codex review 결과 확인 및 실제 지적사항 처리
- [ ] 운영자가 PR merge 후 공개 대상 main commit SHA 확정
- [ ] `Stream Archive release artifacts`를 해당 merge commit에서 수동 실행
- [ ] release-contracts, windows-package, Linux/macOS package job 성공 및 동일 commit 확인
- [ ] Windows EXE ProductVersion, RELEASE_INFO.txt, Unix CLI가 모두 1.0.1인지 확인
- [ ] package-local 및 archive-level checksum, fresh extract smoke, corrupted archive 거부 확인
- [ ] credentials/cookies/logs/DB/media/process-state 및 외부 미디어 도구 binary가 공식 artifact에 없는지 확인

PR CI의 패키지 smoke는 merge 후 공개할 artifact 빌드를 대신하지 않습니다. workflow는 기존 read-only·수동 실행 방식을 유지하며 tag·Release를 자동 publish하지 않습니다.

## Windows 수동 검증

- [ ] 신규 압축 해제 후 실행 및 재실행
- [ ] tray 이동·복원 반복, 복원 직후 첫 화면 표시
- [ ] Explorer 재시작 후 tray 재등록 또는 창 fallback
- [ ] 복구 banner가 navigation·최소화/복원·resize를 막지 않는지 확인
- [ ] 종료 확인 및 다운로드 notification·History 이동
- [ ] 정상 종료 후 owned child process 잔존 여부
- [ ] 같은 PC/renderer에서 장시간 실행하며 Working Set·Private Bytes·handles·threads 추세 기록

## 실제 서비스·데이터·비밀정보 저장

- [ ] SOOP LIVE / VOD 시작·완료·취소·재시도
- [ ] CHZZK LIVE / VOD 시작·완료·취소·재시도
- [ ] KICK 공개 LIVE 및 공개·구독 VOD
- [ ] SOOP VOD 인증 갱신과 이어받기
- [ ] Queue / History와 Backup / Restore
- [ ] 대표적인 1.0.0 데이터로 upgrade 및 재실행 후 설정 유지
- [ ] rollback 절차 확인
- [ ] Windows DPAPI / Linux Secret Service / macOS Keychain 저장·조회

인증정보 없는 fixture test를 실제 provider 성공으로 기록하지 않습니다. 미완료 항목은 미완료로 남기며 공개 시 알려진 제한과 검증 범위를 함께 안내합니다.

## 공개 및 다운로드 연결

- [ ] 공개할 archive와 `.sha256`을 release artifact run에서 확보하고 hash 재검증
- [ ] 기존 v1.0.0 tag·Release·첨부파일·보존 checksum 유지
- [ ] 확정한 1.0.1 commit에 v1.0.1 tag 생성
- [ ] 새 GitHub Release에 플랫폼별 archive 및 대응 `.sha256`을 정식 Release assets로 첨부
- [ ] 한국어 1.0.1 릴리스 노트와 미완료 수동 검증 범위를 반영하여 공개
- [ ] 로그아웃 상태에서 Release 및 모든 다운로드 링크 접근 확인
- [ ] 실제 공개된 asset의 checksum을 `docs/releases/v1.0.1/`에 보존
- [ ] 공개 확인 후 README의 Release·다운로드·checksum 링크와 준비 상태 갱신

다운로드는 실제 assets가 업로드된 뒤 `https://github.com/p1k1g/stream-archive/releases/download/v1.0.1/<파일명>`을 사용합니다. 아직 없는 주소나 1.0.0 checksum을 1.0.1 다운로드로 연결하지 않습니다.
