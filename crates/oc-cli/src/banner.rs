use oc_core::LOGO;

const CYAN: &str = "\x1b[1;36m";
const MAGENTA: &str = "\x1b[1;35m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

pub fn print_banner() {
    eprintln!("{CYAN}{LOGO}{RESET}");
    eprintln!();
    eprintln!(
        "  {MAGENTA}Rust Edition{RESET}  {DIM}·  v{}{RESET}",
        env!("CARGO_PKG_VERSION")
    );
    eprintln!();
}
