use std::{env, path::PathBuf, process::ExitCode};

use binger::{config::Config, input::AREA_KEY_COUNT};

fn print_help() {
    println!(
        "binger\n\nUsage\n  binger [--config PATH] [--print-keymap]\n\nOn Linux, run binger again to close the overlay in the current Wayland session.\n\nOptions\n  --config PATH     load a TOML configuration file\n  --print-keymap    print the selection, click and nudge keys\n  -h, --help        show this help\n  -V, --version     show the version\n"
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
            "--toggle" => {} // Accepted by existing Hyprland bindings.
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

    let load_config = || config_path.map_or_else(Config::load_default, Config::load);
    #[cfg(all(feature = "wayland-backend", target_os = "linux"))]
    if !print_keymap {
        return report_wayland_result(binger::wayland::toggle(load_config));
    }

    let config = match load_config() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(1);
        }
    };

    if print_keymap {
        let key_map = config.key_map().expect("the default config is valid");
        println!(
            "area keys: {}",
            (0..AREA_KEY_COUNT)
                .filter_map(|index| key_map.area_key(index))
                .collect::<String>()
        );
        println!(
            "left click: {} (Enter/Space)",
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
        let nudge_keys: String = key_map.nudge_keys().collect();
        println!(
            "nudge keys: {}",
            if nudge_keys.is_empty() {
                "(none)"
            } else {
                &nudge_keys
            }
        );
        return ExitCode::SUCCESS;
    }

    #[cfg(feature = "wayland-backend")]
    return report_wayland_result(binger::wayland::run(config));

    #[cfg(not(feature = "wayland-backend"))]
    {
        println!("binger core build");
        println!("configured keys: {}", config.home_row_keys);
        println!("Wayland backend is disabled in this build; use --features wayland-backend");
        ExitCode::SUCCESS
    }
}

#[cfg(feature = "wayland-backend")]
fn report_wayland_result(result: Result<(), binger::wayland::WaylandError>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}
