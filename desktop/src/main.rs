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

enum Signal {
    Ready,
    Broken(Failure),
}

type Slot = Arc<Mutex<Option<Daemon>>>;

fn main() -> wry::Result<()> {
    let event_loop = EventLoopBuilder::<Signal>::with_user_event().build();
    let window = WindowBuilder::new()
        .with_title("Game Studio Crew")
        .with_inner_size(LogicalSize::new(1440.0, 900.0))
        .with_min_inner_size(LogicalSize::new(900.0, 600.0))
        .build(&event_loop)
        .expect("the studio window could not be created");

    let webview = WebViewBuilder::new()
        .with_html(page::starting())
        .build(&window)?;

    let slot: Slot = Arc::new(Mutex::new(None));
    let starter = event_loop.create_proxy();
    let watcher = event_loop.create_proxy();
    let doctor = event_loop.create_proxy();
    let supervised = slot.clone();

    std::thread::spawn(move || {
        let complain = move |missing| {
            let _ = doctor.send_event(Signal::Broken(missing));
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
