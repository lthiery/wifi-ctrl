use env_logger::Env;
use log::{error, info};
use wifi_ctrl::{sta, Result};

#[tokio::main]
async fn main() -> Result {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    info!("Starting wifi-sta example");

    let mut setup = sta::WifiSetup::new()?;
    // Use something like ifconfig to figure out the name of your WiFi interface
    setup.set_socket_path("/var/run/wpa_supplicant/wlp2s0");

    let broadcast = setup.get_broadcast_receiver();
    let requester = setup.get_request_client();
    let runtime = setup.complete();

    let (_runtime, _broadcast) = tokio::join!(
        async move {
            if let Err(e) = runtime.run().await {
                error!("Error: {e}");
            }
        },
        app(requester, broadcast)
    );
    Ok(())
}

async fn app(requester: sta::RequestClient, mut broadcast: sta::BroadcastReceiver) -> Result {
    // Block until runtime has a confirmed socket connection.
    // This isn't really essential, but it makes a shutdown happen more gracefully on failures.
    while let Ok(broadcast) = broadcast.recv().await {
        if let sta::Broadcast::Ready = broadcast {
            break;
        } else {
            info!("Received unexpected broadcast: {:?}", broadcast);
        }
    }

    info!("Requesting scan");
    let scan = requester.get_scan().await?;
    info!("Scan complete");
    for scan in scan.iter() {
        info!("   {:?}", scan);
    }

    let networks = requester.get_networks().await?;
    info!("Known networks");
    for networks in networks.iter() {
        info!("   {:?}", networks);
    }
    info!("Shutting down");
    requester.shutdown().await?;
    Ok(())
}
