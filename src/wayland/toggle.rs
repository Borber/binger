use std::{
    env, fs, io,
    os::{
        linux::net::SocketAddrExt,
        unix::{
            fs::MetadataExt,
            net::{SocketAddr, UnixListener, UnixStream},
        },
    },
    path::PathBuf,
};

use rustix::{net::sockopt::socket_peercred, process::geteuid};
use smithay_client_toolkit::reexports::calloop::{
    Interest, LoopHandle, Mode, PostAction, generic::Generic,
};

use super::{Binger, Exit, WaylandError, run_overlay};
use crate::config::{Config, ConfigError};

/// Open the overlay, or close the existing overlay on the same named Wayland display.
pub fn toggle(
    load_config: impl FnOnce() -> Result<Config, ConfigError>,
) -> Result<(), WaylandError> {
    let Some(address) = session_address().map_err(WaylandError::Toggle)? else {
        return run_overlay(load_config()?, None);
    };
    if let Some(listener) = listen_or_close(&address).map_err(WaylandError::Toggle)? {
        return run_overlay(load_config()?, Some(listener));
    }
    Ok(())
}

fn listen_or_close(address: &SocketAddr) -> io::Result<Option<UnixListener>> {
    match UnixListener::bind_addr(address) {
        Ok(listener) => Ok(Some(listener)),
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
            match UnixStream::connect_addr(address) {
                Ok(stream) if same_user(&stream)? => Ok(None),
                Ok(_) => Err(io::Error::from(io::ErrorKind::PermissionDenied)),
                // The overlay may have closed between binding and connecting.
                Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
                    UnixListener::bind_addr(address).map(Some)
                }
                Err(error) => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}

fn session_address() -> io::Result<Option<SocketAddr>> {
    if env::var_os("WAYLAND_SOCKET").is_some() {
        return Ok(None);
    }
    let Some(display) = env::var_os("WAYLAND_DISPLAY")
        .filter(|display| !display.is_empty())
        .map(PathBuf::from)
    else {
        return Ok(None);
    };
    let path = if display.is_absolute() {
        display
    } else {
        env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or_else(|| io::Error::other("XDG_RUNTIME_DIR must be an absolute path"))?
            .join(display)
    };
    // The socket inode also identifies the display when reached via a symlink.
    let session = fs::metadata(path)?;
    let name = format!(
        "binger-{}-{}-{}",
        geteuid().as_raw(),
        session.dev(),
        session.ino()
    );
    SocketAddr::from_abstract_name(name).map(Some)
}

fn same_user(stream: &UnixStream) -> io::Result<bool> {
    Ok(socket_peercred(stream)?.uid == geteuid())
}

pub(super) fn register(
    handle: LoopHandle<'_, Binger>,
    listener: UnixListener,
) -> Result<(), WaylandError> {
    listener
        .set_nonblocking(true)
        .map_err(WaylandError::Toggle)?;
    handle
        .insert_source(
            Generic::new(listener, Interest::READ, Mode::Level),
            |_, listener, state| {
                let close = listener
                    .accept()
                    .is_ok_and(|(stream, _)| same_user(&stream).unwrap_or(false));
                if close {
                    state.exit.get_or_insert(Exit::Cancel);
                }
                Ok(PostAction::Continue)
            },
        )
        .map_err(|error| WaylandError::Dispatch(error.to_string()))?;
    Ok(())
}
