#[cfg(unix)]
use std::os::unix::process::CommandExt;

use std::{
    env, fs,
    fs::OpenOptions,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
    thread,
    time::Duration,
};

#[derive(Debug, Clone, Copy)]
enum ProviderTool {
    Streamlink,
    YtDlp,
    Ffmpeg,
}

fn main() {
    let mut args = env::args_os().skip(1).collect::<Vec<_>>();

    if let Some(tool) = provider_tool() {
        record_provider_invocation(tool, &args);
        run_provider_tool(tool, &args);
        return;
    }

    if args.first().is_some_and(|arg| arg == "--version" || arg == "-version") {
        match env::var("STREAM_ARCHIVE_FIXTURE_VERSION_MODE").as_deref() {
            Ok("fail") => {
                eprintln!("fixture version failure");
                process::exit(9);
            }
            Ok("hang") => {
                thread::sleep(Duration::from_secs(30));
                return;
            }
            _ => {
                println!("fixture-media-tool 1.2.3");
                return;
            }
        }
    }

    if args.is_empty() {
        eprintln!("missing fixture command");
        process::exit(2);
    }
    let command = args.remove(0).to_string_lossy().into_owned();

    match command.as_str() {
        "echo-args" => {
            for (index, arg) in args.iter().enumerate() {
                println!("{index}:{}", arg.to_string_lossy());
            }
        }
        "emit" => {
            if args.len() != 3 {
                process::exit(2);
            }
            println!("{}", args[0].to_string_lossy());
            eprintln!("{}", args[1].to_string_lossy());
            let code = args[2].to_string_lossy().parse::<i32>().unwrap_or(2);
            process::exit(code);
        }
        "large-output" => {
            let bytes = args
                .first()
                .and_then(|value| value.to_string_lossy().parse::<usize>().ok())
                .unwrap_or(131_072);
            let stdout = io::stdout();
            let stderr = io::stderr();
            let mut out = stdout.lock();
            let mut err = stderr.lock();
            let chunk = vec![b'O'; 4096];
            let err_chunk = vec![b'E'; 4096];
            let mut written = 0usize;
            while written < bytes {
                let count = (bytes - written).min(chunk.len());
                out.write_all(&chunk[..count]).unwrap();
                err.write_all(&err_chunk[..count]).unwrap();
                written += count;
            }
            out.write_all(b"STDOUT-END\n").unwrap();
            err.write_all(b"STDERR-END\n").unwrap();
            out.flush().unwrap();
            err.flush().unwrap();
        }
        "invalid-utf8" => {
            io::stdout().write_all(&[b'A', 0xff, b'Z', b'\n']).unwrap();
            io::stderr().write_all(&[b'E', 0xfe, b'R', b'\n']).unwrap();
        }
        "cwd-env" => {
            if args.len() != 2 {
                process::exit(2);
            }
            let key = args[0].to_string_lossy();
            println!("cwd={}", env::current_dir().unwrap().display());
            println!("env={}", env::var(key.as_ref()).unwrap_or_default());
            println!("arg={}", args[1].to_string_lossy());
        }
        "write-file" => {
            if args.len() != 2 {
                process::exit(2);
            }
            let path = Path::new(&args[0]);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, args[1].to_string_lossy().as_bytes()).unwrap();
        }
        "sleep" => {
            if args.is_empty() {
                process::exit(2);
            }
            let millis = parse_millis(&args[0]);
            if let Some(ready) = args.get(1) {
                write_marker(Path::new(ready));
            }
            thread::sleep(Duration::from_millis(millis));
        }
        "spawn-detached-output-holder" => {
            #[cfg(unix)]
            {
                if args.len() != 2 {
                    process::exit(2);
                }
                let pid_path = args[0].clone();
                let ready_path = args[1].clone();
                let mut child_command = Command::new(env::current_exe().unwrap());
                child_command
                    .arg("sleep")
                    .arg("30000")
                    .arg(&ready_path)
                    .stdin(Stdio::null())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit());
                child_command.process_group(0);
                let child = child_command.spawn().unwrap();
                fs::write(&pid_path, child.id().to_string()).unwrap();
                wait_for_marker(Path::new(&ready_path));
                thread::sleep(Duration::from_secs(30));
            }
            #[cfg(not(unix))]
            {
                eprintln!("spawn-detached-output-holder is Unix-only");
                process::exit(2);
            }
        }
        "spawn-child" | "spawn-child-ready" => {
            if args.len() < 2 {
                process::exit(2);
            }
            let millis = args[0].clone();
            let pid_path = args[1].clone();
            let child_ready = env::temp_dir().join(format!(
                "stream-archive-fixture-child-{}.ready",
                process::id()
            ));
            let child = Command::new(env::current_exe().unwrap())
                .arg("sleep")
                .arg(millis)
                .arg(&child_ready)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            fs::write(&pid_path, child.id().to_string()).unwrap();
            wait_for_marker(&child_ready);
            if command == "spawn-child-ready" {
                let Some(ready) = args.get(2) else {
                    process::exit(2);
                };
                write_marker(Path::new(ready));
            }
            thread::sleep(Duration::from_secs(30));
        }
        other => {
            eprintln!("unknown fixture command: {other}");
            process::exit(2);
        }
    }
}

