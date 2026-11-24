use std::io::ErrorKind;

/// Result of running a helper program.
pub enum Output {
	/// The program is not installed.
	Missing,
	/// It ran but failed.
	Failed(String),
	Ok(String),
}

pub async fn run(program: &str, args: &[&str]) -> Output {
	match tokio::process::Command::new(program).args(args).output().await {
		Err(e) if e.kind() == ErrorKind::NotFound => Output::Missing,
		Err(e) => Output::Failed(e.to_string()),
		Ok(out) if out.status.success() => Output::Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
		Ok(out) => Output::Failed(String::from_utf8_lossy(&out.stderr).trim().to_owned()),
	}
}
