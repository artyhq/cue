use crate::dsp::{self, StereoResampler, MIX_RATE, QUEUE_CAP};
use crate::settings::{self, AudioDeviceDto, AppSettings, Channel, PlayingApp};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, DeviceId, ErrorKind, FromSample, Sample, SampleFormat, StreamConfig};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use chrono::Local;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedRecording {
    filename: String,
    path: String,
}

struct MixPacket {
    source: usize,
    samples: Vec<f32>,
}

pub struct RecorderState {
    streams: Vec<cpal::Stream>,
    workers: Vec<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    save: Arc<AtomicBool>,
    truncate: Arc<AtomicBool>,
    wait_handle: Option<JoinHandle<Option<PathBuf>>>,
}

pub struct AudioState(pub std::sync::Mutex<Option<RecorderState>>);

pub struct MonitorBundle {
    streams: Vec<cpal::Stream>,
    workers: Vec<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}

pub struct MonitorState(pub std::sync::Mutex<Option<MonitorBundle>>);

fn stream_err(err: cpal::Error) {
    if err.kind() != ErrorKind::Xrun {
        eprintln!("cue stream error: {err}");
    }
}

fn build_capture_stream(
    device: &Device,
    config: StreamConfig,
    sample_format: SampleFormat,
    mut on_data: impl FnMut(&[f32]) + Send + 'static,
) -> Result<cpal::Stream, String> {
    fn convert<T: Sample>(data: &[T]) -> Vec<f32>
    where
        f32: FromSample<T>,
    {
        data.iter().map(|&s| s.to_sample::<f32>()).collect()
    }
    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| on_data(data),
            stream_err,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| on_data(&convert(data)),
            stream_err,
            None,
        ),
        SampleFormat::I24 => device.build_input_stream(
            config,
            move |data: &[cpal::I24], _: &cpal::InputCallbackInfo| on_data(&convert(data)),
            stream_err,
            None,
        ),
        SampleFormat::I32 => device.build_input_stream(
            config,
            move |data: &[i32], _: &cpal::InputCallbackInfo| on_data(&convert(data)),
            stream_err,
            None,
        ),
        SampleFormat::U8 => device.build_input_stream(
            config,
            move |data: &[u8], _: &cpal::InputCallbackInfo| on_data(&convert(data)),
            stream_err,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            config,
            move |data: &[u16], _: &cpal::InputCallbackInfo| on_data(&convert(data)),
            stream_err,
            None,
        ),
        SampleFormat::U24 => device.build_input_stream(
            config,
            move |data: &[cpal::U24], _: &cpal::InputCallbackInfo| on_data(&convert(data)),
            stream_err,
            None,
        ),
        SampleFormat::U32 => device.build_input_stream(
            config,
            move |data: &[u32], _: &cpal::InputCallbackInfo| on_data(&convert(data)),
            stream_err,
            None,
        ),
        SampleFormat::F64 => device.build_input_stream(
            config,
            move |data: &[f64], _: &cpal::InputCallbackInfo| on_data(&convert(data)),
            stream_err,
            None,
        ),
        other => return Err(format!("Unsupported sample format: {other}")),
    }
    .map_err(|e| format!("Couldn't open audio: {e}"))?;
    Ok(stream)
}

fn device_name(device: &Device) -> String {
    device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "Unknown device".into())
}

fn device_id_string(device: &Device) -> String {
    device.id().map(|id| id.to_string()).unwrap_or_default()
}

fn find_device(id: &str, inputs: bool) -> Option<Device> {
    let host = cpal::default_host();
    if id.trim().is_empty() {
        return if inputs {
            host.default_input_device()
        } else {
            host.default_output_device()
        };
    }
    if let Ok(parsed) = DeviceId::from_str(id) {
        if let Some(device) = host.device_by_id(&parsed) {
            return Some(device);
        }
    }
    let mut iter = if inputs {
        host.input_devices().ok()?
    } else {
        host.output_devices().ok()?
    };
    iter.find(|device| device_id_string(device) == id || device_name(device) == id)
}

fn resolve_input_device(settings: &AppSettings) -> Result<Device, String> {
    find_device(&settings.input_device_id, true).ok_or_else(|| {
        if settings.input_device_id.is_empty() {
            "No microphone found".into()
        } else {
            "Selected microphone is unavailable. Pick another in Settings.".into()
        }
    })
}

fn resolve_output_device(settings: &AppSettings) -> Result<Device, String> {
    find_device(&settings.output_device_id, false)
        .ok_or_else(|| "No speakers found for computer sound.".into())
}

