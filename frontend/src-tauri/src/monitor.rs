//! Live monitor: reads a board's streamed trace off its console port.
//!
//! Read-only. This watches what a board is doing; it never tells it to do
//! anything. See `.internal/specs/2026-09-10-live-monitor-design.md`.
//!
//! No serial crate. `firmware/scripts/console.py` already establishes that a
//! macOS `/dev/cu.*` device is a plain character file once `stty` has set the
//! line discipline, and the same is true from Rust - so the dependency here is
//! `stty`, which is already a dependency of the tooling beside it.

use std::fs::File;
use std::io::Read;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// The console baud the control firmware uses (`CTRL_CONSOLE_BAUD`).
///
/// macOS accepts only the standard ladder on this driver - 115200, 230400,
/// 460800, 921600 - and rejects anything above, so 921600 is the ceiling for
/// the ST-Link VCP path rather than a tuning choice.
pub const DEFAULT_BAUD: u32 = 921_600;

#[derive(Default)]
pub struct MonitorState {
  running: Arc<AtomicBool>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MonitorRow {
  /// The board's own clock, in seconds, from the row's first column.
  t: f32,
  values: Vec<f32>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MonitorStatus {
  running: bool,
  /// Rows parsed and emitted.
  rows: u64,
  /// Lines discarded because they were not a whole row.
  ///
  /// Not a curiosity: this port damages roughly one row per run, always by
  /// dropping the start of a line, and the count is the honest way to show it.
  /// The alternative - interpolating across the gap - would put a fabricated
  /// sample on a plot whose entire purpose is showing what the board did.
  damaged: u64,
  /// Present only when the reader stopped because of an error.
  error: Option<String>,
}

/// Ports that could be a board. The caller picks; guessing the wrong one
/// produces silence rather than an error, which is hard to tell from a board
/// that is simply not running.
#[tauri::command]
pub fn monitor_ports() -> Vec<String> {
  let mut ports: Vec<String> = std::fs::read_dir("/dev")
    .into_iter()
    .flatten()
    .flatten()
    .filter_map(|entry| entry.file_name().into_string().ok())
    // cu.* rather than tty.*: cu does not block waiting for carrier detect.
    .filter(|name| name.starts_with("cu.usbmodem") || name.starts_with("ttyACM"))
    .map(|name| format!("/dev/{name}"))
    .collect();
  ports.sort();
  ports
}

/// Puts the port into the line discipline the board's output needs.
///
/// `clocal -crtscts` is not optional and getting it wrong looks exactly like a
/// firmware bug: macOS defaults this port to hardware flow control, the
/// ST-Link VCP does not drive RTS/CTS, so the kernel throttles itself and
/// drops bytes mid-line. `-ixon -ixoff` for the same class of reason, so no
/// data byte is ever read as a flow-control character.
///
/// `min 0 time 1` comes last on purpose. `raw` sets min=1 time=0, which makes
/// a read block until at least one byte arrives - and a blocked read cannot
/// notice that the user pressed stop. With these, a read returns after 100 ms
/// with whatever is there, including nothing.
fn configure_port(port: &str, baud: u32) -> Result<(), String> {
  let output = Command::new("stty")
    .args([
      "-f", port, &baud.to_string(), "cs8", "-cstopb", "-parenb", "raw", "-echo",
      "clocal", "-crtscts", "-ixon", "-ixoff", "min", "0", "time", "1",
    ])
    .output()
    .map_err(|error| format!("could not run stty: {error}"))?;

  if !output.status.success() {
    return Err(format!(
      "stty rejected {baud} baud on {port}: {}",
      String::from_utf8_lossy(&output.stderr).trim()
    ));
  }
  Ok(())
}

/// Parses one streamed row: `T,<8 hex>,<8 hex>,...`, each field the raw
/// little-endian f32 bits. Returns None for anything else, which is how a
/// damaged line is discarded rather than guessed at.
fn parse_row(line: &str) -> Option<MonitorRow> {
  let body = line.strip_prefix("T,")?;
  let mut fields = body.split(',');

  let mut values = Vec::new();
  let t = f32::from_bits(u32::from_str_radix(fields.next()?.trim(), 16).ok()?);

  for field in fields {
    let field = field.trim();
    if field.len() != 8 {
      return None;
    }
    values.push(f32::from_bits(u32::from_str_radix(field, 16).ok()?));
  }

  if values.is_empty() {
    return None;
  }
  Some(MonitorRow { t, values })
}

#[tauri::command]
pub fn monitor_start(
  app: AppHandle,
  state: tauri::State<'_, MonitorState>,
  port: String,
  baud: Option<u32>,
) -> Result<(), String> {
  if state.running.swap(true, Ordering::SeqCst) {
    return Err("the monitor is already running".to_string());
  }

  let baud = baud.unwrap_or(DEFAULT_BAUD);
  if let Err(error) = configure_port(&port, baud) {
    state.running.store(false, Ordering::SeqCst);
    return Err(error);
  }

  let mut file = match File::open(&port) {
    Ok(file) => file,
    Err(error) => {
      state.running.store(false, Ordering::SeqCst);
      return Err(format!("could not open {port}: {error}"));
    }
  };

  let running = Arc::clone(&state.running);

  std::thread::spawn(move || {
    let mut pending = String::new();
    let mut buffer = [0u8; 4096];
    let mut rows: u64 = 0;
    let mut damaged: u64 = 0;
    let mut error = None;

    while running.load(Ordering::SeqCst) {
      let read = match file.read(&mut buffer) {
        Ok(0) => {
          // `min 0 time 1` expired with nothing to say. Not an end of stream:
          // a board between runs is simply quiet.
          std::thread::sleep(Duration::from_millis(20));
          continue;
        }
        Ok(n) => n,
        Err(io) => {
          error = Some(format!("read failed: {io}"));
          break;
        }
      };

      // Bytes, not chars: the board also prints human text on this port, and a
      // partial UTF-8 sequence at a chunk boundary must not abort the reader.
      pending.push_str(&String::from_utf8_lossy(&buffer[..read]));

      while let Some(end) = pending.find('\n') {
        let line: String = pending.drain(..=end).collect();
        let line = line.trim();

        // Only rows are of interest; the board's banner and its end-of-run
        // report share this port and are not damage.
        if !line.starts_with("T,") {
          continue;
        }

        match parse_row(line) {
          Some(row) => {
            rows += 1;
            let _ = app.emit("monitor://row", row);
          }
          None => damaged += 1,
        }
      }

      let _ = app.emit(
        "monitor://status",
        MonitorStatus { running: true, rows, damaged, error: None },
      );
    }

    running.store(false, Ordering::SeqCst);
    let _ = app.emit(
      "monitor://status",
      MonitorStatus { running: false, rows, damaged, error },
    );
  });

  Ok(())
}

#[tauri::command]
pub fn monitor_stop(state: tauri::State<'_, MonitorState>) {
  state.running.store(false, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
  use super::*;

  /// A row exactly as the firmware emits it: t, then one field per signal.
  #[test]
  fn parses_a_whole_row() {
    let row = parse_row("T,3d4ccccd,00000000,3f800000").expect("a whole row parses");
    assert_eq!(row.t, 0.05);
    assert_eq!(row.values, vec![0.0, 1.0]);
  }

  /// The real thing, copied from a capture on 2026-09-10. This port drops the
  /// start of about one line per run, and the result is still a plausible
  /// comma-separated list of hex - which is exactly why it has to be rejected
  /// on shape rather than on looking wrong.
  #[test]
  fn rejects_the_damage_this_port_actually_produces() {
    assert!(parse_row("a6bc,3f800000,3f800000,3f1caca2,3d437f75").is_none());
    assert!(parse_row("9b8,3d4738db").is_none());
  }

  /// A truncated field is the other shape damage takes, and 4 hex digits parse
  /// perfectly well as a number - so length is checked, not just parseability.
  #[test]
  fn rejects_a_short_field() {
    assert!(parse_row("T,3d4ccccd,0000").is_none());
    assert!(parse_row("T,3d4ccccd,zzzzzzzz").is_none());
  }

  /// The board shares this port with its banner and its end-of-run report.
  /// Those are not damage and must not be counted as such.
  #[test]
  fn ignores_lines_that_are_not_rows() {
    assert!(parse_row("stream_begin signals=6").is_none());
    assert!(parse_row("deadlines   0 missed of 501 ticks").is_none());
  }

  /// t alone is not a sample.
  #[test]
  fn rejects_a_row_with_no_signals() {
    assert!(parse_row("T,3d4ccccd").is_none());
  }

  /// Against a real board, because everything above tests the parser and
  /// nothing tests the port. The line discipline is the part that fails
  /// silently - a wrong `crtscts` drops bytes rather than erroring - so this
  /// exercises configure_port and a real read and reports what it saw.
  ///
  ///   CTRL_MONITOR_PORT=/dev/cu.usbmodem11403 \
  ///     cargo test --manifest-path frontend/src-tauri/Cargo.toml \
  ///     -- --ignored --nocapture
  ///
  /// Ignored by default: it needs a board that is streaming.
  #[test]
  #[ignore]
  fn reads_rows_from_a_real_board() {
    use std::io::Read;
    use std::time::Instant;

    let port = std::env::var("CTRL_MONITOR_PORT")
      .expect("set CTRL_MONITOR_PORT to the board's console device");

    configure_port(&port, DEFAULT_BAUD).expect("the port must accept the line discipline");
    let mut file = std::fs::File::open(&port).expect("the port must open");

    let mut pending = String::new();
    let mut buffer = [0u8; 4096];
    let (mut rows, mut damaged) = (0u64, 0u64);
    let deadline = Instant::now() + Duration::from_secs(20);

    while Instant::now() < deadline {
      let read = match file.read(&mut buffer) {
        Ok(0) => continue,
        Ok(n) => n,
        Err(error) => panic!("read failed: {error}"),
      };
      pending.push_str(&String::from_utf8_lossy(&buffer[..read]));

      while let Some(end) = pending.find('\n') {
        let line: String = pending.drain(..=end).collect();
        let line = line.trim();
        if !line.starts_with("T,") {
          continue;
        }
        match parse_row(line) {
          Some(_) => rows += 1,
          None => damaged += 1,
        }
      }
    }

    println!("read {rows} rows, discarded {damaged} damaged lines");
    assert!(rows > 0, "no rows arrived - is the board streaming, and is this the right port?");
  }
}