fn provider_tool() -> Option<ProviderTool> {
    if env::var_os("STREAM_ARCHIVE_FIXTURE_GENERIC").is_some() {
        return None;
    }
    let name = env::current_exe()
        .ok()?
        .file_name()?
        .to_string_lossy()
        .to_ascii_lowercase();
    if name.starts_with("streamlink") {
        Some(ProviderTool::Streamlink)
    } else if name.starts_with("yt-dlp") || name.starts_with("ytdlp") {
        Some(ProviderTool::YtDlp)
    } else if name.starts_with("ffmpeg") {
        Some(ProviderTool::Ffmpeg)
    } else {
        None
    }
}

fn run_provider_tool(tool: ProviderTool, args: &[std::ffi::OsString]) {
    let mode = provider_mode();
    match tool {
        ProviderTool::Streamlink => run_streamlink_fixture(args, &mode),
        ProviderTool::YtDlp => run_ytdlp_fixture(args, &mode),
        ProviderTool::Ffmpeg => run_ffmpeg_fixture(args, &mode),
    }
}

fn run_streamlink_fixture(args: &[std::ffi::OsString], mode: &str) {
    let preflight = has_arg(args, "--can-handle-url") || has_arg(args, "--help");
    if preflight {
        match mode {
            "preflight-fail" => {
                eprintln!("fixture streamlink preflight failure");
                process::exit(8);
            }
            "preflight-hang" => {
                thread::sleep(Duration::from_secs(30));
                return;
            }
            _ => {}
        }
        if has_arg(args, "--help") {
            println!("fixture streamlink help --http-cookies-file --player --output");
        }
        return;
    }

    apply_run_mode(mode);

    if has_arg(args, "--stdout") {
        let mut out = io::stdout().lock();
        let chunk = vec![b'M'; 8192];
        for _ in 0..32 {
            out.write_all(&chunk).unwrap();
        }
        out.flush().unwrap();
        return;
    }

    if let Some(output) = arg_after(args, "--output") {
        write_sized_file(Path::new(output), 64 * 1024);
        return;
    }

    if let Some(player_args) = arg_after(args, "--player-args")
        && let Some(output) = last_quoted_path(player_args)
    {
        write_sized_file(Path::new(&output), 64 * 1024);
        return;
    }

    eprintln!("fixture streamlink could not determine output contract");
    process::exit(2);
}

fn run_ytdlp_fixture(args: &[std::ffi::OsString], mode: &str) {
    apply_run_mode(mode);

    if has_arg(args, "--cookies-from-browser")
        && let Some(cookie_path) = arg_after(args, "--cookies")
    {
        if let Some(parent) = Path::new(cookie_path).parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(
            cookie_path,
            "# Netscape HTTP Cookie File\n.sooplive.com\tTRUE\t/\tTRUE\t2147483647\tfixture\tcookie\n",
        )
        .unwrap();
        return;
    }

    if has_arg(args, "--dump-single-json") {
        let manifest = provider_sidecar_value("manifest-url")
            .unwrap_or_else(|| "https://fixture.invalid/master.m3u8".into());
        println!(
            r#"{{"title":"Fixture VOD","uploader":"Fixture BJ","uploader_id":"fixture","upload_date":"20260923","entries":[{{"url":"{manifest}","duration":60}}]}}"#
        );
        return;
    }

    if let Some(output) = arg_after(args, "-o") {
        write_sized_file(Path::new(output), 128 * 1024);
        println!("[download] 50.0%");
        println!("[download] 100.0%");
        return;
    }

    println!("fixture yt-dlp");
}

