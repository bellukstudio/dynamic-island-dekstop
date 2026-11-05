//! Akses file HP lewat MTP ("Transfer file") TANPA USB debugging. Hanya Windows.
//!
//! Memakai API WinRT `Windows.Devices.Portable.StorageDevice` + `StorageFolder`, jadi tidak perlu
//! menulis kode COM WPD. Perangkat tampil dengan serial `mtp:<id perangkat>`.
//!
//! Struktur MTP: perangkat -> penyimpanan ("Internal shared storage", kartu SD) -> folder.
//! Untuk menjaga antarmuka yang sama dengan adb, awalan `/sdcard` dipetakan ke penyimpanan
//! internal. Jalur lain berupa `/<nama penyimpanan>/folder/...`.
//!
//! Catatan kinerja: MTP lambat. Ukuran file hanya dibaca untuk `SIZE_LIMIT` file pertama per folder;
//! sisanya ditampilkan 0 B agar daftar folder berisi ribuan foto tidak membeku.
//! Mirror layar tidak tersedia lewat MTP (butuh adb).

use std::path::{Path, PathBuf};

use windows::core::{Interface, HSTRING};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Devices::Portable::StorageDevice;
use windows::Foundation::DateTime;
use windows::Storage::{
    CreationCollisionOption, IStorageItem, NameCollisionOption, StorageFile, StorageFolder,
    StorageItemTypes,
};
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

use super::adb::{Device, Entry};

const SIZE_LIMIT: usize = 150;
const HINT: &str = "HP tidak bisa dibaca: buka kunci layar HP dan pilih mode USB \"Transfer file\".";

fn e<T: std::fmt::Display>(x: T) -> String {
    x.to_string()
}

pub fn is_mtp(serial: &str) -> bool {
    serial.starts_with("mtp:")
}

fn device_id(serial: &str) -> Result<&str, String> {
    serial.strip_prefix("mtp:").filter(|s| !s.is_empty()).ok_or_else(|| "serial MTP tidak valid".to_string())
}

pub fn devices() -> Result<Vec<Device>, String> {
    let selector = StorageDevice::GetDeviceSelector().map_err(e)?;
    let list = DeviceInformation::FindAllAsyncAqsFilter(&selector).map_err(e)?.get().map_err(e)?;
    let mut out = Vec::new();
    for info in list {
        let id = info.Id().map_err(e)?.to_string();
        let name = info.Name().map(|n| n.to_string()).unwrap_or_default();
        out.push(Device {
            serial: format!("mtp:{id}"),
            state: "device".into(),
            model: if name.is_empty() { "Perangkat MTP".into() } else { name },
            wifi: false,
            mtp: true,
        });
    }
    Ok(out)
}

