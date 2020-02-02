extern crate clap;
extern crate log;
extern crate simple_logger;

use std::error::Error;

mod config;
mod execution;

fn do_main() -> Result<(), Box<dyn Error>> {
    let args = clap::App::new("decompose")
        .author("Klaas de Vries")
        .about("service orchestration for devs")
        .arg(
            clap::Arg::with_name("debug")
                .help("enable debug logging")
                .short("d")
                .long("debug"),
        )
        .arg(
            clap::Arg::with_name("config")
                .help("configuration file, in toml format")
                .required(true)
                .index(1),
        )
        .get_matches();

    let level = if args.is_present("debug") {
        log::Level::Debug
    } else {
        log::Level::Info
    };

    simple_logger::init_with_level(level)?;

    log::debug!("arguments are config file is {:?}", args);

    let sys = config::System::from_file(args.value_of("config").unwrap())?;
    log::info!("system is {:?}", sys);

    let mut exec = execution::Execution::from_config(sys)?;
    exec.wait();

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    do_main()
        .map_err(|e| {
            log::error!("{:?}", e);
            e
        })
}
