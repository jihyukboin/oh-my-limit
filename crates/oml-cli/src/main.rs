mod commands;

fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "help".to_owned());

    match command.as_str() {
        "doctor" => commands::doctor::run(),
        "codex" => match args.next().as_deref() {
            Some("run") => commands::run::run(),
            _ => commands::codex::run(),
        },
        "bench" => commands::bench::run(),
        "help" | "--help" | "-h" => commands::help::run(),
        _ => {
            eprintln!("unknown command: {command}");
            commands::help::run();
            std::process::exit(2);
        }
    }
}
