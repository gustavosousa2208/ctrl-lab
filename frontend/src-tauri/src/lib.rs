use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use ctrl_backend::simulate_project_json;
use serde::Serialize;

fn repo_root() -> Result<PathBuf, String> {
  let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  manifest_dir
    .parent()
    .and_then(Path::parent)
    .map(Path::to_path_buf)
    .ok_or_else(|| "failed to resolve repository root".to_string())
}

fn backend_binary_path(backend_dir: &Path) -> PathBuf {
  #[cfg(target_os = "windows")]
  {
    backend_dir.join("target").join("debug").join("ctrl-backend.exe")
  }

  #[cfg(not(target_os = "windows"))]
  {
    backend_dir.join("target").join("debug").join("ctrl-backend")
  }
}

#[tauri::command]
fn compile_project_report(project_json: String) -> Result<String, String> {
  let backend_dir = repo_root()?.join("backend");
  let build_output = Command::new("cargo")
    .arg("build")
    .current_dir(&backend_dir)
    .output()
    .map_err(|error| format!("failed to run backend build: {error}"))?;

  if !build_output.status.success() {
    let stderr = String::from_utf8_lossy(&build_output.stderr);
    let stdout = String::from_utf8_lossy(&build_output.stdout);
    return Err(format!(
      "backend build failed\n\nstdout:\n{}\n\nstderr:\n{}",
      stdout.trim(),
      stderr.trim()
    ));
  }

  let timestamp = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map_err(|error| format!("failed to create temp file timestamp: {error}"))?
    .as_millis();
  let temp_project_path = env::temp_dir().join(format!("ctrl-lab-compile-{timestamp}.json"));
  fs::write(&temp_project_path, project_json)
    .map_err(|error| format!("failed to write temporary project file: {error}"))?;

  let backend_output = Command::new(backend_binary_path(&backend_dir))
    .arg(&temp_project_path)
    .current_dir(&backend_dir)
    .output()
    .map_err(|error| format!("failed to run backend compiler: {error}"))?;

  let _ = fs::remove_file(&temp_project_path);

  if backend_output.status.success() {
    Ok(String::from_utf8_lossy(&backend_output.stdout).trim().to_string())
  } else {
    let stderr = String::from_utf8_lossy(&backend_output.stderr);
    let stdout = String::from_utf8_lossy(&backend_output.stdout);
    Err(format!(
      "compile failed\n\nstdout:\n{}\n\nstderr:\n{}",
      stdout.trim(),
      stderr.trim()
    ))
  }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SimulationTraceResponse {
  times: Vec<f64>,
  values_by_node_id: std::collections::HashMap<String, Vec<f64>>,
}

#[tauri::command]
fn simulate_project(project_json: String) -> Result<SimulationTraceResponse, String> {
  let simulation = simulate_project_json(&project_json).map_err(|error| error.to_string())?;

  Ok(SimulationTraceResponse {
    times: simulation.times,
    values_by_node_id: simulation.values_by_node_id,
  })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_fs::init())
    .invoke_handler(tauri::generate_handler![compile_project_report, simulate_project])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
