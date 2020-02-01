extern crate clap;
extern crate log;
extern crate simple_logger;

use decompose;

use std::error::Error;

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

    let level = match args.is_present("debug") {
        true => log::Level::Debug,
        false => log::Level::Info,
    };

    simple_logger::init_with_level(level)?;

    log::debug!("arguments are config file is {:?}", args);

    let sys = decompose::config::System::from_file(args.value_of("config").unwrap())?;
    log::info!("system is {:?}", sys);

    let listener: decompose::execution::EventLogger = ();
    let mut exec = decompose::execution::Execution::from_config(sys, listener)?;
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
