//! Windows camera streaming detection via Media Foundation sensor activity reports.
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use windows::core::{implement, Ref};
use windows::Win32::Media::MediaFoundation::{
    IMFSensorActivitiesReport, IMFSensorActivitiesReportCallback,
    IMFSensorActivitiesReportCallback_Impl, MFCreateSensorActivityMonitor, MFStartup,
    MFSTARTUP_FULL, MF_VERSION,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

static STREAMING: AtomicBool = AtomicBool::new(false);
static MONITOR_READY: AtomicBool = AtomicBool::new(false);
static REPORT_COUNT: AtomicU32 = AtomicU32::new(0);

#[implement(IMFSensorActivitiesReportCallback)]
struct SensorCallback;

impl IMFSensorActivitiesReportCallback_Impl for SensorCallback_Impl {
    fn OnActivitiesReport(&self, report: Ref<'_, IMFSensorActivitiesReport>) -> windows::core::Result<()> {
        let Some(report) = report.as_ref() else { return Ok(()) };
        let own_pid = std::process::id();
        let mut active = false;
        unsafe {
            for i in 0..report.GetCount().unwrap_or(0) {
                let Ok(device) = report.GetActivityReport(i) else { continue };
                for j in 0..device.GetProcessCount().unwrap_or(0) {
                    let Ok(activity) = device.GetProcessActivity(j) else { continue };
                    let streaming = activity.GetStreamingState().map(|v| v.as_bool()).unwrap_or(false);
                    let pid = activity.GetProcessId().unwrap_or(0);
                    if streaming && pid != 0 && pid != own_pid { active = true; break; }
                }
                if active { break; }
            }
        }
        STREAMING.store(active, Ordering::Release);
        REPORT_COUNT.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

pub fn init() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        if MFStartup(MF_VERSION, MFSTARTUP_FULL).is_err() { return; }
        let callback: IMFSensorActivitiesReportCallback = SensorCallback.into();
        let Ok(monitor) = MFCreateSensorActivityMonitor(&callback) else { return };
        if monitor.Start().is_err() { return; }
        MONITOR_READY.store(true, Ordering::Release);
        std::mem::forget(monitor);
        std::mem::forget(callback);
    }
}

/// None until the monitor delivers its first report; caller then uses Registry fallback.
pub fn is_camera_active() -> Option<bool> {
    if MONITOR_READY.load(Ordering::Acquire) && REPORT_COUNT.load(Ordering::Relaxed) > 0 {
        Some(STREAMING.load(Ordering::Acquire))
    } else { None }
}
