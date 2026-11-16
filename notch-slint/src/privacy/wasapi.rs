//! Active microphone capture-session detection through Windows WASAPI.
use windows::Win32::Media::Audio::{
    eCapture, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2,
    IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};

pub fn init() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

/// None means the Windows audio-session enumeration failed; caller can use Registry fallback.
pub fn is_microphone_active() -> Option<bool> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let devices = enumerator
            .EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)
            .ok()?;
        let device_count = devices.GetCount().ok()?;
        let mut inspected = 0u32;
        let own_pid = std::process::id();

        for index in 0..device_count {
            let Ok(device) = devices.Item(index) else { continue };
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                continue;
            };
            let Ok(sessions) = manager.GetSessionEnumerator() else { continue };
            inspected += 1;

            for session_index in 0..sessions.GetCount().unwrap_or(0) {
                let Ok(session) = sessions.GetSession(session_index) else { continue };
                if session.GetState().ok() != Some(AudioSessionStateActive) {
                    continue;
                }
                let Ok(session2) = session.cast::<IAudioSessionControl2>() else { continue };
                if let Ok(pid) = session2.GetProcessId() {
                    if pid != 0 && pid != own_pid {
                        return Some(true);
                    }
                }
            }
        }

        if device_count > 0 && inspected == 0 {
            None
        } else {
            Some(false)
        }
    }
}
