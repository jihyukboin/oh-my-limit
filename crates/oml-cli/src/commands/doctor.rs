use std::{fs, path::Path, process::Command};

use oml_codex_appserver::process::{APP_SERVER_ARGS, APP_SERVER_COMMAND};

#[derive(Debug)]
struct CheckResult {
    available: bool,
    detail: String,
}

impl CheckResult {
    fn ok(detail: impl Into<String>) -> Self {
        Self {
            available: true,
            detail: detail.into(),
        }
    }

    fn failed(detail: impl Into<String>) -> Self {
        Self {
            available: false,
            detail: detail.into(),
        }
    }
}

pub fn run() {
    let codex = check_codex_version();
    let app_server = check_help(&["app-server", "--help"]);
    let schema = check_help(&["app-server", "generate-json-schema", "--help"]);
    let schema_snapshot = check_schema_snapshot();
    let exec = check_help(&["exec", "--help"]);

    println!("Oh My Limit doctor");
    print_check("codex binary", &codex);

    if codex.available {
        println!("codex version: {}", codex.detail);
    }

    print_check("app-server", &app_server);

    if app_server.available {
        let transport = if app_server.detail.contains("stdio://") {
            "stdio JSONL"
        } else {
            "unknown"
        };
        println!("transport: {transport}");
        println!(
            "app-server command: {} {}",
            APP_SERVER_COMMAND,
            APP_SERVER_ARGS.join(" ")
        );
    }

    print_check("schema generation", &schema);
    print_check("schema snapshot", &schema_snapshot);
    print_check("exec fallback", &exec);

    let recommended_runner =
        if codex.available && app_server.available && schema.available && exec.available {
            "app-server"
        } else if exec.available {
            "exec"
        } else {
            "unavailable"
        };

    println!("recommended runner: {recommended_runner}");

    if app_server.available {
        println!("warning: codex app-server is experimental; keep exec fallback enabled");
    }
}

fn check_schema_snapshot() -> CheckResult {
    let schema_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("schemas")
        .join("codex-appserver");
    let bundle = schema_dir.join("codex_app_server_protocol.schemas.json");

    if !bundle.is_file() {
        return CheckResult::failed(format!(
            "{} snapshot bundle is missing",
            schema_dir.display()
        ));
    }

    match count_schema_files(&schema_dir) {
        Ok(count) => CheckResult::ok(format!("{count} json schema files")),
        Err(error) => CheckResult::failed(error.to_string()),
    }
}

fn count_schema_files(schema_dir: &Path) -> std::io::Result<usize> {
    let mut count = 0;
    let mut pending = vec![schema_dir.to_path_buf()];

    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                count += 1;
            }
        }
    }

    Ok(count)
}

fn print_check(label: &str, result: &CheckResult) {
    let status = if result.available {
        "available"
    } else {
        "unavailable"
    };
    println!("{label}: {status}");

    if !result.available {
        println!("{label} detail: {}", result.detail);
    }
}

fn check_codex_version() -> CheckResult {
    match Command::new(APP_SERVER_COMMAND).arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = first_output_line(&output.stdout, &output.stderr);
            CheckResult::ok(version.unwrap_or_else(|| "version output empty".to_owned()))
        }
        Ok(output) => {
            CheckResult::failed(command_failure_detail(output.status.code(), &output.stderr))
        }
        Err(error) => CheckResult::failed(error.to_string()),
    }
}

fn check_help(args: &[&str]) -> CheckResult {
    match Command::new(APP_SERVER_COMMAND).args(args).output() {
        Ok(output) if output.status.success() => {
            CheckResult::ok(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        Ok(output) => {
            CheckResult::failed(command_failure_detail(output.status.code(), &output.stderr))
        }
        Err(error) => CheckResult::failed(error.to_string()),
    }
}

fn command_failure_detail(code: Option<i32>, stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    let message = stderr.lines().next().unwrap_or("no stderr");

    match code {
        Some(code) => format!("exit {code}: {message}"),
        None => format!("terminated by signal: {message}"),
    }
}

fn first_output_line(stdout: &[u8], stderr: &[u8]) -> Option<String> {
    let stdout = String::from_utf8_lossy(stdout);
    if let Some(line) = stdout.lines().next() {
        return Some(line.to_owned());
    }

    let stderr = String::from_utf8_lossy(stderr);
    stderr.lines().next().map(str::to_owned)
}
