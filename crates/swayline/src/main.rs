use std::{
	io::{
		self,
		Write,
	},
	time::Duration,
};

use anyhow::{
	Context,
	Result,
	anyhow,
};
use clap::Parser;

#[derive(clap::Parser)]
struct Args {
	#[arg(
		long,
		env = "SWAYSOCK",
		help = "Unix domain socket of swaywm IPC server"
	)]
	swaysock: String,
	#[arg(long, help = "Status lines output format pattern")]
	format: String,
	#[arg(long, help = "Interval between each flush in seconds")]
	interval: f64,
}
use swaywm;

fn now_rfc3339() -> String {
	chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
	// Parse CLAP (Command Line Argument Parser) command line arguments,
	// check their validity, and exit in case of failure.
	let args = Args::parse();
	// Establish a connection to the Swaywm IPC server to listen for events.
	let mut client = swaywm::Ipc::new(args.swaysock).await?;
	// Receive initial inputs early during the application startup.
	client.send(swaywm::IpcCommand::GetInputs, "").await?;
	let mut layout = client
		.recv()
		.await
		.and_then(|(cmd, payload)| {
			if cmd != swaywm::IpcCommand::GetInputs {
				return Err(anyhow!("Unsupported command type ({:?})", cmd));
			}
			serde_json::from_slice::<Vec<swaywm::InputDevice>>(&payload)
				.map_err(|err| {
					anyhow!("failed to parse IPC inputs response: {err}")
				})
		})
		.and_then(|devices| {
			devices
				.into_iter()
				.find(|device| device.xkb_active_layout_name.is_some())
				.map(|device| device.xkb_active_layout_name.unwrap())
				.context("No inputs to parse from IPC response")
		})?;
	// Subscribe to the input type events messages (e.g. xkb_layout change).
	client
		.send(swaywm::IpcCommand::Subscribe, "[\"input\"]")
		.await?;
	let resp = client.recv().await.and_then(|(_, payload)| {
		serde_json::from_slice::<swaywm::IpcResponse>(&payload)
			.context("Failed to parse IPC response for subscribe")
	})?;
	if resp.success != true {
		return Err(anyhow!("Failed to subscribe input events successfully"));
	}
	// Wait for the layout change message or interval to display the status.
	let mut interval =
		tokio::time::interval(Duration::from_secs_f64(args.interval));
	let mut localtime = now_rfc3339();
	loop {
		tokio::select! {
			Ok((cmd, payload)) = client.recv() => {
				if cmd != swaywm::IpcCommand::InputEvent {
					continue;
				}
				// When an event occurs, the client receives messages from all
				// devices, assuming that any of them is appropriate.
				if let Some(name) = serde_json::from_slice::<
					swaywm::InputEvent
				>(&payload)?.input.xkb_active_layout_name {
					layout = name;
				};
			}
			_ = interval.tick() => {
				localtime = now_rfc3339();
			}
		}
		write!(
			io::stdout(),
			"{}\n",
			args.format
				.replace("%layout%", &layout)
				.replace("%rfc3339%", &localtime)
		)?;
		io::stdout().flush()?;
	}
}
