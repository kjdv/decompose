extern crate clap;
extern crate log;
extern crate simple_logger;

use std::error::Error;

use decompose;

fn main() -> Result<(), Box<dyn Error>> {
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
            clap::Arg::with_name("CONFIG")
                .help("configuration file, in yaml format")
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

    decompose::Program::from_yaml_file(args.value_of("CONFIG").unwrap())
        .map(|program| log::info!("program is {:?}", program))
}
