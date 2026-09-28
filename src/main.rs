use std::{
    env, fs, io,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    const USAGE: &str = "Usage: kdbl -b <PATH> -r <PATH> [-D <DIR>]\n\nOptions:\n  -b, --binary <PATH>    Dropbear multi-call binary (required)\n  -r, --host-key <PATH>  Ed25519 host key; generated if missing (required)\n  -D, --authorized-keys-dir <DIR>  Directory containing authorized_keys\n  -h, --help             Show this help\n\nRuns on 0.0.0.0:2222.";
    let mut args = env::args_os().skip(1);
    let mut binary = None;
    let mut host_key = None;
    let mut authorized_keys_dir = None;
    while let Some(option) = args.next() {
        if option == "--help" || option == "-h" {
            println!("{USAGE}");
            return Ok(());
        }
        let slot = if option == "-b" || option == "--binary" {
            &mut binary
        } else if option == "-r" || option == "--host-key" {
            &mut host_key
        } else if option == "-D" || option == "--authorized-keys-dir" {
            &mut authorized_keys_dir
        } else {
            return Err(format!("unknown option: {}\n{USAGE}", option.to_string_lossy()).into());
        };
        if slot.is_some() {
            return Err(format!("duplicate option: {}", option.to_string_lossy()).into());
        }
        let value = args
            .next()
            .filter(|v| !v.is_empty() && !v.to_string_lossy().starts_with('-'))
            .ok_or_else(|| format!("missing path for {}\n{USAGE}", option.to_string_lossy()))?;
        *slot = Some(PathBuf::from(value));
    }
    let binary = binary.ok_or_else(|| format!("missing required --binary\n{USAGE}"))?;
    let host_key = host_key.ok_or_else(|| format!("missing required --host-key\n{USAGE}"))?;
    let binary = binary
        .canonicalize()
        .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", binary.display())))?;
    if binary == env::current_exe()?.canonicalize()? {
        return Err("Dropbear path points to the launcher itself".into());
    }
    // Dropbear resets PATH even with -e. Set it after session setup using
    // its forced-command hook, then run the client's original command.
    let binary_dir = binary.parent().ok_or("Dropbear has no parent directory")?;
    let binary_dir = binary_dir
        .to_str()
        .ok_or("Dropbear directory is not valid UTF-8")?;
    let quoted_dir = format!("'{}'", binary_dir.replace('\'', "'\\''"));
    let session_command = format!(
        "export PATH={quoted_dir}:\"$PATH\"; \
         if [ -n \"$SSH_ORIGINAL_COMMAND\" ]; then \
         exec \"$SHELL\" -c \"$SSH_ORIGINAL_COMMAND\"; \
         else exec \"$SHELL\" -l; fi"
    );
    if !host_key.try_exists()? {
        if let Some(parent) = host_key.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let status = Command::new(&binary)
            .arg0("dropbearkey")
            .args(["-t", "ed25519", "-f"])
            .arg(&host_key)
            .stdin(Stdio::null())
            .status()?;
        if !status.success() {
            return Err(format!("host key generation failed: {status}").into());
        }
        if !host_key.is_file() {
            return Err(format!("host key was not created: {}", host_key.display()).into());
        }
    }
    let mut command = Command::new(&binary);
    command
        // Multi-call Dropbear selects server mode using argv[0], even when
        // canonicalize() resolves a `dropbear` symlink to `dropbearmulti`.
        .arg0("dropbear")
        .args([
            "-F",
            "-e",
            "-E",
            "-B",
            "-T",
            "3",
            "-p",
            "0.0.0.0:2222",
            "-K",
            "45",
        ])
        .arg("-r")
        .arg(&host_key)
        .arg("-c")
        .arg(&session_command)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(directory) = authorized_keys_dir {
        command.arg("-D").arg(directory);
    }
    // Keep the PID tracked by Upstart and deliver stop signals to Dropbear.
    let error = command.exec();
    Err(io::Error::new(
        error.kind(),
        format!("cannot start {}: {error}", binary.display()),
    )
    .into())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("kdbl: {error}");
            ExitCode::FAILURE
        }
    }
}
