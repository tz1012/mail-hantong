//! Synthetic input probe for the owned-PTY native terminal checks; never packaged.
use hantong_platform::{command_prompt, interactive_select};

fn main() {
    let result = match std::env::args().nth(1).as_deref() {
        Some("prompt") => {
            command_prompt::read_command_line("Mail > ", &["/help".to_owned(), "/back".to_owned()])
        }
        Some("choice") => command_prompt::read_command_line_with_selection(
            "Account > ",
            &[],
            &[
                ("first".to_owned(), "Synthetic first".to_owned()),
                ("second".to_owned(), "Synthetic second".to_owned()),
            ],
            0,
        ),
        Some("select") => interactive_select(
            &["Synthetic first".to_owned(), "Synthetic second".to_owned()],
            0,
        )
        .map(|value| value.map(|index| index.to_string())),
        _ => std::process::exit(2),
    };
    match result {
        Ok(value) => println!("PROBE_RESULT={}", value.as_deref().unwrap_or("cancelled")),
        Err(error) => {
            eprintln!("{}", error.as_str());
            std::process::exit(1);
        }
    }
}
