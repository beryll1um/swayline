use std::{
	mem,
	path::Path,
};

use anyhow::{
	Context,
	Error,
	Result,
	anyhow,
};
use tokio::{
	io::{
		AsyncReadExt,
		AsyncWriteExt,
	},
	net::UnixStream,
};
use zerocopy::{
	FromBytes,
	Immutable,
	IntoBytes,
	KnownLayout,
	NativeEndian,
	U32,
	Unaligned,
};

/// Constants below are hardcoded inside the IPC server and must be the same
/// in both client and server IPC headers.
/// See: https://github.com/swaywm/sway/blob/c57daaf/common/ipc-client.c#L11
const IPC_MAGIC: [u8; 6] = *b"i3-ipc";

/// See: https://github.com/swaywm/sway/blob/c57daaf/include/ipc.h#L10
const IPC_SUBSCRIBE: u32 = 2;

/// See: https://github.com/swaywm/sway/blob/c57daaf/include/ipc.h#L23
const IPC_GET_INPUTS: u32 = 100;

/// See: https://github.com/swaywm/sway/blob/c57daaf/include/ipc.h#L38
const IPC_EVENT_INPUT: u32 = (1 << 31) | 21;

/// IPC command types supported by the Sway/Wayland compositor IPC protocol.
#[repr(u32)]
#[derive(PartialEq, Debug)]
pub enum IpcCommand {
	Subscribe = IPC_SUBSCRIBE,
	GetInputs = IPC_GET_INPUTS,
	InputEvent = IPC_EVENT_INPUT,
}

impl TryFrom<u32> for IpcCommand {
	type Error = Error;
	fn try_from(value: u32) -> Result<Self> {
		match value {
			IPC_SUBSCRIBE => Ok(Self::Subscribe),
			IPC_GET_INPUTS => Ok(Self::GetInputs),
			IPC_EVENT_INPUT => Ok(Self::InputEvent),
			other => Err(anyhow!("Unsupported command type ({other})")),
		}
	}
}

/// Packed IPC message header as defined by the Sway IPC protocol.
#[repr(packed)]
#[derive(IntoBytes, FromBytes, Immutable, Unaligned, KnownLayout)]
struct IpcHeader {
	magic: [u8; IPC_MAGIC.len()],
	size: U32<NativeEndian>,
	r#type: U32<NativeEndian>,
}

impl IpcHeader {
	/// Create a new IPC header for the given payload size and command.
	fn new(
		size: u32,
		cmd: IpcCommand,
	) -> Self {
		Self {
			magic: IPC_MAGIC,
			size: U32::new(size),
			r#type: U32::new(cmd as _),
		}
	}

	/// Parse the fixed-size header from raw bytes, validating magic bytes.
	fn parse<'a>(bytes: &'a [u8]) -> Result<&'a Self> {
		let header = IpcHeader::ref_from_bytes(bytes)
			// By default branching is impossible,
			// since it contains a reference to the original data.
			.map_err(|err| anyhow!("{err}"))?;
		// Header magic should always be the expected one, otherwise
		// flow is potentially corrupted.
		if header.magic != IPC_MAGIC {
			return Err(anyhow!("Magic does not match the expected bytes"));
		}
		Ok(header)
	}
}

/// Client for Sway IPC protocol over Unix socket.
pub struct Ipc {
	stream: UnixStream,
}

impl Ipc {
	/// Connect to the Sway IPC Unix socket at the given path.
	pub async fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
		Ok(Self {
			stream: UnixStream::connect(path)
				.await
				.context("Failed to connect to Unix socket")?,
		})
	}

	/// Send an IPC command with optional string payload.
	pub async fn send(
		&mut self,
		cmd: IpcCommand,
		input: &str,
	) -> Result<()> {
		// Write the header and payload to the stream.
		self.stream
			.write_all(
				IpcHeader::new(
					// It's practically impossible to have payload
					// that longer than 2^32-1.
					input.len() as u32,
					cmd,
				)
				.as_bytes(),
			)
			.await?;
		self.stream.write_all(input.as_bytes()).await?;
		self.stream.flush().await?;
		Ok(())
	}

	/// Receive one IPC response: command type and raw payload bytes.
	pub async fn recv(&mut self) -> Result<(IpcCommand, Vec<u8>)> {
		// The IPC header size is always fixed, so read it first to determine
		// the future payload size.
		let mut buf = [0u8; mem::size_of::<IpcHeader>()];
		self.stream.read_exact(&mut buf).await?;
		let header = IpcHeader::parse(&buf)?;
		// Immediately following the header should be the payload,
		// the size of which is specified in the header.
		let mut payload = vec![0u8; header.size.get() as usize];
		self.stream.read_exact(&mut payload).await?;
		// The IPC command type in the header must
		// match the type of the command that invoked it.
		Ok((IpcCommand::try_from(header.r#type.get())?, payload))
	}
}
