use std::io;

use tetris_tui::app;
use tetris_tui::game::Mode;

const USAGE: &str = "usage: tetris-tui [nes|modern] [start-level]\nwith no arguments, the title screen opens instead";

fn main() -> io::Result<()> {
    let mut mode = None;
    let mut start_level = None;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "nes" => mode = Some(Mode::Nes),
            "modern" => mode = Some(Mode::Modern),
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => match other.parse::<u32>() {
                Ok(level) => start_level = Some(level),
                Err(_) => {
                    eprintln!("{USAGE}");
                    std::process::exit(2);
                }
            },
        }
    }

    // Anything not given on the command line comes from the config file, and a
    // mode given here skips the title screen straight into a run.
    app::run(mode, start_level)
}
