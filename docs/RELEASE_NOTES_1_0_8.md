# Stream Archive 1.0.8 릴리스 노트

상태: **CHZZK HEVC(H.265) LIVE·VOD 무손실 MPEG-TS 녹화 핫픽스 — 2026-11-22**

치지직(CHZZK)에서 HEVC(H.265) 1080p60 스트림을 송출하는 방송을 녹화할 때 FFmpeg 비트스트림 필터가 실패하던 문제를 수정했습니다.

## 주요 변경사항

- **CHZZK HEVC(H.265) LIVE·VOD 무손실 녹화 지원:** CHZZK LIVE 리코더(`recorder.rs`)와 CHZZK Streamlink VOD 파이프(`chzzk/vod.rs`)에서 H.264 전용 `-bsf:v h264_mp4toannexb` 하드코딩을 제거해, FFmpeg `mpegts` muxer의 자동 비트스트림 필터(`auto_bsf`)가 H.264(`h264_mp4toannexb`)와 HEVC(`hevc_mp4toannexb`)를 코덱에 맞게 자동 처리하도록 변경했습니다. 재인코딩 없는 무손실 `-c copy` 및 `00:00` 타임스탬프 리베이스 정책을 그대로 유지해 CPU 사용량 증가 없이 녹화됩니다.
- **0바이트 즉시 종료 감지 및 빈 파일 정리:** FFmpeg/출력 오류로 0바이트만 기록하고 Streamlink가 정상 종료 코드(`0`)로 끝난 경우를 `EMPTY OUTPUT (0 bytes)` 실패(`FAILED` / `ERROR`)로 분류하고 빈 `.ts` 파일을 자동 정리합니다.

## 지원 범위

- Windows Native GPUI GUI 및 Linux/macOS CLI·headless
- SOOP/CHZZK LIVE·VOD (H.264 / HEVC), KICK 공개 LIVE 및 공개·구독 VOD
- Queue / History, Backup / Restore, Diagnostics / Runtime Logs

## 패키지 및 외부 도구

| 플랫폼 | 배포 파일 |
|---|---|
| Windows x64 | `stream-archive-windows-x64.zip` |
| Linux x64 | `stream-archive-linux-x64.tar.gz` |
| macOS arm64 | `stream-archive-macos-arm64.tar.gz` |

`RELEASE_INFO.txt`, Windows EXE `ProductVersion`, Unix CLI version은 1.0.8여야 합니다. Streamlink, yt-dlp, FFmpeg는 별도로 설치하며 공식 패키지에 번들하지 않습니다.

## 업그레이드 안내 (upgrade)

1. 녹화·다운로드를 정리하고 Backup을 생성하여 무결성을 확인합니다.
2. 앱을 정상 종료하고 이전 패키지와 백업을 보관합니다.
3. 1.0.8 archive를 새 디렉터리에 압축 해제하고 기존 `data` 경로를 유지해 실행한 뒤 설정·Channels·Queue·History를 확인합니다.
