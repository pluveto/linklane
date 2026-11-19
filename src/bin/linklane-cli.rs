use std::{
    io,
    process::{Command, ExitCode, Stdio},
};

fn main() -> ExitCode {
    let result = (|| -> io::Result<_> {
        let app = std::env::current_exe()?.with_file_name(if cfg!(windows) {
            "linklane.exe"
        } else {
            "linklane"
        });
        Command::new(app)
            .arg("--rules-cli")
            .args(std::env::args_os().skip(1))
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
    })();
    match result {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("无法启动轻舟。请将 linklane-cli 与 linklane 放在同一文件夹：{error}");
            ExitCode::FAILURE
        }
    }
}
