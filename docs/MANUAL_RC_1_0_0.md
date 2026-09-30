# Stream Archive 1.0.0 최종 수동 검증 체크리스트

상태: **수동 검증 필요 — 아래 항목은 모두 미완료**

최종 공개 대상으로 선택한 1.0.0 압축 파일을 새 디렉터리에 풀어 검증합니다. 개발 checkout이나 다른 revision의 결과로 대신하지 않습니다. 자동 CI 성공은 실제 서비스 세션·GUI·native secret store의 수동 성공을 뜻하지 않습니다. 인증정보나 환경이 없으면 `미검증`으로 기록하며 PASS로 표시하지 않습니다.

이 문서는 [Phase 23.9 문서](PHASE23_9_FINAL_MANUAL_RC_RELEASE_PREP.md)의 5절 및 7–14절에 남은 gate를 유지합니다. 해당 조건을 면제하지 않으며 아래 한국어 세부 항목과 함께 충족해야 합니다. Windows의 과거 headless gate는 Phase 23.10 Native-only contract에 따라 적용하지 않지만 Linux/macOS CLI·headless 검증은 유지합니다.

## 검증 증빙 기록

각 항목에 다음 정보를 남깁니다. 인증정보, cookie, token 값은 문서·로그·PR 댓글에 기록하지 않습니다.

| 항목 | 기록할 내용 |
|---|---|
| 대상 파일 | 압축 파일명, SHA-256, commit, `version=1.0.0` |
| 환경 | OS·버전, 아키텍처, Windows DPI, 필요한 서비스·도구 버전 |
| 수행 정보 | 확인 날짜, 검증자, 실행 단계 |
| 결과 | 성공 / 실패 / 미검증, 실제 관찰 결과와 증빙 |
| 제한사항 | 사용할 수 없는 세션·인증정보·환경과 영향 |

## Windows 최종 수동 검증

`stream-archive-windows-x64.zip`에서 새로 압축 해제한 `StreamArchive.exe`로 확인합니다.

- [ ] Explorer 큰 아이콘 확인
- [ ] Explorer 작은 아이콘 확인
- [ ] 제목 표시줄 아이콘 확인
- [ ] 실행 중 작업표시줄 아이콘 확인
- [ ] 작업 관리자 아이콘 확인
- [ ] 작업표시줄 고정 → 해제 → 재고정 확인
- [ ] 화면 repaint 확인
- [ ] 최소화 / 복원 확인
- [ ] 창 크기 변경 확인
- [ ] 100% DPI 확인
- [ ] 가능한 경우 125% / 150% DPI 확인(환경이 없으면 미검증 사유 기록)

### 실행 및 경로

- [ ] `StreamArchive.exe` 실행
- [ ] 시작 중 crash 없음
- [ ] Native UI 정상 표시
- [ ] 예상하지 않은 console 창 없음
- [ ] 정상 종료
- [ ] 재실행 성공
- [ ] ASCII 경로 확인
- [ ] 공백이 있는 경로 확인
- [ ] 한글·Unicode 경로 확인

