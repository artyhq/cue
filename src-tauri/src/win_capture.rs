#![cfg(windows)]

use crate::dsp::{StereoResampler, MIX_RATE};
use crate::settings::PlayingApp;
use std::collections::BTreeMap;
use std::mem::{size_of, ManuallyDrop};
use std::ops::Deref;
use std::pin::Pin;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use windows::core::{implement, Interface, HRESULT, IUnknown, PWSTR};
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    eConsole, eRender, ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
    IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
    IAudioCaptureClient, IAudioClient, IAudioSessionControl2, IAudioSessionManager2,
    IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT,
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
    AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
    VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0};
use windows::Win32::System::Com::BLOB;
use windows::Win32::System::Threading::{
    CreateEventA, OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::System::Variant::VT_BLOB;

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct ActivateHandler(Arc<(Mutex<bool>, Condvar)>);

impl IActivateAudioInterfaceCompletionHandler_Impl for ActivateHandler_Impl {
    fn ActivateCompleted(
        &self,
        _activateoperation: windows::core::Ref<IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let (lock, cvar) = &*self.0;
        let mut completed = lock.lock().unwrap();
        *completed = true;
        cvar.notify_one();
        Ok(())
    }
}

fn exe_name_from_pid(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            Default::default(),
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        path.rsplit(['\\', '/']).next().map(|s| s.to_string())
    }
}

fn friendly_app_name(exe: &str) -> String {
    exe.trim_end_matches(".exe")
        .trim_end_matches(".EXE")
        .replace(['-', '_'], " ")
}

pub fn list_playing_apps() -> Result<Vec<PlayingApp>, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| e.to_string())?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|e| e.to_string())?;
        let manager: IAudioSessionManager2 = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| e.to_string())?;
        let sessions = manager
            .GetSessionEnumerator()
            .map_err(|e| e.to_string())?;
        let count = sessions.GetCount().map_err(|e| e.to_string())?;
        let self_pid = std::process::id();
        let mut by_exe: BTreeMap<String, String> = BTreeMap::new();
        for i in 0..count {
            let ctrl = match sessions.GetSession(i) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let ctrl2: IAudioSessionControl2 = match ctrl.cast() {
                Ok(c) => c,
                Err(_) => continue,
            };
            let pid = ctrl2.GetProcessId().unwrap_or(0);
            if pid == 0 || pid == self_pid {
                continue;
            }
            let Some(exe) = exe_name_from_pid(pid) else {
                continue;
            };
            let key = exe.to_ascii_lowercase();
            by_exe.entry(key).or_insert(exe);
        }
        Ok(by_exe
            .into_values()
            .map(|exe| PlayingApp {
                name: friendly_app_name(&exe),
                exe,
            })
            .collect())
    }
}

pub fn find_pid_for_exe(exe: &str) -> Option<u32> {
    let want = exe.to_ascii_lowercase();
    let apps_ok = (|| unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None).ok()?;
        let sessions = manager.GetSessionEnumerator().ok()?;
        let count = sessions.GetCount().ok()?;
        let self_pid = std::process::id();
        let mut found = None;
        for i in 0..count {
            let Ok(ctrl) = sessions.GetSession(i) else { continue };
            let Ok(ctrl2) = ctrl.cast::<IAudioSessionControl2>() else { continue };
            let pid = ctrl2.GetProcessId().unwrap_or(0);
            if pid == 0 || pid == self_pid {
                continue;
            }
            let Some(name) = exe_name_from_pid(pid) else { continue };
            if name.to_ascii_lowercase() == want {
                found = Some(pid);
                break;
            }
        }
        found
    })();
    apps_ok
}

fn float_stereo_format() -> WAVEFORMATEX {
    WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_IEEE_FLOAT,
        nChannels: 2,
        nSamplesPerSec: MIX_RATE,
        nAvgBytesPerSec: MIX_RATE * 8,
        nBlockAlign: 8,
        wBitsPerSample: 32,
        cbSize: 0,
    }
}

