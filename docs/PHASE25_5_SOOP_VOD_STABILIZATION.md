# Phase 25.5 — SOOP VOD 분석·인증 안정화

상태: PR 검증 및 실제 계정 수동 확인 단계. 공개 릴리스 완료를 의미하지 않습니다.

## 확인한 문제

- 현재 yt-dlp SOOP extractor는 PART가 하나면 단일 영상 객체를, 여러 개면 `entries` 배열을 반환합니다. 기존 앱은 배열만 허용해 단일 영상에서 `VOD PART를 찾지 못했습니다`로 종료했습니다.
- CloudFront Cookie가 없는 공개 영상도 `private_auth`를 먼저 요구했습니다. `202339229`는 비로그인 API에서 공개·비유료로 표시되며 반환된 HLS manifest가 쿠키 없이 HTTP 200을 응답했지만, 앱에서는 `private_auth -13`으로 실패했습니다. 이 확인은 전체 다운로드 성공을 의미하지 않습니다.
- yt-dlp가 추출 중 갱신한 Cookie 파일을 앱이 다시 읽지 않아 새 signed Cookie를 이전 Cookie로 덮어쓸 수 있었습니다.
- 기존 API 보완은 이미 존재하는 PART의 빈 값만 채웠고 오류를 무시했습니다. PART 목록이 없는 경우에는 보완에 도달하지 못했습니다. API의 `duration` 단위는 밀리초입니다.
- API 보완 경로가 `.sooplive.co.kr`을 사용해 `.sooplive.com` 로그인 Cookie의 domain scope와 맞지 않았습니다. 현재 yt-dlp extractor와 같은 `api.m.sooplive.com`을 사용합니다.

## 변경 범위

- 단일 영상과 여러 PART를 동일한 내부 `VodMetadata`로 정규화합니다. 화질 분석에는 선택된 하위 영상 주소보다 master manifest를 우선합니다.
- yt-dlp 추출 이후 Cookie 파일을 다시 읽어 새 인증 정보를 유지합니다.
- 추출 결과가 없거나 재생 주소가 불완전하면 동일 세션의 SOOP API 응답을 검증하고 PART 목록을 구성합니다. 삭제·비공개·구독 권한·성인 로그인 오류를 구분하며 API가 거부한 파일은 사용하지 않습니다.
- 공개 manifest는 scoped Cookie만 사용해 먼저 확인합니다. 구독 영상 표시가 있는 결과는 기존 signed Cookie 또는 정상 인증 갱신을 요구합니다. HTTP 401/403에만 추가 인증 갱신을 시도하고 다른 서버 오류를 인증 오류로 바꾸지 않습니다.
- 잘못된 HTML 응답을 화질 분석 성공으로 표시하지 않습니다. HLS 응답은 1 MiB로 제한하고, 기존 direct MP4는 Range 요청으로 파일 헤더만 확인합니다.
- 분석·다운로드·다운로드 재시도에 같은 메타데이터/인증 경로를 적용합니다. Queue는 기존 provider-neutral 서비스에서 같은 SOOP VOD provider를 사용합니다.
- 재시도 때 권한 확인이 실패하거나 최신 PART가 없으면 이전 영상 주소로 되돌아가지 않습니다.

`StreamArchiveCore`, DB schema, Queue/History contract, owned-process lifecycle은 유지합니다. UI의 직접 SQLite/HTTP/child-process 호출을 추가하지 않습니다. VOD 결과 박스의 썸네일 UI는 별도 후속 작업입니다.

## 자동 검증

실제 계정 대신 로컬 HTTP 서버와 media-tool fixture로 검증합니다. 실행 결과는 이 PR의 CI 기록을 기준으로 확인합니다.

- 단일·다중 PART, PART 순서, master manifest 우선순위
- yt-dlp의 초 단위와 SOOP API의 밀리초 단위 구분
- API 기반 PART 생성 및 숫자/문자열 오류 코드
- 구독·비공개·삭제·성인 로그인 거부 응답
- Cookie domain scope, 만료된 signed Cookie 거부
- 공개 HLS의 Cookie 없는 분석·다운로드 및 일시 실패 후 재시도
- yt-dlp가 발급한 signed Cookie를 다시 읽는 단일 구독 영상 fixture
- HTTP 403/500, HTML, 과도한 응답 크기 거부 및 direct MP4 유지
- 기존 취소·병합 실패·프로세스 소유권 테스트
- Rust fmt/test/check/clippy, RuntimeContracts 및 Windows/Linux/macOS 기존 CI

fixture 성공은 실제 구독 계정 재생 성공을 의미하지 않습니다. 개발 환경에 설치된 FFmpeg 때문에 기존 자동 탐색 테스트의 "도구가 없다"는 전제가 깨지는 경우는 원본 `main`에서도 확인하고, 테스트를 약화하지 않습니다.

## 실제 서비스 수동 확인

브라우저와 앱에서 같은 SOOP 계정을 사용해야 합니다. 비밀번호·Cookie·signed URL을 이슈나 로그에 공개하지 않습니다.

- [ ] `202339229`: 분석, 화질 선택, 다운로드 완료, 실제 파일 재생
- [ ] `208817835`: 구독 계정으로 분석, PART 확인, 다운로드 완료, 실제 파일 재생
- [ ] 미구독 계정: 구독 권한 오류를 표시하고 다운로드하지 않음
- [ ] 성인 인증이 필요한 영상: 계정 인증 조건에 맞는 결과/오류 표시
- [ ] 여러 PART 영상: 순서·길이·선택 다운로드·병합
- [ ] Cookie 만료: 정상 재로그인/갱신 또는 명확한 오류 표시
- [ ] Queue 등록·취소·재시도, History 기록
- [ ] 다운로드 중 취소 및 재실행 시 임시 파일 정리
- [ ] CHZZK VOD와 SOOP/CHZZK LIVE 기존 동작 확인
- [ ] Windows 완료·실패 알림 및 트레이 동작 확인

이 문서 작성 단계에서는 실제 구독 계정의 인증 정보가 없어 위 수동 항목을 완료로 기록하지 않습니다.
