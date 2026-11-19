use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

pub fn run(args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rustyjs"));
    command
        .args(args)
        // Entrypoints are absolute in file tests; imports must use the referrer,
        // not accidentally pass because the child inherited the project cwd.
        .current_dir(std::env::temp_dir());
    run_command(command)
}

pub fn run_command(mut command: Command) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start rustyjs");
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child.try_wait().expect("poll rustyjs").is_some() {
            return child.wait_with_output().expect("collect rustyjs output");
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("runtime command did not exit within 15 seconds");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
