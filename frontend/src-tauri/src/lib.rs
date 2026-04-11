use std::process::Command;

#[tauri::command]
fn reveal_in_explorer(path: String) -> Result<(), String> {
  #[cfg(target_os = "windows")]
  {
    Command::new("explorer")
      .arg(format!("/select,{}", path))
      .spawn()
      .map_err(|error| error.to_string())?;
    return Ok(());
  }

  #[cfg(not(target_os = "windows"))]
  {
    let _ = path;
    Err("Explorer reveal is only implemented on Windows".to_string())
  }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_fs::init())
    .invoke_handler(tauri::generate_handler![reveal_in_explorer])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
