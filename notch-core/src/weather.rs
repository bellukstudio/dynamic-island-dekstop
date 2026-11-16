//! Pengambilan cuaca Open-Meteo untuk UI desktop native.
//! Fungsi fetch memblokir; jalankan dari thread latar, bukan thread UI.

#[derive(Clone, Debug)]
pub struct Reading {
    pub temp: f64,
    pub feels: f64,
    pub humidity: f64,
    pub code: u32,
    pub is_day: bool,
    pub raining: bool,
    pub rain_in: Option<u32>,
}

#[derive(serde::Deserialize)]
struct Response {
    current: Current,
    #[serde(default)]
    minutely_15: Option<Minutely>,
}

#[derive(serde::Deserialize)]
struct Current {
    temperature_2m: f64,
    apparent_temperature: f64,
    relative_humidity_2m: f64,
    weather_code: u32,
    is_day: u8,
}

#[derive(serde::Deserialize)]
struct Minutely {
    precipitation: Vec<Option<f64>>,
}

/// Unduh kondisi saat ini dan perkiraan hujan per 15 menit untuk koordinat yang diberikan.
pub fn fetch(lat: f64, lon: f64) -> Result<Reading, String> {
    if !lat.is_finite() || !lon.is_finite() || !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Err("Koordinat cuaca tidak valid".into());
    }

    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &current=temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,is_day\
         &minutely_15=precipitation&forecast_minutely_15=8&timezone=auto"
    );
    let body = crate::http::get_text(&url, 15)?;
    let response: Response = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let current = response.current;

    let raining = (51..=67).contains(&current.weather_code)
        || (80..=82).contains(&current.weather_code)
        || current.weather_code >= 95;

    let rain_in = if raining {
        None
    } else {
        response.minutely_15.and_then(|minutes| {
            minutes.precipitation
                .iter()
                .position(|amount| amount.unwrap_or(0.0) >= 0.2)
                .map(|index| (index as u32 + 1) * 15)
        })
    };

    Ok(Reading {
        temp: current.temperature_2m,
        feels: current.apparent_temperature,
        humidity: current.relative_humidity_2m,
        code: current.weather_code,
        is_day: current.is_day == 1,
        raining,
        rain_in,
    })
}

/// Ringkasan cuaca mengikuti pemetaan WMO yang sama dengan versi Tauri.
pub fn describe(code: u32, is_day: bool) -> &'static str {
    if code == 0 {
        if is_day { "Cerah" } else { "Cerah (malam)" }
    } else if code <= 2 {
        "Cerah berawan"
    } else if code == 3 {
        "Mendung"
    } else if code <= 48 {
        "Berkabut"
    } else if code <= 57 {
        "Gerimis"
    } else if code <= 67 || (80..=82).contains(&code) {
        "Hujan"
    } else if code >= 95 {
        "Badai petir"
    } else {
        "Berawan"
    }
}

/// Ikon vektor ringkas agar tetap tajam pada renderer software Slint (tanpa glyph emoji).
pub fn icon_path(code: u32, is_day: bool) -> &'static str {
    if code == 0 {
        if is_day {
            "M 12 3 A 4 4 0 1 0 12 11 A 4 4 0 1 0 12 3 M 12 1 V 0 M 12 23 V 22 M 1 12 H 0 M 24 12 H 23 M 4.2 4.2 L 3.5 3.5 M 20.5 20.5 L 19.8 19.8 M 19.8 4.2 L 20.5 3.5 M 3.5 20.5 L 4.2 19.8"
        } else {
            "M 19 16.5 A 8 8 0 1 1 10.5 5 A 6 6 0 0 0 19 16.5 Z"
        }
    } else if code <= 2 {
        "M 4 16 A 4 4 0 0 1 5 8 A 6 6 0 0 1 17 9 A 4 4 0 0 1 18 16 Z M 7 4 L 6 2 M 13 3 L 13 1"
    } else if code == 3 || code <= 48 {
        "M 3 16 A 4 4 0 0 1 4 8 A 6 6 0 0 1 16 9 A 4 4 0 0 1 18 16 Z"
    } else if code <= 67 || (80..=82).contains(&code) {
        "M 4 13 A 4 4 0 0 1 5 6 A 6 6 0 0 1 17 7 A 4 4 0 0 1 19 13 Z M 7 16 L 6 20 M 13 16 L 12 21 M 19 16 L 18 19"
    } else if code >= 95 {
        "M 13 2 L 7 13 H 12 L 10 22 L 18 10 H 13 Z"
    } else {
        "M 4 16 A 4 4 0 0 1 5 8 A 6 6 0 0 1 17 9 A 4 4 0 0 1 18 16 Z"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn description_matches_common_wmo_codes() {
        assert_eq!(describe(0, true), "Cerah");
        assert_eq!(describe(3, true), "Mendung");
        assert_eq!(describe(51, true), "Gerimis");
        assert_eq!(describe(61, true), "Hujan");
        assert_eq!(describe(95, true), "Badai petir");
    }

    #[test]
    fn icon_is_always_a_vector_path() {
        assert!(icon_path(0, true).contains("A"));
        assert!(icon_path(61, true).contains("L"));
    }
}