fn segments(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

fn primary_storage(root: &StorageFolder) -> Result<StorageFolder, String> {
    let storages = root
        .GetFoldersAsyncOverloadDefaultOptionsStartAndCount()
        .map_err(e)?
        .get()
        .map_err(|_| HINT.to_string())?;
    let mut first = None;
    for s in storages {
        let name = s.Name().map(|n| n.to_string().to_lowercase()).unwrap_or_default();
        if name.contains("internal") || name.contains("internal shared") || name.contains("internal storage") {
            return Ok(s);
        }
        first.get_or_insert(s);
    }
    first.ok_or_else(|| HINT.to_string())
}

fn open_root(serial: &str) -> Result<StorageFolder, String> {
    let id = HSTRING::from(device_id(serial)?);
    StorageDevice::FromId(&id).map_err(|_| HINT.to_string())
}

fn resolve_segments(root: StorageFolder, segs: &[&str]) -> Result<StorageFolder, String> {
    let mut cur = root;
    for (i, seg) in segs.iter().enumerate() {
        cur = if i == 0 && seg.eq_ignore_ascii_case("sdcard") {
            primary_storage(&cur)?
        } else {
            cur.GetFolderAsync(&HSTRING::from(*seg))
                .map_err(e)?
                .get()
                .map_err(|_| format!("Folder \"{seg}\" tidak ditemukan di HP"))?
        };
    }
    Ok(cur)
}

fn resolve_folder(serial: &str, path: &str) -> Result<StorageFolder, String> {
    resolve_segments(open_root(serial)?, &segments(path))
}

fn format_date(d: DateTime) -> String {
    // DateTime.UniversalTime = tick 100 ns sejak 1601-01-01 UTC, sama dengan FILETIME.
    let t = d.UniversalTime as u64;
    let ft = FILETIME { dwLowDateTime: t as u32, dwHighDateTime: (t >> 32) as u32 };
    let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    unsafe {
        if FileTimeToSystemTime(&ft, &mut utc).is_err() {
            return String::new();
        }
        if SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_err() {
            local = utc;
        }
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute
    )
}

pub fn list_dir(serial: &str, path: &str) -> Result<Vec<Entry>, String> {
    let folder = resolve_folder(serial, path)?;
    let items = folder
        .GetItemsAsyncOverloadDefaultStartAndCount()
        .map_err(e)?
        .get()
        .map_err(|_| HINT.to_string())?;
    let mut out = Vec::new();
    let mut sized = 0usize;
    for item in items {
        let name = item.Name().map_err(e)?.to_string();
        if name.starts_with('.') {
            continue;
        }
        let is_dir = item.IsOfType(StorageItemTypes::Folder).unwrap_or(false);
        let date = item.DateCreated().map(format_date).unwrap_or_default();
        let mut size = 0u64;
        if !is_dir && sized < SIZE_LIMIT {
            sized += 1;
            if let Ok(file) = item.cast::<StorageFile>() {
                size = file
                    .GetBasicPropertiesAsync()
                    .and_then(|o| o.get())
                    .and_then(|p| p.Size())
                    .unwrap_or(0);
            }
        }
        out.push(Entry { name, is_dir, size, date });
    }
    out.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

fn local_folder(path: &Path) -> Result<StorageFolder, String> {
    StorageFolder::GetFolderFromPathAsync(&HSTRING::from(path.to_string_lossy().as_ref()))
        .map_err(e)?
        .get()
        .map_err(|err| format!("Folder lokal tidak bisa dibuka: {err}"))
}

fn copy_file_to(file: &StorageFile, dest: &StorageFolder) -> Result<StorageFile, String> {
    let name = file.Name().map_err(e)?;
    file.CopyOverload(dest, &name, NameCollisionOption::GenerateUniqueName)
        .map_err(e)?
        .get()
        .map_err(|err| format!("Gagal menyalin \"{name}\": {err}"))
}

fn copy_folder_to(src: &StorageFolder, dest: &StorageFolder) -> Result<StorageFolder, String> {
    let name = src.Name().map_err(e)?;
    let made = dest
        .CreateFolderAsync(&name, CreationCollisionOption::GenerateUniqueName)
        .map_err(e)?
        .get()
        .map_err(|err| format!("Folder \"{name}\" tidak bisa dibuat: {err}"))?;
    let items = src
        .GetItemsAsyncOverloadDefaultStartAndCount()
        .map_err(e)?
        .get()
        .map_err(e)?;
    for item in items {
        if item.IsOfType(StorageItemTypes::Folder).unwrap_or(false) {
            copy_folder_to(&item.cast::<StorageFolder>().map_err(e)?, &made)?;
        } else {
            copy_file_to(&item.cast::<StorageFile>().map_err(e)?, &made)?;
        }
    }
    Ok(made)
}

/// Ambil file/folder dari HP ke `dest_dir` di PC. Mengembalikan jalur lokal hasilnya.
pub fn pull(serial: &str, remote: &str, dest_dir: &Path) -> Result<PathBuf, String> {
    let segs = segments(remote);
    let (name, parent) = segs.split_last().ok_or("Jalur HP kosong")?;
    let parent = resolve_segments(open_root(serial)?, parent)?;
    let item: IStorageItem = parent
        .GetItemAsync(&HSTRING::from(*name))
        .map_err(e)?
        .get()
        .map_err(|_| format!("\"{name}\" tidak ditemukan di HP"))?;
    let dest = local_folder(dest_dir)?;
    let result_path = if item.IsOfType(StorageItemTypes::Folder).unwrap_or(false) {
        copy_folder_to(&item.cast::<StorageFolder>().map_err(e)?, &dest)?.Path().map_err(e)?
    } else {
        copy_file_to(&item.cast::<StorageFile>().map_err(e)?, &dest)?.Path().map_err(e)?
    };
    Ok(PathBuf::from(result_path.to_string()))
}

/// Kirim file/folder dari PC ke folder `remote_dir` di HP.
pub fn push(serial: &str, local: &Path, remote_dir: &str) -> Result<(), String> {
    let dest = resolve_folder(serial, remote_dir)?;
    if local.is_dir() {
        push_dir(local, &dest)
    } else {
        let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(local.to_string_lossy().as_ref()))
            .map_err(e)?
            .get()
            .map_err(|err| format!("File lokal tidak bisa dibuka: {err}"))?;
        copy_file_to(&file, &dest).map(|_| ())
    }
}

fn push_dir(local: &Path, dest: &StorageFolder) -> Result<(), String> {
    let name = local.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "folder".into());
    let made = dest
        .CreateFolderAsync(&HSTRING::from(name.as_str()), CreationCollisionOption::GenerateUniqueName)
        .map_err(e)?
        .get()
        .map_err(|err| format!("Folder \"{name}\" tidak bisa dibuat di HP: {err}"))?;
    for entry in std::fs::read_dir(local).map_err(e)? {
        let path = entry.map_err(e)?.path();
        if path.is_dir() {
            push_dir(&path, &made)?;
        } else {
            let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.to_string_lossy().as_ref()))
                .map_err(e)?
                .get()
                .map_err(e)?;
            copy_file_to(&file, &made)?;
        }
    }
    Ok(())
}