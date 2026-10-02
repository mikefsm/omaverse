//! Drive a pointer in a Wayland compositor, for testing Omaverse.
//!
//! wlrctl can click, but a click is a press and a release in the same place.
//! A drag is press, then move, then release, and wlrctl exposes no way to hold
//! a button down. The protocol underneath it does, so this reaches for that
//! directly.
//!
//! It speaks only to the compositor named by WAYLAND_DISPLAY, which is how it
//! stays confined to a nested test compositor and never touches a real desktop.
//!
//! Usage:
//!   vpointer --size WxH  move X Y  down left  move X Y  up left  sleep 200
//!
//! Positions are absolute, in pixels, within the given size.

use std::time::{SystemTime, UNIX_EPOCH};
use wayland_client::protocol::wl_pointer::ButtonState;
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

#[derive(Default)]
struct Finder {
    manager: Option<ZwlrVirtualPointerManagerV1>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for Finder {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            if interface == "zwlr_virtual_pointer_manager_v1" {
                state.manager =
                    Some(registry.bind::<ZwlrVirtualPointerManagerV1, _, _>(
                        name,
                        version.min(2),
                        qh,
                        (),
                    ));
            }
        }
    }
}

impl Dispatch<ZwlrVirtualPointerManagerV1, ()> for Finder {
    fn event(
        _: &mut Self,
        _: &ZwlrVirtualPointerManagerV1,
        _: <ZwlrVirtualPointerManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrVirtualPointerV1, ()> for Finder {
    fn event(
        _: &mut Self,
        _: &ZwlrVirtualPointerV1,
        _: <ZwlrVirtualPointerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

fn now() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0)
}

fn button_code(name: &str) -> Option<u32> {
    match name {
        "left" => Some(BTN_LEFT),
        "right" => Some(BTN_RIGHT),
        "middle" => Some(BTN_MIDDLE),
        _ => None,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!(
            "usage: vpointer --size WxH move X Y | down BTN | up BTN | click BTN | sleep MS ..."
        );
        std::process::exit(2);
    }

    let conn = Connection::connect_to_env()?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());

    let mut finder = Finder::default();
    queue.roundtrip(&mut finder)?;
    let manager = finder
        .manager
        .clone()
        .ok_or("this compositor offers no virtual pointer")?;
    let pointer = manager.create_virtual_pointer(None, &qh, ());

    // Absolute motion is expressed as a fraction of an extent, so the extent
    // has to match the surface being aimed at.
    let (mut width, mut height) = (1920u32, 1080u32);

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--size" => {
                let spec = args.get(i + 1).ok_or("--size needs WxH")?;
                let (w, h) = spec.split_once('x').ok_or("--size needs WxH")?;
                width = w.parse()?;
                height = h.parse()?;
                i += 2;
            }
            "move" => {
                let x: u32 = args.get(i + 1).ok_or("move needs X Y")?.parse()?;
                let y: u32 = args.get(i + 2).ok_or("move needs X Y")?.parse()?;
                pointer.motion_absolute(now(), x, y, width, height);
                pointer.frame();
                conn.flush()?;
                i += 3;
            }
            action @ ("down" | "up") => {
                let name = args.get(i + 1).map(String::as_str).unwrap_or("left");
                let code = button_code(name).ok_or("unknown button")?;
                let state = if action == "down" {
                    ButtonState::Pressed
                } else {
                    ButtonState::Released
                };
                pointer.button(now(), code, state);
                pointer.frame();
                conn.flush()?;
                i += 2;
            }
            "click" => {
                let name = args.get(i + 1).map(String::as_str).unwrap_or("left");
                let code = button_code(name).ok_or("unknown button")?;
                pointer.button(now(), code, ButtonState::Pressed);
                pointer.frame();
                pointer.button(now(), code, ButtonState::Released);
                pointer.frame();
                conn.flush()?;
                i += 2;
            }
            "sleep" => {
                let ms: u64 = args.get(i + 1).ok_or("sleep needs MS")?.parse()?;
                conn.flush()?;
                std::thread::sleep(std::time::Duration::from_millis(ms));
                i += 2;
            }
            other => return Err(format!("unknown command: {other}").into()),
        }
    }

    conn.flush()?;
    queue.roundtrip(&mut finder)?;
    Ok(())
}