fn format_is_float(fmt: &WAVEFORMATEX) -> bool {
    match fmt.wFormatTag {
        WAVE_FORMAT_IEEE_FLOAT => true,
        WAVE_FORMAT_PCM => false,
        WAVE_FORMAT_EXTENSIBLE => unsafe {
            let ext = &*(ptr::from_ref(fmt) as *const WAVEFORMATEXTENSIBLE);
            ext.SubFormat.data1 == u32::from(WAVE_FORMAT_IEEE_FLOAT)
        },
        _ => fmt.wBitsPerSample == 32,
    }
}

fn decode_pcm(data_ptr: *const u8, frames: usize, channels: usize, bits: u16, block_align: usize, is_float: bool) -> Vec<f32> {
    let count = frames.saturating_mul(channels.max(1));
    if data_ptr.is_null() || frames == 0 {
        return vec![0.0; count];
    }
    if is_float && bits == 32 {
        let samples = unsafe { std::slice::from_raw_parts(data_ptr as *const f32, count) };
        return samples.to_vec();
    }
    if is_float && bits == 64 {
        let samples = unsafe { std::slice::from_raw_parts(data_ptr as *const f64, count) };
        return samples.iter().map(|&s| s as f32).collect();
    }
    match bits {
        16 => {
            let samples = unsafe { std::slice::from_raw_parts(data_ptr as *const i16, count) };
            samples.iter().map(|&s| s as f32 / 32768.0).collect()
        }
        32 => {
            let samples = unsafe { std::slice::from_raw_parts(data_ptr as *const i32, count) };
            samples.iter().map(|&s| s as f32 / 2147483648.0).collect()
        }
        24 => {
            let bytes_per_sample = if channels == 0 {
                3
            } else {
                (block_align / channels).max(3)
            };
            if bytes_per_sample >= 4 {
                let samples = unsafe { std::slice::from_raw_parts(data_ptr as *const i32, count) };
                samples
                    .iter()
                    .map(|&s| (s >> 8) as f32 / 8_388_608.0)
                    .collect()
            } else {
                let bytes =
                    unsafe { std::slice::from_raw_parts(data_ptr, frames * block_align.max(1)) };
                let mut out = Vec::with_capacity(count);
                for frame in 0..frames {
                    for ch in 0..channels {
                        let i = frame * block_align + ch * 3;
                        if i + 2 >= bytes.len() {
                            out.push(0.0);
                            continue;
                        }
                        let mut v = i32::from(bytes[i])
                            | (i32::from(bytes[i + 1]) << 8)
                            | (i32::from(bytes[i + 2]) << 16);
                        if v & 0x800000 != 0 {
                            v |= !0xFF_FFFF;
                        }
                        out.push(v as f32 / 8_388_608.0);
                    }
                }
                out
            }
        }
        8 => {
            let samples = unsafe { std::slice::from_raw_parts(data_ptr, count) };
            samples.iter().map(|&s| (s as f32 - 128.0) / 128.0).collect()
        }
        _ => vec![0.0; count],
    }
}

fn activate_process_client(pid: u32) -> windows::core::Result<IAudioClient> {
    unsafe {
        let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: pid,
                    ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
                },
            },
        };
        let pinned = Pin::new(&mut params);
        let raw_prop = PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_BLOB,
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: PROPVARIANT_0_0_0 {
                        blob: BLOB {
                            cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                            pBlobData: ptr::from_mut(pinned.get_mut()).cast(),
                        },
                    },
                }),
            },
        };
        let activation_prop = ManuallyDrop::new(raw_prop);
        let pinned_prop = Pin::new(activation_prop.deref());
        let activation_params = Some(ptr::from_ref(pinned_prop.get_ref()));

        let setup = Arc::new((Mutex::new(false), Condvar::new()));
        let callback: IActivateAudioInterfaceCompletionHandler =
            ActivateHandler(setup.clone()).into();
        let operation = ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            activation_params,
            &callback,
        )?;

        let (lock, cvar) = &*setup;
        let mut completed = lock.lock().unwrap();
        while !*completed {
            completed = cvar.wait(completed).unwrap();
        }
        drop(completed);

        let mut audio_client: Option<IUnknown> = None;
        let mut result = HRESULT::default();
        operation.GetActivateResult(&mut result, &mut audio_client)?;
        result.ok()?;
        audio_client.unwrap().cast()
    }
}

