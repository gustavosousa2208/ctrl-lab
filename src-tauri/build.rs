use std::{env, fs, path::PathBuf};

fn main() {
  ensure_windows_icon();
  tauri_build::build()
}

fn ensure_windows_icon() {
  let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
  if target_os != "windows" {
    return;
  }

  let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
  let icon_path = manifest_dir.join("icons").join("icon.ico");
  if icon_path.exists() {
    return;
  }

  if let Some(parent) = icon_path.parent() {
    let _ = fs::create_dir_all(parent);
  }

  let mut bytes = Vec::with_capacity(1150);

  // ICO header with a single 16x16 32-bit image.
  bytes.extend_from_slice(&0u16.to_le_bytes());
  bytes.extend_from_slice(&1u16.to_le_bytes());
  bytes.extend_from_slice(&1u16.to_le_bytes());
  bytes.push(16);
  bytes.push(16);
  bytes.push(0);
  bytes.push(0);
  bytes.extend_from_slice(&1u16.to_le_bytes());
  bytes.extend_from_slice(&32u16.to_le_bytes());
  bytes.extend_from_slice(&1128u32.to_le_bytes());
  bytes.extend_from_slice(&22u32.to_le_bytes());

  // BITMAPINFOHEADER.
  bytes.extend_from_slice(&40u32.to_le_bytes());
  bytes.extend_from_slice(&16i32.to_le_bytes());
  bytes.extend_from_slice(&32i32.to_le_bytes());
  bytes.extend_from_slice(&1u16.to_le_bytes());
  bytes.extend_from_slice(&32u16.to_le_bytes());
  bytes.extend_from_slice(&0u32.to_le_bytes());
  bytes.extend_from_slice(&1024u32.to_le_bytes());
  bytes.extend_from_slice(&0i32.to_le_bytes());
  bytes.extend_from_slice(&0i32.to_le_bytes());
  bytes.extend_from_slice(&0u32.to_le_bytes());
  bytes.extend_from_slice(&0u32.to_le_bytes());

  // Opaque black pixels in BGRA order.
  for _ in 0..256 {
    bytes.extend_from_slice(&[0, 0, 0, 255]);
  }

  // Empty AND mask.
  bytes.extend(std::iter::repeat_n(0u8, 64));

  fs::write(icon_path, bytes).expect("failed to write fallback Windows icon");
}
