use std::{
    env, fs,
    io::{self, Write},
    path::Path,
    process::{self, Command, Stdio},
    thread,
    time::Duration,
};

fn main() {
    let mut args = env::args_os().skip(1).collect::<Vec<_>>();
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
