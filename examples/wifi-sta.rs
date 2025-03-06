#![allow(clippy::result_large_err)]
use env_logger::Env;
use log::{error, info};
use network_interface::{NetworkInterface, NetworkInterfaceConfig};
use tokio::io;
use wifi_ctrl::{sta, Result};

#[tokio::main]
async fn main() -> Result {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    info!("Starting wifi-sta example");

    let mut network_interfaces = NetworkInterface::show().unwrap();
    network_interfaces.sort_by(|a, b| a.index.cmp(&b.index));
    for (i, itf) in network_interfaces.iter().enumerate() {
        info!("[{:?}] {:?}", i, itf.name);
    }
    let user_input = read_until_break().await;
    let index = user_input.trim().parse::<usize>().expect("");
    let mut setup = sta::WifiSetup::new();

    let proposed_path = format!("/var/run/wpa_supplicant/{}", network_interfaces[index].name);
    info!("Connect to \"{proposed_path}\"? Type full new path or just press enter to accept.");

    let user_input = read_until_break().await;
    if user_input.trim().is_empty() {
        setup.set_socket_path(proposed_path);
    } else {
        setup.set_socket_path(user_input.trim().to_string());
    }

    let broadcast = setup.get_broadcast_receiver();
    let requester = setup.get_request_client();
    let mut runtime = setup.complete();

    let (_runtime, _app, _broadcast) = tokio::join!(
        async move {
            if let Err(e) = runtime.run().await {
                error!("Error: {e}");
            }
        },
        app(requester),
        broadcast_listener(broadcast),
    );
    Ok(())
}

async fn app(requester: sta::RequestClient) -> Result {
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

async fn broadcast_listener(mut broadcast_receiver: sta::BroadcastReceiver) -> Result {
    while let Ok(broadcast) = broadcast_receiver.recv().await {
        info!("Broadcast: {:?}", broadcast);
    }
    Ok(())
}

async fn read_until_break() -> String {
    use futures::stream::StreamExt;
    use tokio_util::codec::{FramedRead, LinesCodec};
    let stdin = io::stdin();
    let mut reader = FramedRead::new(stdin, LinesCodec::new());
    reader.next().await.unwrap().unwrap()
}
