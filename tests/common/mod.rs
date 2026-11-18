use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

pub fn run(args: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rustyjs"))
        .args(args)
        // Entrypoints are absolute in file tests; imports must use the referrer,
        // not accidentally pass because the child inherited the project cwd.
        .current_dir(std::env::temp_dir())
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
            panic!("rustyjs did not exit within 15 seconds: {args:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
