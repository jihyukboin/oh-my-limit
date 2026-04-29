pub fn run() {
    if let Err(error) = crate::tui::run() {
        eprintln!("failed to start TUI: {error}");
        std::process::exit(1);
    }
}