pub fn start_app_capture(
    exe: String,
    tx: crossbeam_channel::Sender<Vec<f32>>,
    stop: Arc<AtomicBool>,
) -> Result<JoinHandle<()>, String> {
    let pid = find_pid_for_exe(&exe)
        .ok_or_else(|| format!("{} isn't playing anything right now.", friendly_app_name(&exe)))?;

    let handle = thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let client = match activate_process_client(pid) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("cue: could not capture {exe}: {e}");
                    CoUninitialize();
                    return;
                }
            };
            let mix_ptr = match client.GetMixFormat() {
                Ok(ptr) if !ptr.is_null() => ptr,
                other => {
                    eprintln!("cue: mix format for {exe} failed: {other:?}");
                    CoUninitialize();
                    return;
                }
            };
            let mix_fmt = *mix_ptr;
            let mut channels = mix_fmt.nChannels.max(1) as usize;
            let mut rate = mix_fmt.nSamplesPerSec;
            let mut bits = mix_fmt.wBitsPerSample;
            let mut block_align = mix_fmt.nBlockAlign.max(1) as usize;
            let mut is_float = format_is_float(&*mix_ptr);
            let flags = AUDCLNT_STREAMFLAGS_LOOPBACK
                | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
            let init = client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                flags,
                200_000,
                0,
                mix_ptr,
                None,
            );
            CoTaskMemFree(Some(mix_ptr.cast()));
            if let Err(e) = init {
                let fallback = float_stereo_format();
                if let Err(e2) = client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    flags,
                    200_000,
                    0,
                    &fallback,
                    None,
                ) {
                    eprintln!("cue: could not start {exe} capture: {e} / {e2}");
                    CoUninitialize();
                    return;
                }
                channels = 2;
                rate = MIX_RATE;
                bits = 32;
                block_align = 8;
                is_float = true;
            }
            let event = match CreateEventA(None, false, false, windows::core::PCSTR::null()) {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("cue: capture event failed: {e}");
                    CoUninitialize();
                    return;
                }
            };
            if let Err(e) = client.SetEventHandle(event) {
                eprintln!("cue: set event failed: {e}");
                let _ = CloseHandle(event);
                CoUninitialize();
                return;
            }
            let capture: IAudioCaptureClient = match client.GetService() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("cue: capture client failed: {e}");
                    let _ = CloseHandle(event);
                    CoUninitialize();
                    return;
                }
            };
            if let Err(e) = client.Start() {
                eprintln!("cue: start {exe} failed: {e}");
                let _ = CloseHandle(event);
                CoUninitialize();
                return;
            }
            eprintln!(
                "cue: capturing app {exe} (pid {pid}) {rate} Hz {channels} ch {bits}-bit {}",
                if is_float { "float" } else { "pcm" }
            );
            let mut resampler = StereoResampler::new(rate, MIX_RATE);

            while !stop.load(Ordering::Relaxed) {
                let wait = WaitForSingleObject(event, 80);
                if wait != WAIT_OBJECT_0 {
                    continue;
                }
                loop {
                    let mut data_ptr = ptr::null_mut();
                    let mut frames = 0u32;
                    let mut flags = 0u32;
                    match capture.GetBuffer(
                        &mut data_ptr,
                        &mut frames,
                        &mut flags,
                        None,
                        None,
                    ) {
                        Ok(()) if frames > 0 => {
                            let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                            let decoded = if silent {
                                vec![0.0f32; frames as usize * channels]
                            } else {
                                decode_pcm(
                                    data_ptr as *const u8,
                                    frames as usize,
                                    channels,
                                    bits,
                                    block_align,
                                    is_float,
                                )
                            };
                            let samples = resampler.convert(&decoded, channels);
                            if !samples.is_empty() {
                                let _ = tx.try_send(samples);
                            }
                            let _ = capture.ReleaseBuffer(frames);
                        }
                        Ok(()) => {
                            let _ = capture.ReleaseBuffer(frames);
                            break;
                        }
                        Err(_) => break,
                    }
                }
            }
            let _ = client.Stop();
            let _ = CloseHandle(event);
            CoUninitialize();
        }
    });
    Ok(handle)
}


