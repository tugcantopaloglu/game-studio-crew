#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod daemon;
mod page;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

use daemon::{Daemon, Failure};

const WATCH_INTERVAL: Duration = Duration::from_millis(500);
#[cfg(windows)]
const ICON_RESOURCE: u16 = 1;

enum Signal {
    Ready,
    Broken(Failure),
    Restart,
}

type Slot = Arc<Mutex<Option<Daemon>>>;

fn main() -> wry::Result<()> {
    let event_loop = EventLoopBuilder::<Signal>::with_user_event().build();
    let window = wearing_the_studio_mark(
        WindowBuilder::new()
            .with_title("Game Studio Crew")
            .with_inner_size(LogicalSize::new(1440.0, 900.0))
            .with_min_inner_size(LogicalSize::new(900.0, 600.0)),
    )
    .build(&event_loop)
    .expect("the studio window could not be created");

    let asked = event_loop.create_proxy();
    let webview = WebViewBuilder::new()
        .with_html(page::starting())
        .with_ipc_handler(move |request| {
            if request.body() == page::RESTART {
                let _ = asked.send_event(Signal::Restart);
            }
        })
        .build(&window)?;

    let slot: Slot = Arc::new(Mutex::new(None));
    bring_the_studio_up(&slot, event_loop.create_proxy());

    let restarter = event_loop.create_proxy();
    let mut broken = false;
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(Signal::Ready) if !broken => {
                let _ = webview.load_url(&daemon::floor_url());
            }
            Event::UserEvent(Signal::Broken(failure)) => {
                broken = true;
                let _ = webview.load_html(&page::failure(&failure));
            }
            Event::UserEvent(Signal::Restart) if broken => {
                broken = false;
                let _ = webview.load_html(&page::starting());
                shut_the_studio_down(&slot);
                empty_the_slot(&slot);
                bring_the_studio_up(&slot, restarter.clone());
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                shut_the_studio_down(&slot);
                *control_flow = ControlFlow::Exit;
            }
            Event::LoopDestroyed => shut_the_studio_down(&slot),
            _ => {}
        }
    });
}

fn bring_the_studio_up(slot: &Slot, proxy: EventLoopProxy<Signal>) {
    let supervised = slot.clone();
    let starter = proxy.clone();
    let watcher = proxy.clone();

    std::thread::spawn(move || {
        let complain = move |missing| {
            let _ = proxy.send_event(Signal::Broken(missing));
        };
        match daemon::bring_up(&supervised, complain) {
            Err(failure) => {
                let _ = starter.send_event(Signal::Broken(failure));
            }
            Ok(()) => {
                let _ = starter.send_event(Signal::Ready);
                watch(&supervised, watcher);
            }
        }
    });
}

fn empty_the_slot(slot: &Slot) {
    if let Ok(mut held) = slot.lock() {
        *held = None;
    }
}

#[cfg(windows)]
fn wearing_the_studio_mark(builder: WindowBuilder) -> WindowBuilder {
    use tao::dpi::PhysicalSize;
    use tao::platform::windows::{IconExtWindows, WindowBuilderExtWindows};
    use tao::window::Icon;

    let sized = |edge| Icon::from_resource(ICON_RESOURCE, Some(PhysicalSize::new(edge, edge))).ok();
    builder
        .with_window_icon(sized(16))
        .with_taskbar_icon(sized(32))
}

#[cfg(not(windows))]
fn wearing_the_studio_mark(builder: WindowBuilder) -> WindowBuilder {
    builder
}

fn watch(slot: &Slot, proxy: EventLoopProxy<Signal>) {
    loop {
        std::thread::sleep(WATCH_INTERVAL);
        let notice = match slot.lock() {
            Err(_) => return,
            Ok(mut held) => match held.as_mut() {
                None => return,
                Some(daemon) => {
                    if !daemon.stopped() {
                        continue;
                    }
                    daemon.death_notice()
                }
            },
        };
        let _ = proxy.send_event(Signal::Broken(notice));
        return;
    }
}

fn shut_the_studio_down(slot: &Slot) {
    if let Ok(mut held) = slot.lock() {
        if let Some(daemon) = held.as_mut() {
            daemon.shutdown();
        }
    }
}
