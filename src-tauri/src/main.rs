// Sin esto Windows abre una consola junto a la ventana en las versiones compiladas
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod convert;
mod data;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
struct FileInfo {
    path: String,
    name: String,
    ext: String,
    size: u64,
}

#[tauri::command]
fn file_info(paths: Vec<String>) -> Vec<FileInfo> {
    paths
        .into_iter()
        .filter_map(|path| {
            let meta = std::fs::metadata(&path).ok().filter(|m| m.is_file())?;
            let p = Path::new(&path);
            Some(FileInfo {
                name: p.file_name()?.to_string_lossy().into_owned(),
                ext: convert::normalize_ext(p),
                size: meta.len(),
                path,
            })
        })
        .collect()
}

#[tauri::command]
async fn convert_file(
    app: AppHandle,
    job: u32,
    path: String,
    target: String,
    out_dir: Option<String>,
) -> Result<String, String> {
    let input = PathBuf::from(path);
    // Por defecto a Descargas; si el sistema no tiene esa carpeta, junto al original
    let dir = match out_dir {
        Some(dir) => PathBuf::from(dir),
        None => app
            .path()
            .download_dir()
            .ok()
            .or_else(|| input.parent().map(Path::to_path_buf))
            .unwrap_or_default(),
    };
    convert::convert(&app, job, &input, &target, &dir)
        .await
        .map(|output| output.to_string_lossy().into_owned())
        .map_err(|failure| failure.code().to_string())
}

#[tauri::command]
fn cancel_job(app: AppHandle, job: u32) {
    convert::cancel(&app, job);
}

// Para cada formato de destino, el programa que falta ("libreoffice" o "calibre"), si falta alguno
#[tauri::command]
fn missing_tools(
    app: AppHandle,
    exts: Vec<String>,
    targets: Vec<String>,
) -> HashMap<String, &'static str> {
    targets
        .into_iter()
        .filter_map(|to| {
            let tool = exts
                .iter()
                .find_map(|from| convert::missing_tool(&app, from, &to))?;
            Some((to, tool))
        })
        .collect()
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .manage(convert::Jobs::default())
        .invoke_handler(tauri::generate_handler![
            file_info,
            convert_file,
            cancel_job,
            missing_tools
        ])
        .run(tauri::generate_context!())
        .expect("no se pudo arrancar Swayfi");
}
