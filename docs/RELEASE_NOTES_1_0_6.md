# Stream Archive 1.0.6 릴리스 노트

상태: **Windows Native GUI의 순수 Rust GPUI(DirectX 11) 전면 전환 — 2026-11-20**

1.0.5 이후 Windows 데스크톱 프런트엔드를 Slint(`.slint`)에서 순수 Rust GPU 가속 프레임워크인 **GPUI (`gpui-kit`, DirectX 11)** 로 전면 마이그레이션하여 UI 밀도·가독성·응답성과 메모리 효율을 대폭 개선했습니다. 저장소에서 `.slint` 파일을 완전히 제거해 프런트엔드와 백엔드·유지보수 도구 모두 순수 Rust 단일 스택으로 통합되었습니다.

## 주요 변경사항

- **GPUI (DirectX 11) 데스크톱 UI 전환:** Zed 스타일의 고대비 다크 테마, 통합 다크 타이틀바(`TitleBar`: 드래그 이동·스냅·최소화/최대화/닫기 일체형), 선명한 `Malgun Gothic` 한글 렌더링 및 네이티브 Windows IME 입력을 제공합니다.
- **1라인 컴팩트 채널 관리 테이블:** 채널 관리 탭을 `감시 스위치 · 플랫폼(SOOP/CHZZK/KICK) · 채널 ID/계정 · 표시 이름 · 저장 폴더 · 폴더/조회/삭제` 단일 행 테이블로 개편해 다수 채널을 한 화면에서 빠르게 편집할 수 있습니다.
- **트레이 숨김 시 GPU 표면 완전 해제 및 경량화:** `QuitMode::Explicit`와 영속 `AppController` 구조로 시스템 트레이 이동 시 GPUI 창과 DirectX 11 표면을 완전히 파괴해 GPU 드라이버 리셋(TDR) 영향을 차단하고 유휴 메모리를 절감합니다. 복원 시 이전 상태와 입력값을 그대로 유지합니다.
- **아키텍처 경계 및 기능 패리티 유지:** LIVE 썸네일/오프라인 플랫폼 아이콘, 저장공간 진단, VOD 분석·화질/PART 선택, 영구 Queue 상세 펼침, 6주 한글 달력 History, DPAPI 암호화 인증정보, 설정 > 백업·복원, 읽기 전용 Diagnostics/Runtime Logs 및 오프라인 CLI 유지보수 훅(`StreamArchive.exe --backup/--restore`)을 모두 `StreamArchiveCore` 경계 아래 그대로 유지했습니다.

## 지원 범위

- Windows Native GPUI GUI 및 Linux/macOS CLI·headless
- SOOP/CHZZK LIVE·VOD, KICK 공개 LIVE 및 공개·구독 VOD
- Queue / History, Backup / Restore, Diagnostics / Runtime Logs

## 패키지 및 외부 도구

| 플랫폼 | 배포 파일 |
|---|---|
| Windows x64 | `stream-archive-windows-x64.zip` |
| Linux x64 | `stream-archive-linux-x64.tar.gz` |
| macOS arm64 | `stream-archive-macos-arm64.tar.gz` |

`RELEASE_INFO.txt`, Windows EXE `ProductVersion`, Unix CLI version은 1.0.6여야 합니다. Streamlink, yt-dlp, FFmpeg는 별도로 설치하며 공식 패키지에 번들하지 않습니다.

## 업그레이드 안내 (upgrade)

1. 녹화·다운로드를 정리하고 Backup을 생성하여 무결성을 확인합니다.
2. 앱을 정상 종료하고 이전 패키지와 백업을 보관합니다.
3. 1.0.6 archive를 새 디렉터리에 압축 해제하고 기존 `data` 경로를 유지해 실행한 뒤 설정·Channels·Queue·History를 확인합니다.
