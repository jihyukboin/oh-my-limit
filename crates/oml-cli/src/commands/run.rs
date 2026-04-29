use std::{env, path::PathBuf};

use oml_codex_appserver::client::{RunOptions, run_prompt};
use tokio::runtime::Runtime;

pub fn run(args: impl Iterator<Item = String>) {
    let args = args.collect::<Vec<_>>();

    if args.is_empty() {
        eprintln!("usage: oml codex run <prompt>");
        std::process::exit(2);
    }

    let prompt = args.join(" ");
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let runtime = match Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("failed to start async runtime: {error}");
            std::process::exit(1);
        }
    };

    let result = runtime.block_on(run_prompt(RunOptions { prompt, cwd }));

    match result {
        Ok(result) => {
            if result.answer.is_empty() {
                println!("(empty assistant response)");
            } else {
                println!("{}", result.answer);
            }
        }
        Err(error) => {
            eprintln!("oml codex run failed: {error:#}");
            std::process::exit(1);
        }
    }
}