pub fn list_devices(kind: &str) -> Result<Vec<AudioDeviceDto>, String> {
    let host = cpal::default_host();
    let inputs = kind != "output";
    let default_id = if inputs {
        host.default_input_device().map(|d| device_id_string(&d))
    } else {
        host.default_output_device().map(|d| device_id_string(&d))
    };
    let iter = if inputs {
        host.input_devices().map_err(|e| e.to_string())?
    } else {
        host.output_devices().map_err(|e| e.to_string())?
    };
    let mut devices = Vec::new();
    for device in iter {
        let id = device_id_string(&device);
        if id.is_empty() {
            continue;
        }
        let is_default = default_id.as_ref() == Some(&id);
        devices.push(AudioDeviceDto {
            id,
            name: device_name(&device),
            is_default,
        });
    }
    Ok(devices)
}

#[tauri::command]
pub fn list_audio_devices(kind: String) -> Result<Vec<AudioDeviceDto>, String> {
    list_devices(&kind)
}

#[tauri::command]
pub fn list_playing_apps() -> Result<Vec<PlayingApp>, String> {
    #[cfg(windows)]
    {
        crate::win_capture::list_playing_apps()
    }
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}

fn start_mic_stream(
    settings: &AppSettings,
    tx: crossbeam_channel::Sender<MixPacket>,
    source: usize,
) -> Result<cpal::Stream, String> {
    let device = resolve_input_device(settings)?;
    let supported = device
        .default_input_config()
        .map_err(|e| format!("Microphone isn't available: {e}"))?;
    let sample_format = supported.sample_format();
    let config = supported.config();
    let channels = config.channels as usize;
    let rate = config.sample_rate;
    eprintln!("cue: mic '{}' {rate} Hz {channels} ch {sample_format:?}", device_name(&device));
    let mut resampler = StereoResampler::new(rate, MIX_RATE);
    let stream = build_capture_stream(&device, config, sample_format, move |data| {
        let _ = tx.try_send(MixPacket {
            source,
            samples: resampler.convert(data, channels),
        });
    })?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

fn start_system_stream(
    settings: &AppSettings,
    tx: crossbeam_channel::Sender<MixPacket>,
    source: usize,
) -> Result<cpal::Stream, String> {
    let device = resolve_output_device(settings)?;
    let supported = device
        .default_output_config()
        .map_err(|e| format!("Computer sound isn't available: {e}"))?;
    let sample_format = supported.sample_format();
    let config = supported.config();
    let channels = config.channels as usize;
    let rate = config.sample_rate;
    eprintln!(
        "cue: system loopback '{}' {rate} Hz {channels} ch {sample_format:?}",
        device_name(&device)
    );
    let mut resampler = StereoResampler::new(rate, MIX_RATE);
    let stream = build_capture_stream(&device, config, sample_format, move |data| {
        let _ = tx.try_send(MixPacket {
            source,
            samples: resampler.convert(data, channels),
        });
    })?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

fn start_channel(
    channel: &Channel,
    settings: &AppSettings,
    tx: crossbeam_channel::Sender<MixPacket>,
    source: usize,
    stop: &Arc<AtomicBool>,
    streams: &mut Vec<cpal::Stream>,
    workers: &mut Vec<JoinHandle<()>>,
) -> Result<(), String> {
    match channel.kind.as_str() {
        "mic" => streams.push(start_mic_stream(settings, tx, source)?),
        "system" => streams.push(start_system_stream(settings, tx, source)?),
        "app" => {
            #[cfg(windows)]
            {
                let (local_tx, local_rx) = crossbeam_channel::unbounded::<Vec<f32>>();
                workers.push(crate::win_capture::start_app_capture(
                    channel.exe.clone(),
                    local_tx,
                    stop.clone(),
                )?);
                workers.push(thread::spawn(move || {
                    while let Ok(samples) = local_rx.recv() {
                        let _ = tx.try_send(MixPacket { source, samples });
                    }
                }));
            }
            #[cfg(not(windows))]
            {
                let _ = (tx, stop, source);
                return Err("App capture is only available on Windows.".into());
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn start_recording(app: AppHandle) -> Result<(), String> {
    {
        let state = app.state::<AudioState>();
        if state.0.lock().unwrap().is_some() {
            return Ok(());
        }
    }
    stop_monitor(&app);

    let settings = settings::current(&app);
    let preset = settings::active_preset(&settings);
    let enabled: Vec<Channel> = preset
        .channels
        .iter()
        .filter(|c| c.enabled)
        .cloned()
        .collect();
    if enabled.is_empty() {
        return Err("Turn on at least one source in Settings.".into());
    }

    let out_dir = settings::resolved_output_dir(&settings)?;
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let slug = settings::preset_slug(&preset);
    let filename = format!("{}-{}.wav", Local::now().format("%Y-%m-%d-%H%M%S"), slug);
    let filepath = out_dir.join(&filename);

    let (tx, rx) = crossbeam_channel::unbounded::<MixPacket>();
    let save = Arc::new(AtomicBool::new(true));
    let stop = Arc::new(AtomicBool::new(false));
    let truncate = Arc::new(AtomicBool::new(false));
    let mut streams = Vec::new();
    let mut workers = Vec::new();
    let mut started = Vec::new();
    let mut errors = Vec::new();
    let mut source_count = 0usize;

    for channel in &enabled {
        let source = source_count;
        match start_channel(
            channel,
            &settings,
            tx.clone(),
            source,
            &stop,
            &mut streams,
            &mut workers,
        ) {
            Ok(()) => {
                source_count += 1;
                started.push(channel.name.clone());
            }
            Err(e) => {
                eprintln!("cue: skip '{}': {e}", channel.name);
                errors.push(e);
            }
        }
    }
    drop(tx);

    if streams.is_empty() && workers.is_empty() {
        return Err(errors
            .into_iter()
            .next()
            .unwrap_or_else(|| "Couldn't start any audio source.".into()));
    }

    let save_writer = save.clone();
    let truncate_writer = truncate.clone();
    let filepath_clone = filepath.clone();
    let app_clone = app.clone();
    let writer_handle = thread::spawn(move || {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: MIX_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = match hound::WavWriter::create(&filepath_clone, spec) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("cue: failed to create wav: {e}");
                return None;
            }
        };

        let mut queues: Vec<VecDeque<f32>> =
            (0..source_count).map(|_| VecDeque::new()).collect();
        let mut sample_count = 0usize;
        let emit_every = (MIX_RATE as usize) / 30;
        let mut max_amp = 0.0f32;
        let mut levels: VecDeque<f32> = std::iter::repeat(0.0).take(48).collect();
        let mut disconnected = false;
        let mut primed = source_count <= 1;
        let mut last_mix = Instant::now();
        let startup_wait = Duration::from_millis(200);
        let underrun_wait = Duration::from_millis(40);
        let pad_block = (MIX_RATE as usize / 125) * 2; // 8ms of stereo

        let push_packet = |packet: MixPacket, queues: &mut [VecDeque<f32>]| {
            if let Some(queue) = queues.get_mut(packet.source) {
                dsp::enqueue(queue, &packet.samples, QUEUE_CAP);
            }
        };

        loop {
            match rx.recv_timeout(Duration::from_millis(8)) {
                Ok(packet) => push_packet(packet, &mut queues),
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => disconnected = true,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            }
            while let Ok(packet) = rx.try_recv() {
                push_packet(packet, &mut queues);
            }

            if !primed {
                let all_ready = queues.iter().all(|q| q.len() >= 2);
                if all_ready || disconnected || last_mix.elapsed() >= startup_wait {
                    primed = true;
                    last_mix = Instant::now();
                } else {
                    continue;
                }
            }

            loop {
                let mut mixed = dsp::mix_aligned(&mut queues);
                if mixed.is_empty() {
                    let some_ready = queues.iter().any(|q| q.len() >= 2);
                    if disconnected && some_ready {
                        let target = queues.iter().map(|q| q.len()).max().unwrap_or(0);
                        dsp::pad_to(&mut queues, target);
                        mixed = dsp::mix_aligned(&mut queues);
                    } else if some_ready && last_mix.elapsed() >= underrun_wait {
                        let longest = queues.iter().map(|q| q.len()).max().unwrap_or(0);
                        dsp::pad_to(&mut queues, longest.min(pad_block).max(2));
                        mixed = dsp::mix_aligned(&mut queues);
                    } else {
                        break;
                    }
                }
                if mixed.is_empty() {
                    break;
                }
                last_mix = Instant::now();
                for (i, sample) in mixed.iter().copied().enumerate() {
                    let _ = writer.write_sample((sample * i16::MAX as f32) as i16);
                    max_amp = max_amp.max(sample.abs());
                    if i % 2 == 1 {
                        sample_count += 1;
                        if sample_count >= emit_every {
                            levels.pop_front();
                            levels.push_back(max_amp);
                            let _ = app_clone
                                .emit("cue://levels", levels.iter().copied().collect::<Vec<_>>());
                            max_amp = 0.0;
                            sample_count = 0;
                        }
                    }
                }
            }

            if disconnected {
                break;
            }
        }

        let _ = writer.finalize();
        if save_writer.load(Ordering::Acquire) {
            if truncate_writer.load(Ordering::Acquire) {
                if let Ok(mut file) = std::fs::OpenOptions::new().write(true).read(true).open(&filepath_clone) {
                    use std::io::{Seek, SeekFrom};
                    use byteorder::{LittleEndian, WriteBytesExt};
                    if let Ok(file_size) = file.metadata().map(|m| m.len()) {
                        let bytes_to_remove = 480_000;
                        if file_size > bytes_to_remove + 44 {
                            let new_size = file_size - bytes_to_remove;
                            let _ = file.set_len(new_size);
                            let _ = file.seek(SeekFrom::Start(4));
                            let _ = file.write_u32::<LittleEndian>((new_size - 8) as u32);
                            let _ = file.seek(SeekFrom::Start(40));
                            let _ = file.write_u32::<LittleEndian>((new_size - 44) as u32);
                        }
                    }
                }
            }
            Some(filepath_clone)
        } else {
            let _ = std::fs::remove_file(&filepath_clone);
            None
        }
    });

    eprintln!(
        "cue: recording preset '{}' [{}] -> {}",
        preset.name,
        started.join(", "),
        filepath.display()
    );

    *app.state::<AudioState>().0.lock().unwrap() = Some(RecorderState {
        streams,
        workers,
        stop,
        save,
        truncate,
        wait_handle: Some(writer_handle),
    });
    crate::tray::set_state(&app, crate::tray::TrayState::Recording);
    let _ = app.emit("cue://recording-started", preset.name);
    Ok(())
}

pub fn stop_recording(app: AppHandle, cancel: bool, stopped_via_voice: bool) {
    let recorder = {
        let state = app.state::<AudioState>();
        let mut guard = state.0.lock().unwrap();
        guard.take()
    };
    let Some(mut recorder) = recorder else {
        if cancel {
            hide_overlay(&app);
            let _ = app.emit("cue://cancelled", ());
        }
        return;
    };
    crate::tray::set_state(&app, crate::tray::TrayState::Idle);
    recorder.save.store(!cancel, Ordering::Release);
    recorder.stop.store(true, Ordering::Release);
    recorder.truncate.store(stopped_via_voice, Ordering::Release);
    let app_for_join = app.clone();
    thread::spawn(move || {
        recorder.streams.clear();
        for worker in recorder.workers {
            let _ = worker.join();
        }
        let saved = recorder
            .wait_handle
            .take()
            .and_then(|handle| handle.join().ok())
            .flatten();
        if cancel {
            hide_overlay(&app_for_join);
            let _ = app_for_join.emit("cue://cancelled", ());
            return;
        }
        if let Some(path) = saved {
            let filename = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            eprintln!("cue: saved {filename}");
            let _ = app_for_join.emit(
                "cue://stopped",
                SavedRecording {
                    filename,
                    path: path.to_string_lossy().to_string(),
                },
            );
            crate::focus_overlay(&app_for_join);
        } else {
            crate::tray::set_state(&app_for_join, crate::tray::TrayState::Error);
            crate::show_overlay(&app_for_join, false);
            let _ = app_for_join.emit(
                "cue://error",
                "Couldn't save the recording.".to_string(),
            );
            crate::focus_overlay(&app_for_join);
        }
    });
}

pub fn is_recording(app: &AppHandle) -> bool {
    app.state::<AudioState>()
        .0
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(false)
}

pub fn stop_monitor(app: &AppHandle) {
    if let Ok(mut guard) = app.state::<MonitorState>().0.lock() {
        if let Some(mut bundle) = guard.take() {
            bundle.stop.store(true, Ordering::Release);
            bundle.streams.clear();
            for worker in bundle.workers {
                let _ = worker.join();
            }
        }
    }
}

#[tauri::command]
pub fn start_input_monitor(app: AppHandle) -> Result<(), String> {
    if is_recording(&app) {
        return Ok(());
    }
    stop_monitor(&app);
    let settings = settings::current(&app);
    let preset = settings::active_preset(&settings);
    let (tx, rx) = crossbeam_channel::unbounded::<MixPacket>();
    let stop = Arc::new(AtomicBool::new(false));
    let mut streams = Vec::new();
    let mut workers = Vec::new();
    let mut source = 0usize;
    for channel in preset.channels.iter().filter(|c| c.enabled) {
        if start_channel(
            channel,
            &settings,
            tx.clone(),
            source,
            &stop,
            &mut streams,
            &mut workers,
        )
        .is_ok()
        {
            source += 1;
        }
    }
    drop(tx);
    let app_clone = app.clone();
    let stop_thread = stop.clone();
    workers.push(thread::spawn(move || {
        while !stop_thread.load(Ordering::Relaxed) {
            match rx.recv_timeout(std::time::Duration::from_millis(80)) {
                Ok(packet) => {
                    let peak = packet.samples.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
                    let _ = app_clone.emit("cue://monitor-level", peak);
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            }
        }
    }));
    *app.state::<MonitorState>().0.lock().unwrap() = Some(MonitorBundle {
        streams,
        workers,
        stop,
    });
    Ok(())
}

#[tauri::command]
pub fn stop_input_monitor(app: AppHandle) {
    stop_monitor(&app);
}

fn hide_overlay(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.hide();
        let _ = window.emit("overlay-hidden", ());
    }
}
