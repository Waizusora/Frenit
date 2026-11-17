use std::process::ExitCode;

fn main() -> ExitCode {
    match finit::cli::run(std::env::args().skip(1)) {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("finit: {error}");
            ExitCode::from(2)
        }
    }
}