예시 경로: `C:\Stream Archive RC\`, `C:\테스트\Stream Archive\`.

### Settings

- [ ] 설정 불러오기
- [ ] 출력·다운로드 경로 변경
- [ ] 백업 경로 변경
- [ ] Streamlink 실행 파일 경로 직접 지정
- [ ] yt-dlp 실행 파일 경로 직접 지정
- [ ] FFmpeg 실행 파일 경로 직접 지정
- [ ] 설정 저장
- [ ] 재실행
- [ ] 저장한 설정 유지 확인

### Diagnostics

- [ ] 새로고침
- [ ] Streamlink 탐색·probe
- [ ] yt-dlp 탐색·probe
- [ ] FFmpeg 탐색·probe
- [ ] 잘못된 경로의 오류가 이해 가능한 내용으로 표시됨
- [ ] 도구가 없는 상태가 이해 가능한 내용으로 표시됨
- [ ] 올바른 경로로 복구 후 새로고침에 정상 반영됨

### Channels 및 서비스 설정

- [ ] Native UI에서 채널 목록 불러오기
- [ ] 채널 초안 추가
- [ ] UI에 노출된 채널 식별자·이름 편집
- [ ] 활성 상태 전환
- [ ] UI에서 제공하는 서비스 플랫폼 선택·전환
- [ ] Native 동작으로 채널 정보 resolve
- [ ] Channels 설정 저장
- [ ] 설정 재조회 후 저장한 채널 유지 확인
- [ ] Native Delete로 테스트 채널 삭제
- [ ] 저장·재조회 후 삭제한 채널은 없고 다른 채널은 유지됨
- [ ] Native UI에서 서비스 설정 저장
- [ ] 인증정보·환경이 허용하면 SOOP 인증 테스트 callback 확인(불가 시 미검증)

### LIVE Native 동작

SOOP LIVE와 CHZZK LIVE 각각에 대해 수행합니다.

- [ ] Native UI에서 LIVE 상태 새로고침
- [ ] Native UI에서 watcher·녹화 시작
- [ ] 상태·진행률 갱신 확인
- [ ] 중지 가능한 활성 항목에서 Stop 실행 후 상태 전환 확인
- [ ] 재개 가능한 항목에서 Resume 실행 후 상태·진행률 재개 확인
- [ ] 적용 가능한 항목에서 Recheck 실행 후 상태 갱신 확인
- [ ] Stop 후 소유한 프로세스 정리 및 UI 상태 복구 확인
- [ ] 정리 후 두 번째 Native LIVE 작업 시작 가능

Stop, Resume, Recheck를 **각각 적용 가능한 상태에서** 확인해야 이 gate를 통과한 것으로 기록할 수 있습니다. 한 동작만 확인한 결과는 충분하지 않습니다.

### VOD Native 동작

SOOP VOD와 CHZZK VOD 각각에 대해 수행합니다.

- [ ] Native UI에서 VOD URL 입력·편집
- [ ] Native 동작으로 VOD 분석
- [ ] metadata·화질·PART 상태 표시 확인
- [ ] 출력 디렉터리 선택·편집
- [ ] 화질 및 적용 가능한 PART·merge 옵션 선택
- [ ] Native UI에서 직접 다운로드 시작
- [ ] 상태·진행률 갱신 확인
- [ ] Native UI에서 취소 후 정리 확인
- [ ] 새 Native VOD 작업으로 재시도해 완료 확인

### Queue

- [ ] 분석한 VOD를 Native UI에서 Queue에 추가
- [ ] Queue 새로고침
- [ ] 대기·실행·완료 상태 정상 표시
- [ ] 취소 가능한 항목에서 Cancel 실행
- [ ] 재시작 없이 취소 상태 반영 및 소유한 프로세스 정리 완료
- [ ] 재시도 가능한 실패·취소 항목에서 Retry 실행
- [ ] 재시작 없이 예상 Queue 상태 생성·갱신
- [ ] 삭제 가능한 항목에서 Remove 실행
- [ ] 의도한 Queue 행만 삭제되고 다른 항목은 유지됨
- [ ] 재실행 후 보관된 Queue 상태 정상 표시

Cancel, Retry, Remove를 **각각 적용 가능한 상태에서** 확인해야 합니다. 한 동작만 확인한 결과는 충분하지 않습니다.

### History

- [ ] Native UI에서 History 열기
- [ ] History 새로고침
- [ ] 시작·종료 날짜 달력 열기·선택·초기화
- [ ] 가능한 경우 달력 월 이동
- [ ] 제공되는 필터·조회 제한 적용
- [ ] LIVE/VOD 기록이 선택한 필터와 일치함
- [ ] 필터 변경·초기화 후 화면 정상 갱신

### 설정의 관리 화면

- [ ] 설정 > 관리 화면 오류 없이 표시
- [ ] 백업 정책값 불러오기
- [ ] Native UI에서 백업 정책 변경·저장
- [ ] 재실행 후 백업 정책 유지
- [ ] Native Backup으로 관리형 백업 생성
- [ ] 새 백업이 Native 목록에 표시됨
- [ ] Native Restore에서 유효한 백업 선택
- [ ] 활성 런타임·안전하지 않은 복구 조건을 명확히 거부함
- [ ] 허용된 복구 후 Settings/Channels/History/Queue 상태 확인
- [ ] 복구된 Queue 항목·상태가 즉시 화면에 갱신됨
- [ ] 재실행 후 복구된 Settings/Channels/History/Queue 유지
- [ ] Native UI에서 Logs 화면·callback 불러오기
- [ ] 로그 새로고침
- [ ] 제공되는 로그 행·상세 조작 확인
- [ ] 로그가 없거나 비어 있어도 crash 없음

Windows Native 항목은 새로 압축 해제한 GUI에서 수행해야 합니다. CLI/headless 및 오프라인 스크립트 성공은 보조 증빙이며 Slint callback·화면 검증을 대신하지 않습니다. 아래 백업·복구 무결성 검증도 별도로 필요합니다.

## 실제 서비스 검증

| 서비스 | 상태 |
|---|---|
| SOOP LIVE | 수동 검증 필요 |
| SOOP VOD | 수동 검증 필요 |
| CHZZK LIVE | 수동 검증 필요 |
| CHZZK VOD | 수동 검증 필요 |

Windows의 각 서비스 PASS에는 위 Native 동작 검증이 필요합니다. Linux/macOS는 해당 CLI·headless 실행 경로로 확인합니다.

### LIVE — SOOP / CHZZK 각각 확인

- [ ] 서비스·채널 탐색
- [ ] 방송 중 여부·상태 조회
- [ ] 녹화 시작
- [ ] 출력 파일 생성
- [ ] 녹화 진행 확인
- [ ] 취소
- [ ] 소유한 프로세스 정리
- [ ] 두 번째 녹화 시작 가능
- [ ] 정상 종료·완료
- [ ] History 기록 반영
- [ ] 재실행 후 기록 유지

### VOD — SOOP / CHZZK 각각 확인

- [ ] URL·식별자 탐색
- [ ] metadata·제목 확인
- [ ] 출력 디렉터리 확인
- [ ] 실제 외부 도구 실행
- [ ] 진행 상태 확인
- [ ] 취소
- [ ] 프로세스 정리
- [ ] 재시도
- [ ] 완료
- [ ] 출력 파일 확인
- [ ] Queue 반영
- [ ] History 반영
- [ ] 재실행 후 상태 유지

## 외부 미디어 도구

Streamlink, yt-dlp, FFmpeg 각각에 대해 확인합니다. 공식 패키지에는 번들하지 않습니다.

| 도구 | 버전 | 탐색 경로 출처 | 최종 경로 | probe | 실제 서비스 사용 |
|---|---|---|---|---|---|
| Streamlink | 미확인 | 미확인 | 미확인 | 미검증 | 미검증 |
| yt-dlp | 미확인 | 미확인 | 미확인 | 미검증 | 미검증 |
| FFmpeg | 미확인 | 미확인 | 미확인 | 미검증 | 미검증 |

- [ ] `PATH`에서 탐색
- [ ] 직접 지정한 경로 사용
- [ ] 잘못된 경로 처리
- [ ] 도구가 없는 경우 처리
- [ ] 버전 probe
- [ ] 공백이 있는 경로
- [ ] Unicode 경로
- [ ] 해당하는 경우 timeout·취소 처리

## 데이터 및 업그레이드

### 신규 설치

DB·설정·백업·로그가 없는 환경에서 시작합니다.

- [ ] 최초 실행
- [ ] 디렉터리 초기화
- [ ] SQLite DB 생성
- [ ] 기본값 사용 가능
- [ ] Diagnostics 사용 가능
- [ ] 서비스 설정 가능
- [ ] 미디어 도구 탐색
- [ ] 정상 종료
- [ ] 재실행
- [ ] 설정 유지
- [ ] repository 상대 경로에 대한 런타임 의존성 없음

### 기존 데이터로 업그레이드

서로 다른 버전 간 업그레이드의 대표 사례를 기록합니다. 이전 RC package는 보관합니다.

1. 기존 런타임을 종료합니다.
2. 백업을 만들고 무결성을 확인합니다.
3. 이전 패키지를 보관합니다.
4. 1.0.0을 별도 디렉터리에 압축 해제합니다.
5. 기존 기준 데이터 디렉터리를 연결합니다.
6. 새 버전을 실행합니다.
7. DB·Settings·Channels·History·Queue·Diagnostics 및 필요한 비밀정보 사용 상태를 확인합니다.
8. 정상 종료합니다.
9. 다시 실행합니다.
10. 데이터와 설정 유지 여부를 확인합니다.

- [ ] 기존 데이터를 사용한 업그레이드 확인
- [ ] 재실행 후 설정·데이터 유지 확인

예상하지 않은 데이터 초기화·삭제·강제 재초기화는 원인을 확인하고 해결할 때까지 공개 차단 사유입니다.

### rollback 절차

새 런타임을 종료한 뒤 이전 패키지를 복구합니다. 실제 DB 상태에서 필요할 때만 검증한 업그레이드 전 백업을 복구합니다.

- [ ] 새 버전 최초 실행 전, 이전 패키지가 그대로 사용 가능한지 확인
- [ ] 새 버전 실행 후, 실제 DB 상태에서 이전 패키지의 동작 확인
- [ ] 필요한 경우 검증한 업그레이드 전 백업 복구 절차 확인

**임의 schema downgrade 호환성은 보장하지 않습니다.** 검증한 조건을 넘어서는 rollback 성공을 주장하지 않습니다. 플랫폼별 절차는 [운영 가이드](OPERATIONS.md)를 따릅니다.

### Backup / Restore

- [ ] Backup 생성
- [ ] metadata 존재
- [ ] SHA-256 존재·무결성 확인
- [ ] 백업 목록 조회
- [ ] 대표적인 Queue 상태가 백업에 포함됨
- [ ] 백업 후 Settings/Channels/History/Queue 상태 변경
- [ ] Restore 수행
- [ ] Settings 복구
- [ ] Channels 복구
- [ ] History 복구
- [ ] Queue 행·상태가 복구 직후 반영됨
- [ ] 재실행 후 복구한 Settings/Channels/History/Queue 유지
- [ ] 유효하지 않은 백업 거부
- [ ] 손상된 백업 거부
- [ ] 활성 런타임 상태에서 복구 차단
- [ ] 공백이 있는 경로
- [ ] Unicode 경로

자동 백업·복구 검증 결과와 별도로 실제 사용 조건을 확인합니다.

## 비밀정보 저장

### Windows DPAPI

- [ ] 비밀정보 저장
- [ ] 재실행
- [ ] 재실행 후 애플리케이션에서 비밀정보 사용 가능
- [ ] SQLite·설정·로그에 평문 유출 없음

### Linux Secret Service

사용 가능한 세션이 없으면 미검증으로 기록합니다.

- [ ] `secret-tool` 사용 가능
- [ ] 사용할 수 있는 Secret Service 세션
- [ ] 비밀정보 저장·조회
- [ ] 세션을 사용할 수 없으면 안전하게 거부함

### macOS Keychain

사용 가능한 환경이 없으면 미검증으로 기록합니다.

- [ ] Keychain 저장·조회
- [ ] 재실행 후 유지
- [ ] 실패 경로에서 안전하게 거부함

## 공개할 정확한 artifact 점검

세 플랫폼 각각의 실제 공개 대상 파일로 수행합니다. 과거 CI가 통과한 다른 파일의 결과를 재사용하지 않습니다.

- [ ] 최종 압축 파일의 `.sha256` 확인
- [ ] 새 디렉터리에 압축 해제 후 패키지 내부 `SHA256SUMS.txt` 확인
- [ ] `RELEASE_INFO.txt`의 `version=1.0.0` 및 대상 commit 확인
- [ ] Windows EXE의 `ProductVersion=1.0.0` 확인
- [ ] 런타임 `data/`가 비어 있음
- [ ] SQLite DB/WAL/SHM 없음
- [ ] 로그 없음
- [ ] lock/PID/socket 파일 없음
- [ ] 활성 claim·runtime 상태 파일 없음
- [ ] 인증정보·cookies 없음
- [ ] 다운로드한 미디어 없음
- [ ] 백업 DB 없음
- [ ] fixture 출력 없음
- [ ] 사용자 설정·config 없음
- [ ] 임시 파일 없음
- [ ] 외부 Streamlink·yt-dlp·FFmpeg binary 없음
- [ ] Windows package에 browser/Web UI·Axum presentation·Web launcher·`RUN_HEADLESS.bat`·`stream-archive-server.exe` 없음
- [ ] Linux/macOS CLI·headless surface 유지

공식 검증 명령은 [운영 가이드의 패키지 검증 절차](OPERATIONS.md#릴리스-패키지-검증)를 참고합니다.

## 최종 공개 판정

필수 CI와 모든 해당 수동 gate의 증빙이 확보돼야 공개할 수 있습니다. 실패 또는 필요한 항목의 미검증 상태를 PASS로 바꾸지 않습니다. Ready for review는 수동 RC 완료를 의미하지 않습니다. 운영자가 merge, `v1.0.0` tag 생성과 GitHub Release 공개를 별도로 결정합니다.
