use serde::Deserialize;

#[derive(Deserialize)]
pub struct OpenSkyResponse {
    pub states: Option<Vec<Vec<serde_json::Value>>>,
}

pub async fn fetch_aircraft() -> Vec<(f32, f32)> {
    let url = "https://opensky-network.org/api/states/all";

    let resp = match reqwest::get(url).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Network error fetching aircraft: {e}");
            return Vec::new();
        }
    };

    let body: OpenSkyResponse = match resp.json().await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("JSON parse error: {e}");
            return Vec::new();
        }
    };

    let mut aircraft = Vec::new();
    if let Some(states) = body.states {
        for s in &states {
            if s.len() > 6
                && let (Some(lon), Some(lat)) = (s[5].as_f64(), s[6].as_f64())
            {
                aircraft.push((lat as f32, lon as f32));
            }
        }
    }
    aircraft
}
