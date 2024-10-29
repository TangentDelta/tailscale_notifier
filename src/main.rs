use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct Device {
    hostname: String,

    #[serde(with = "date_format")]
    expires: chrono::DateTime<chrono::Utc>
}

#[derive(Debug, Deserialize)]
struct Devices {
    devices: Vec<Device>
}

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    tailnet_name: String,
    tailscale_token: String,
    pushover_token: String,
    pushover_user_key: String
}

impl ::std::default::Default for Config {
    fn default() -> Self {
        Self {
            tailnet_name: String::new(),
            tailscale_token: String::new(),
            pushover_token: String::new(),
            pushover_user_key: String::new()
        }
    }
}

mod date_format {
    use chrono::DateTime;
    use serde::{self, Deserialize, Deserializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<chrono::Utc>, D::Error>
    where 
        D: Deserializer<'de>
    {
        let s = String::deserialize(deserializer)?;
        let dt = DateTime::parse_from_rfc3339(&s).map_err(serde::de::Error::custom)?.with_timezone(&chrono::Utc);
        Ok(dt)
    }
}

fn send_message(msg: &str, token: &str, user_key: &str){
    use pushover::{ API, requests::message::SendMessage };
    let api = API::new();
    let msg_send = SendMessage::new(token, user_key, msg);
    let response = api.send(&msg_send);
    println!("{:?}", response.expect("Error sending message"));
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    use reqwest::{Client, IntoUrl, Response};

    let cfg: Config = confy::load("tailscale_notifier", None)?;

    let file = confy::get_configuration_file_path("tailscale_notifier", None)?;
    println!("Loading config from path: {:#?}", file);

    async fn get<T: IntoUrl + Clone>(url: T, key: &str) -> reqwest::Result<Response> {
        let header_value = format!("Bearer {}", key);
        Client::builder()
            .build()?
            .get(url)
            .header("Authorization", header_value)
            .send()
            .await
    }

    // Get the list of devices from Tailscale and deserialize the JSON into a struct
    let url = format!("https://api.tailscale.com/api/v2/tailnet/{}/devices", &cfg.tailnet_name);

    eprintln!("Fetching {url:?}...");

    let res = get(url, &cfg.tailscale_token).await?;

    let req_body = res.text().await?;
    let devices: Vec<Device> = serde_json::from_str::<Devices>(&req_body)?.devices;


    // Determine which devices are expiring within 15 days or have already expired
    let utc: chrono::DateTime<chrono::Utc> = chrono::Utc::now();
    let mut devices_expiring: Vec<&Device> = Vec::new();
    let mut devices_expired: Vec<&Device> = Vec::new();

    use std::cmp::Ordering;

    for device in devices.iter(){
        let days_until_expiration = (device.expires - utc).num_days();

        if days_until_expiration < 15 {
            match days_until_expiration.cmp(&(0_i64)) {
                Ordering::Greater => {
                    println!("{} expires in {} days", device.hostname, days_until_expiration);
                    devices_expiring.push(device);
                },
                Ordering::Less => {
                    println!("{} expired {} days ago", device.hostname, days_until_expiration.abs());
                    devices_expired.push(device);
                },
                Ordering::Equal => {
                    println!("{} expires today", device.hostname);
                    devices_expiring.push(device);
                }
            }
        }
    }

    // Send the push notification to my phone
    if devices_expired.len() == 1 {
        let device_name = &devices_expired[0].hostname;
        let msg = format!("{} has expired!", device_name);
        send_message(&msg, &cfg.pushover_token, &cfg.pushover_user_key);
    } else if devices_expired.len() > 1 {
        let msg = format!("{} devices are expired!", devices_expired.len());
        send_message(&msg, &cfg.pushover_token, &cfg.pushover_user_key);
    } else if devices_expiring.len() == 1 {
        let device = devices_expired[0];
        let device_name = &device.hostname;
        let days_until_expiration = (device.expires - utc).num_days();

        let msg = if days_until_expiration == 0 {
            format!("{} is expiring today!", device_name)
        } else {
            format!("{} is expiring in {} days!", device_name, days_until_expiration)
        };

        send_message(&msg, &cfg.pushover_token, &cfg.pushover_user_key);
    } else {
        let msg = format!("{} devices are expiring soon!", devices_expiring.len());
        send_message(&msg, &cfg.pushover_token, &cfg.pushover_user_key);
    }

    Ok(())
}
