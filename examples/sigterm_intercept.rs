use signal_hook::{iterator::Signals, SIGINT, SIGTERM};
use std::error::Error;
use clap;

fn main() -> Result<(), Box<dyn Error>> {
    let args = clap::App::new("signal interceptor")
        .author("Klaas de Vries")
        .about("intercepts SIGINT and SIGTERM, for aide in automated tests of decompose")
        .arg(
            clap::Arg::with_name("diehard")
                .help("die hard, keep running on SIGINT or SIGTERM")
                .short("d")
                .long("diehard"),
        )
        .get_matches();

    let signals = Signals::new(&[SIGINT, SIGTERM])?;

    for sig in signals.forever() {
        println!("Received signal {:?}", sig);

        if !args.is_present("diehard") {
            return Ok(());
        }
    }

    Ok(())
}
