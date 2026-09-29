//! Live check against a real server:
//!   XTREAM_URL=http://host:8080 XTREAM_USER=... XTREAM_PASS=... cargo run -p xtream --example login
//! Never commit credentials. The sample URL printed at the end contains them.

use xtream::Client;

#[tokio::main(flavor = "current_thread")]
async fn main() -> xtream::Result<()> {
    let env = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("set {k}"));
    let c = Client::new(&env("XTREAM_URL"), env("XTREAM_USER"), env("XTREAM_PASS"))?;

    let a = c.auth().await?;
    println!(
        "logged in as {} ({:?}), {:?}/{:?} connections, expires {:?}",
        a.user_info.username,
        a.user_info.status,
        a.user_info.active_cons,
        a.user_info.max_connections,
        a.user_info.exp_date,
    );

    let cats = c.live_categories().await?;
    println!("{} live categories", cats.len());
    for cat in cats.iter().take(10) {
        println!("  {:>6}  {}", cat.category_id, cat.category_name);
    }

    if let Some(cat) = cats.first() {
        let streams = c.live_streams(Some(cat.category_id)).await?;
        println!("{} channels in '{}'", streams.len(), cat.category_name);
        if let Some(s) = streams.first() {
            println!("sample: {} -> {}", s.name, c.live_url(s.stream_id, "m3u8"));
        }
    }
    Ok(())
}