fn run_ffmpeg_fixture(args: &[std::ffi::OsString], mode: &str) {
    apply_run_mode(mode);

    let mut input = Vec::new();
    let _ = io::stdin().read_to_end(&mut input);

    let target = args
        .iter()
        .rev()
        .map(|value| value.to_string_lossy().into_owned())
        .find(|value| !value.starts_with('-') && !value.starts_with("pipe:"));

    if let Some(target) = target {
        write_sized_file(Path::new(&target), 2 * 1024 * 1024);
    }

    println!("out_time_us=1000000");
    println!("progress=end");
}

fn apply_run_mode(mode: &str) {
    match mode {
        "run-fail" => {
            eprintln!("fixture provider process failure");
            process::exit(7);
        }
        "run-hang" => {
            thread::sleep(Duration::from_secs(30));
            process::exit(0);
        }
        "run-spawn-child" => spawn_provider_child_and_hang(),
        _ => {}
    }
}

fn spawn_provider_child_and_hang() -> ! {
    let exe = env::current_exe().unwrap();
    let parent = exe.parent().unwrap();
    let name = exe.file_name().unwrap().to_string_lossy();
    let pid_path = parent.join(format!("{name}.child.pid"));
    let ready_path = parent.join(format!("{name}.child.ready"));
    let child = Command::new(&exe)
        .env("STREAM_ARCHIVE_FIXTURE_GENERIC", "1")
        .arg("sleep")
        .arg("30000")
        .arg(&ready_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    fs::write(&pid_path, child.id().to_string()).unwrap();
    wait_for_marker(&ready_path);
    thread::sleep(Duration::from_secs(30));
    process::exit(0);
}

fn provider_mode() -> String {
    let Some(path) = provider_mode_path() else {
        return "success".into();
    };
    fs::read_to_string(path)
        .unwrap_or_else(|_| "success".into())
        .trim()
        .to_string()
}

fn provider_mode_path() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let name = exe.file_name()?.to_string_lossy();
    Some(exe.parent()?.join(format!("{name}.mode")))
}

fn provider_sidecar_value(suffix: &str) -> Option<String> {
    let exe = env::current_exe().ok()?;
    let name = exe.file_name()?.to_string_lossy();
    let path = exe.parent()?.join(format!("{name}.{suffix}"));
    fs::read_to_string(path).ok().map(|value| value.trim().to_string())
}

fn record_provider_invocation(tool: ProviderTool, args: &[std::ffi::OsString]) {
    let Ok(exe) = env::current_exe() else {
        return;
    };
    let Some(parent) = exe.parent() else {
        return;
    };
    let path = parent.join("invocations.log");
    let mut file = match OpenOptions::new().create(true).append(true).open(path) {
        Ok(file) => file,
        Err(_) => return,
    };
    let cwd = env::current_dir()
        .map(|value| value.display().to_string())
        .unwrap_or_default();
    let utf8 = env::var("PYTHONUTF8").unwrap_or_default();
    let io_encoding = env::var("PYTHONIOENCODING").unwrap_or_default();
    let mut record = format!(
        "tool={tool:?}\ncwd={cwd}\nPYTHONUTF8={utf8}\nPYTHONIOENCODING={io_encoding}\n"
    );
    for (index, arg) in args.iter().enumerate() {
        record.push_str(&format!("argv[{index}]={}\n", arg.to_string_lossy()));
    }
    record.push_str("---\n");
    let _ = file.write_all(record.as_bytes());
    let _ = file.flush();
}

fn has_arg(args: &[std::ffi::OsString], needle: &str) -> bool {
    args.iter().any(|value| value == needle)
}

fn arg_after<'a>(args: &'a [std::ffi::OsString], needle: &str) -> Option<&'a str> {
    let index = args.iter().position(|value| value == needle)?;
    args.get(index + 1)?.to_str()
}

fn last_quoted_path(value: &str) -> Option<String> {
    let end = value.rfind('"')?;
    let start = value[..end].rfind('"')?;
    Some(value[start + 1..end].replace("{{", "{").replace("}}", "}"))
}

fn write_sized_file(path: &Path, size: usize) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let bytes = vec![b'F'; size];
    fs::write(path, bytes).unwrap();
}

fn parse_millis(value: &std::ffi::OsStr) -> u64 {
    value.to_string_lossy().parse::<u64>().unwrap_or(30_000)
}

fn write_marker(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(path, b"READY").unwrap();
}

fn wait_for_marker(path: &Path) {
    for _ in 0..400 {
        if path.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("fixture child did not become ready: {}", path.display());
}
