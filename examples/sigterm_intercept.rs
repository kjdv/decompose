extern crate clap;
extern crate tokio;

use std::error::Error;
use tokio::signal::unix::{signal, SignalKind};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
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

    let ret = !args.is_present("diehard");

    let a = print_signal(SignalKind::terminate(), ret, "SIGTERM");
    let b = print_signal(SignalKind::interrupt(), ret, "SIGINT");

    tokio::select! {
        _ = a => (),
        _ = b => (),
    }

    println!("doing a clean exit");

    Ok(())
}

async fn print_signal(kind: SignalKind, ret: bool, name: &str) {
    let mut stream = signal(kind).expect("signal");
    println!("listening for {}", name);

    loop {
        stream.recv().await;
        println!("Received signal {}", name);

        if ret {
            return;
        }
    }
}
