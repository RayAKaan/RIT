//! Local newline-delimited JSON-RPC discovery transport; no network listener.
use std::io::{self, BufRead, Read, Write};
fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    loop {
        let mut line = Vec::new();
        let n = (&mut input).take(65_537).read_until(b'\n', &mut line)?;
        if n == 0 {
            break;
        }
        if n > 65_536 || !line.ends_with(b"\n") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request too large or unterminated",
            ));
        }
        let text = std::str::from_utf8(&line)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8"))?;
        if let Some(reply) = rit_protocol::dispatch(text) {
            serde_json::to_writer(&mut output, &reply)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    Ok(())
}
