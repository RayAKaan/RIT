//! Local newline-delimited JSON-RPC transport; no network listener.
use std::io;
fn main() -> io::Result<()> {
    let mut args = std::env::args_os();
    let _program = args.next();
    let scope = match args.next() {
        None => None,
        Some(flag) if flag == "--repo" => {
            let path = args.next().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "--repo requires a path")
            })?;
            if args.next().is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unexpected arguments",
                ));
            }
            Some(
                rit_protocol::ReadOnlyScope::open(std::path::Path::new(&path))
                    .map_err(io::Error::other)?,
            )
        }
        Some(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "usage: rit-protocol [--repo PATH]",
            ));
        }
    };
    rit_protocol::serve_stdio(scope)
}
