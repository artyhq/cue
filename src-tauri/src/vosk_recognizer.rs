use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::{AppHandle, Manager};
use std::io::Cursor;
use reqwest::blocking::get;
use zip::read::ZipArchive;

struct VoskState {
    stop_signal: Arc<AtomicBool>,
    thread_handle: Option<thread::JoinHandle<()>>,
}

pub struct VoskManager(Mutex<VoskState>);

pub fn init(app: &AppHandle) {
    app.manage(VoskManager(Mutex::new(VoskState {
        stop_signal: Arc::new(AtomicBool::new(false)),
        thread_handle: None,
    })));
}

pub fn update(app: &AppHandle) {
    let settings = crate::settings::current(app);
    if settings.wake_on_voice {
        start(app);
    } else {
        stop(app);
    }
}

fn start(app: &AppHandle) {
    let manager = app.state::<VoskManager>();
    let mut state = manager.0.lock().unwrap();
    if state.thread_handle.is_some() {
        return; // Already running
    }
    
    let stop_signal = Arc::new(AtomicBool::new(false));
    state.stop_signal = stop_signal.clone();
    
    let app_handle = app.clone();
    
    state.thread_handle = Some(thread::spawn(move || {
        if let Err(e) = run_vosk(app_handle, stop_signal) {
            eprintln!("Vosk error: {}", e);
        }
    }));
}

fn stop(app: &AppHandle) {
    let manager = app.state::<VoskManager>();
    let mut state = manager.0.lock().unwrap();
    if let Some(handle) = state.thread_handle.take() {
        state.stop_signal.store(true, Ordering::SeqCst);
        let _ = handle.join();
    }
}

fn download_model() -> Result<std::path::PathBuf, String> {
    let app_dir = directories::ProjectDirs::from("app", "Cue", "cue")
        .ok_or("No config directory")?;
    let data_dir = app_dir.data_dir();
    std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    
    let model_dir = data_dir.join("vosk-model-small-en-us-0.15");
    if model_dir.exists() {
        return Ok(model_dir);
    }
    
    eprintln!("Downloading Vosk model (50MB)...");
    let url = "https://alphacephei.com/vosk/models/vosk-model-small-en-us-0.15.zip";
    let response = get(url).map_err(|e| e.to_string())?;
    let bytes = response.bytes().map_err(|e| e.to_string())?;
    
    let reader = Cursor::new(bytes);
    let mut archive = ZipArchive::new(reader).map_err(|e| e.to_string())?;
    
    eprintln!("Extracting Vosk model...");
    archive.extract(data_dir).map_err(|e| e.to_string())?;
    
    Ok(model_dir)
}

struct MonoResampler {
    from_hz: u32,
    to_hz: u32,
    phase: u32,
}

impl MonoResampler {
    fn new(from_hz: u32, to_hz: u32) -> Self {
        Self { from_hz, to_hz, phase: 0 }
    }
    fn convert(&mut self, data: &[f32], channels: usize) -> Vec<i16> {
        let mut out = Vec::with_capacity(data.len() / channels + 1);
        for chunk in data.chunks(channels) {
            self.phase += self.to_hz;
            if self.phase >= self.from_hz {
                self.phase -= self.from_hz;
                let sum: f32 = chunk.iter().sum();
                let avg = sum / (channels as f32);
                let clamped = avg.clamp(-1.0, 1.0);
                out.push((clamped * i16::MAX as f32) as i16);
            }
        }
        out
    }
}

fn build_capture_stream(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    mut on_data: impl FnMut(&[f32]) + Send + 'static,
) -> Result<cpal::Stream, String> {
    fn convert<T: cpal::Sample>(data: &[T]) -> Vec<f32>
    where
        f32: cpal::FromSample<T>,
    {
        data.iter().map(|&s| s.to_sample::<f32>()).collect()
    }
    
    let err_fn = |err| {
        eprintln!("vosk stream error: {}", err);
    };
    
    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            config.clone(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| on_data(data),
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            config.clone(),
            move |data: &[i16], _: &cpal::InputCallbackInfo| on_data(&convert(data)),
            err_fn,
            None,
        ),
        cpal::SampleFormat::I32 => device.build_input_stream(
            config.clone(),
            move |data: &[i32], _: &cpal::InputCallbackInfo| on_data(&convert(data)),
            err_fn,
            None,
        ),
        _ => return Err(format!("Unsupported sample format: {}", sample_format)),
    }.map_err(|e| e.to_string())?;
    
    Ok(stream)
}

fn run_vosk(app: AppHandle, stop: Arc<AtomicBool>) -> Result<(), String> {
    let model_path = download_model()?;
    let model = vosk::Model::new(model_path.to_string_lossy())
        .ok_or("Could not load Vosk model")?;
        
    let mut recognizer = vosk::Recognizer::new(&model, 16000.0)
        .ok_or("Could not create Recognizer")?;
        
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("No input device for Vosk")?;
    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    
    let config = supported.config();
    let sample_format = supported.sample_format();
    let channels = config.channels as usize;
    let rate = config.sample_rate;
    
    let (tx, rx) = crossbeam_channel::bounded::<Vec<i16>>(100);
    let mut resampler = MonoResampler::new(rate, 16000);
    
    let stream = build_capture_stream(&device, config, sample_format, move |data| {
        let _ = tx.try_send(resampler.convert(data, channels));
    })?;
    
    stream.play().map_err(|e| e.to_string())?;
    
    while !stop.load(Ordering::Relaxed) {
        if let Ok(data) = rx.recv_timeout(std::time::Duration::from_millis(100)) {
            // flatten since `data` is `Vec<Vec<i16>>` from the resampler ...
            // wait, `convert` returns `Vec<i16>`, so `data` is `Vec<i16>`.
            let state = recognizer.accept_waveform(&data);
            
            // Check partial results for faster response
            let partial = recognizer.partial_result();
            let text = partial.partial;
            let mut triggered = false;
            
            if !text.is_empty() {
                // eprintln!("vosk partial: {}", text);
            }
            
            if text.contains("cue start recording") || text.contains("queue start recording") || text.contains("q start recording") {
                if !crate::audio::is_recording(&app) {
                    eprintln!("vosk: triggered start!");
                    crate::set_arming(&app, true);
                    crate::show_overlay(&app, true);
                    triggered = true;
                }
            } else if text.contains("cue stop recording") || text.contains("queue stop recording") || text.contains("q stop recording") {
                if crate::audio::is_recording(&app) {
                    eprintln!("vosk: triggered stop!");
                    crate::audio::stop_recording(app.clone(), false, true);
                    triggered = true;
                }
            }
            
            if triggered {
                recognizer.reset();
            } else if matches!(state, Ok(vosk::DecodingState::Finalized)) {
                // Also check full result just in case
                if let Some(res) = recognizer.result().single() {
                    let text = res.text;
                    eprintln!("vosk heard: {}", text);
                    if text.contains("cue start recording") || text.contains("queue start recording") || text.contains("q start recording") {
                        if !crate::audio::is_recording(&app) {
                            eprintln!("vosk: triggered start!");
                            crate::set_arming(&app, true);
                            crate::show_overlay(&app, true);
                            recognizer.reset();
                        }
                    } else if text.contains("cue stop recording") || text.contains("queue stop recording") || text.contains("q stop recording") {
                        if crate::audio::is_recording(&app) {
                            eprintln!("vosk: triggered stop!");
                            crate::audio::stop_recording(app.clone(), false, true);
                            recognizer.reset();
                        }
                    }
                }
            }
        }
    }
    
    Ok(())
}
