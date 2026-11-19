use std::{env, path::PathBuf, process::ExitCode};

use binger::config::Config;

fn print_help() {
    println!(
        "binger\n\nUsage\n  binger [--config PATH] [--print-keymap]\n\nOptions\n  --config PATH     load a TOML configuration file\n  --print-keymap    print the configured selection and click keys\n  -h, --help        show this help\n  -V, --version     show the version\n"
    );
}

fn main() -> ExitCode {
    let mut config_path: Option<PathBuf> = None;
    let mut print_keymap = false;
    let mut args = env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("binger {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--print-keymap" => print_keymap = true,
            "--config" => {
                let Some(path) = args.next() else {
                    eprintln!("--config requires a path");
                    return ExitCode::from(2);
                };
                config_path = Some(PathBuf::from(path));
            }
            unknown => {
                eprintln!("unknown argument: {unknown}");
                print_help();
                return ExitCode::from(2);
            }
        }
    }

    let config = match config_path {
        Some(path) => match Config::load(path) {
            Ok(config) => config,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::from(1);
            }
        },
        None => Config::default(),
    };

    if print_keymap {
        let key_map = config.key_map().expect("the default config is valid");
        println!(
            "area keys: {}",
            (0..8)
                .filter_map(|index| key_map.area_key(index))
                .collect::<String>()
        );
        println!(
            "left click: {}",
            key_map.click_key(binger::input::MouseButton::Left)
        );
        println!(
            "right click: {}",
            key_map.click_key(binger::input::MouseButton::Right)
        );
        println!(
            "middle click: {}",
            key_map.click_key(binger::input::MouseButton::Middle)
        );
        return ExitCode::SUCCESS;
    }

    #[cfg(feature = "wayland-backend")]
    return match binger::wayland::run(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    };

    #[cfg(not(feature = "wayland-backend"))]
    {
        println!("binger core build");
        println!("configured keys: {}", config.home_row_keys);
        println!("Wayland backend is disabled in this build; use --features wayland-backend");
        ExitCode::SUCCESS
    }
}
