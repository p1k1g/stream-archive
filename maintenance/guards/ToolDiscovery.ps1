. (Join-Path $PSScriptRoot 'Common.ps1')

$lib = Read-RepoFile 'rust-runtime/src/lib.rs'
$tools = Read-RepoFile 'rust-runtime/src/tool_discovery.rs'
$cli = Read-RepoFile 'rust-runtime/src/bin/stream-archive-cli.rs'

Assert-Match $lib 'pub mod tool_discovery;' 'Shared library boundary must export tool discovery.'
Assert-Match $tools 'ToolKind::Streamlink' 'Tool discovery must include Streamlink.'
Assert-Match $tools 'ToolKind::YtDlp' 'Tool discovery must include yt-dlp.'
Assert-Match $tools 'ToolKind::Ffmpeg' 'Tool discovery must include FFmpeg.'
Assert-Match $tools 'env::split_paths' 'Tool discovery must search PATH portably.'
Assert-Match $tools '"streamlink"' 'Unix Streamlink binary name must remain supported.'
Assert-Match $tools '"yt-dlp"' 'Unix yt-dlp binary name must remain supported.'
Assert-Match $tools '"ffmpeg"' 'Unix FFmpeg binary name must remain supported.'
Assert-Match $tools '/opt/homebrew/bin' 'macOS Homebrew tool discovery must remain supported.'
Assert-Match $tools '\.local.*bin' 'Unix user-local tool discovery must remain supported.'

Assert-Match $cli '"init" => command_init' 'CLI init command is missing.'
Assert-Match $cli '"doctor" => command_doctor' 'CLI doctor command is missing.'
Assert-Match $cli '"tools" => command_tools' 'CLI tools command is missing.'
Assert-Match $cli '"serve" => command_serve' 'CLI serve command is missing.'
Assert-Match $cli 'cli-tool-discovery' 'CLI-discovered tool settings must retain an explicit SQLite source.'
Assert-Match $cli 'STREAM_ARCHIVE_START_WATCHER' 'Headless serve --watch must use the canonical watcher startup flag.'
Assert-Match $cli 'STREAMLINK_PATH' 'CLI must configure the canonical Streamlink setting.'
Assert-Match $cli 'YT_DLP_PATH' 'CLI must configure the canonical yt-dlp setting.'
Assert-Match $cli 'FFMPEG_PATH' 'CLI must configure the canonical FFmpeg setting.'
Assert-NotMatch $cli 'powershell|cmd\.exe|System\.Windows\.Forms' 'Unix/headless CLI must not depend on Windows shell or GUI tooling.'

Write-Host 'Tool discovery and Unix CLI contracts passed.'
