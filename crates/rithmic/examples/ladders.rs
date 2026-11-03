//! Load the last hour of minute ladders (bid and ask volume at each price)
//! from Rithmic Test, and print a summary. Reads `~/.rithmic` as `session` does.

use rithmic_rs::{rti::messages::RithmicMessage, ConnectStrategy, RithmicConfig, RithmicEnv,
    RithmicHistoryPlant, VolumeProfileMinuteBarsRequest};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(std::path::Path::new(&std::env::var("HOME")?).join(".rithmic"))?;
    let mut lines = text.lines();
    let config = RithmicConfig::builder(RithmicEnv::Test)
        .url(rithmic::TEST_URL).beta_url(rithmic::TEST_URL)
        .user(lines.next().unwrap_or_default()).password(lines.next().unwrap_or_default())
        .system_name(rithmic::TEST_SYSTEM).app_name(rithmic::APP_NAME).app_version("0")
        .build()?;
    let plant = RithmicHistoryPlant::connect(&config, ConnectStrategy::Simple).await?;
    let handle = plant.get_handle();
    handle.login().await?;
    let hours: i32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() as i32;
    let started = std::time::Instant::now();
    let bars = handle.load_volume_profile_minute_bars(VolumeProfileMinuteBarsRequest::new()
        .symbol("ESZ6").exchange("CME").bar_type_period(1)
        .start_time_sec(now - hours * 3600).end_time_sec(now).resume_bars(true)).await?;
    let mut minutes = 0; let mut cells = 0; let mut sample = None;
    for bar in &bars {
        if let RithmicMessage::ResponseVolumeProfileMinuteBars(bar) = &bar.message {
            if bar.profile_price.is_empty() { continue; }
            minutes += 1; cells += bar.profile_price.len();
            sample = Some((bar.marker, bar.volume, bar.bid_volume, bar.ask_volume,
                bar.profile_price.iter().zip(&bar.profile_bid_volume).zip(&bar.profile_ask_volume)
                    .map(|((p, b), a)| format!("{p}: {b}x{a}")).collect::<Vec<_>>()));
        }
    }
    println!("{hours}h: {minutes} minutes, {cells} price cells, {:?}", started.elapsed());
    println!("last minute: {sample:?}");
    handle.disconnect().await?;
    Ok(())
}
