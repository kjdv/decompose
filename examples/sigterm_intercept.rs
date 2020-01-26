use signal_hook::{iterator::Signals, SIGINT, SIGTERM};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let signals = Signals::new(&[SIGINT, SIGTERM])?;

    for sig in signals.forever() {
        println!("Received signal {:?}", sig);
        return Ok(());
    }

    Ok(())
}
