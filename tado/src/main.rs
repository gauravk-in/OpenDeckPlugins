mod api;
mod auth;
mod files;
mod render;
mod store;

use openaction::*;
use serde_json::Value;

use store::{DialSettings, RoomSettings};

/// Key: shows a room's temperature; pressing it selects the room for the dials.
struct RoomAction;
#[async_trait]
impl Action for RoomAction {
	const UUID: &'static str = store::ROOM_UUID;
	type Settings = RoomSettings;

	async fn will_appear(&self, instance: &Instance, settings: &RoomSettings) -> OpenActionResult<()> {
		store::set_room(instance, settings);
		store::redraw_later();
		Ok(())
	}
	async fn will_disappear(&self, instance: &Instance, _settings: &RoomSettings) -> OpenActionResult<()> {
		store::forget(instance);
		Ok(())
	}
	async fn did_receive_settings(&self, instance: &Instance, settings: &RoomSettings) -> OpenActionResult<()> {
		store::set_room(instance, settings);
		store::redraw().await;
		Ok(())
	}
	async fn key_up(&self, instance: &Instance, settings: &RoomSettings) -> OpenActionResult<()> {
		store::press_room(instance, settings).await;
		Ok(())
	}
	async fn property_inspector_did_appear(&self, instance: &Instance, _: &RoomSettings) -> OpenActionResult<()> {
		store::inspector_opened(instance).await;
		Ok(())
	}
	async fn property_inspector_did_disappear(&self, instance: &Instance, _: &RoomSettings) -> OpenActionResult<()> {
		store::inspector_closed(instance);
		Ok(())
	}
	async fn send_to_plugin(&self, instance: &Instance, _: &RoomSettings, payload: &Value) -> OpenActionResult<()> {
		store::from_inspector(instance, payload).await;
		Ok(())
	}
}

/// Dial: turn to change the selected room's target temperature; press to go back to the schedule.
struct DialAction;
#[async_trait]
impl Action for DialAction {
	const UUID: &'static str = store::DIAL_UUID;
	type Settings = DialSettings;

	async fn will_appear(&self, instance: &Instance, settings: &DialSettings) -> OpenActionResult<()> {
		store::set_dial(instance, settings);
		store::redraw_later();
		Ok(())
	}
	async fn will_disappear(&self, instance: &Instance, _settings: &DialSettings) -> OpenActionResult<()> {
		store::forget(instance);
		Ok(())
	}
	async fn did_receive_settings(&self, instance: &Instance, settings: &DialSettings) -> OpenActionResult<()> {
		store::set_dial(instance, settings);
		Ok(())
	}
	async fn dial_rotate(&self, instance: &Instance, settings: &DialSettings, ticks: i16, _: bool) -> OpenActionResult<()> {
		store::turn_dial(instance, settings, ticks).await;
		Ok(())
	}
	async fn dial_up(&self, instance: &Instance, settings: &DialSettings) -> OpenActionResult<()> {
		store::press_dial(instance, settings).await;
		Ok(())
	}
	async fn property_inspector_did_appear(&self, instance: &Instance, _: &DialSettings) -> OpenActionResult<()> {
		store::inspector_opened(instance).await;
		Ok(())
	}
	async fn property_inspector_did_disappear(&self, instance: &Instance, _: &DialSettings) -> OpenActionResult<()> {
		store::inspector_closed(instance);
		Ok(())
	}
	async fn send_to_plugin(&self, instance: &Instance, _: &DialSettings, payload: &Value) -> OpenActionResult<()> {
		store::from_inspector(instance, payload).await;
		Ok(())
	}
}

#[tokio::main]
async fn main() -> OpenActionResult<()> {
	let args: Vec<String> = std::env::args().collect();
	// Debug helper: `opendeck-tado --render out.png` draws sample keys.
	if args.get(1).map(String::as_str) == Some("--render") {
		let state = api::RoomState { current: Some(21.34), target: Some(20.5), heating: Some(35.0), ..Default::default() };
		let keys = [
			render::draw(&render::Key { name: "Living room", state: Some(&state), selected: true, pending: None, notice: None }),
			render::draw(&render::Key { name: "Bedroom", state: Some(&state), selected: false, pending: Some(Some(22.0)), notice: None }),
			render::draw(&render::Key {
				name: "Bathroom",
				state: Some(&api::RoomState { current: Some(18.0), manual: true, ..Default::default() }),
				selected: false,
				pending: None,
				notice: None,
			}),
			render::message("tado°", "Choose a room"),
		];
		let size = render::KEY_SIZE;
		let mut sheet = image::RgbaImage::from_pixel(size * 4 + 30, size, image::Rgba([60, 60, 60, 255]));
		for (i, key) in keys.iter().enumerate() {
			image::imageops::overlay(&mut sheet, key, (i as u32 * (size + 10)) as i64, 0);
		}
		sheet.save(args.get(2).map(String::as_str).unwrap_or("keys.png")).expect("save");
		return Ok(());
	}

	{
		use simplelog::*;
		if let Err(error) = TermLogger::init(LevelFilter::Info, Config::default(), TerminalMode::Stdout, ColorChoice::Never) {
			eprintln!("Logger initialization failed: {error}");
		}
	}

	store::init().await;
	register_action(RoomAction).await;
	register_action(DialAction).await;
	run(args).await
}
