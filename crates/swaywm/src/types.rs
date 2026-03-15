use serde::Deserialize;

/// Response to the IPC subscribe command.
#[derive(Deserialize)]
pub struct IpcResponse {
	pub success: bool,
}

/// Input device (e.g., keyboard) with active XKB layout
#[derive(Deserialize)]
pub struct InputDevice {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub xkb_active_layout_name: Option<String>,
}

/// Event payload containing the input device
#[derive(Deserialize)]
pub struct InputEvent {
	pub input: InputDevice,
}
