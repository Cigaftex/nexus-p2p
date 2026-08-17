use std::{path::PathBuf, sync::Arc};

use nexus_core::{DeviceId, Node, NodeConfig};
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let data_dir = PathBuf::from(args.next().unwrap_or_else(|| "./nexus-data".into()));
    let display_name = args.next().unwrap_or_else(|| "Nexus device".into());
    let listen_port = args
        .next()
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(47777);
    let node = Node::new(NodeConfig {
        data_dir,
        display_name,
        listen_port,
        enable_mdns: true,
    })?;
    node.start().await?;
    println!(
        "Nexus {} listening on {}",
        node.identity().device_id,
        listen_port
    );
    println!("commands: peers | pair ID | say ID TEXT | file ID PATH | chat ID | sync ID | quit");

    let printer = Arc::clone(&node);
    tokio::spawn(async move {
        let mut events = printer.subscribe();
        while let Ok(event) = events.recv().await {
            println!(
                "event {}",
                serde_json::to_string(&event).unwrap_or_default()
            );
        }
    });

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        let mut parts = line.trim().splitn(3, ' ');
        match parts.next().unwrap_or("") {
            "peers" => println!("{}", serde_json::to_string_pretty(&node.peers()?)?),
            "pair" => {
                node.pair(&DeviceId(required(parts.next(), "peer id")?.into()))
                    .await?
            }
            "say" => node
                .send_text(
                    &DeviceId(required(parts.next(), "peer id")?.into()),
                    required(parts.next(), "text")?,
                )
                .await
                .map(|_| ())?,
            "file" => node
                .send_file(
                    &DeviceId(required(parts.next(), "peer id")?.into()),
                    required(parts.next(), "path")?,
                    "application/octet-stream",
                )
                .await
                .map(|_| ())?,
            "chat" => println!(
                "{}",
                serde_json::to_string_pretty(
                    &node.chat(&DeviceId(required(parts.next(), "peer id")?.into()))?
                )?
            ),
            "sync" => {
                node.sync(&DeviceId(required(parts.next(), "peer id")?.into()))
                    .await?
            }
            "quit" | "exit" => break,
            "" => {}
            other => eprintln!("unknown command: {other}"),
        }
    }
    Ok(())
}

fn required<'a>(value: Option<&'a str>, name: &str) -> anyhow::Result<&'a str> {
    value.ok_or_else(|| anyhow::anyhow!("missing {name}"))
}
