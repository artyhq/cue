use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let vosk_dir = PathBuf::from(manifest_dir).join("vosk-bin").join("vosk-win64-0.3.45");
    
    if vosk_dir.exists() {
        println!("cargo:rustc-link-search=native={}", vosk_dir.display());
        
        if let Ok(out_dir) = env::var("OUT_DIR") {
            let out_path = PathBuf::from(out_dir);
            if let Some(target_dir) = out_path.parent().and_then(|p| p.parent()).and_then(|p| p.parent()) {
                if let Ok(entries) = std::fs::read_dir(&vosk_dir) {
                    for entry in entries.flatten() {
                        if entry.path().extension().and_then(|e| e.to_str()) == Some("dll") {
                            let _ = std::fs::copy(entry.path(), target_dir.join(entry.file_name()));
                        }
                    }
                }
            }
        }
    }

    tauri_build::build()
}
