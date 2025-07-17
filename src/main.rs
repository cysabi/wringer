// Copyright 2020-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT
use dpi::Size;
use gstreamer::prelude::*;
use std::{
    thread::sleep,
    time::{Duration, Instant},
};
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::WebViewBuilder;
use wry::WebViewExtMacOS;

const WIDTH: u32 = 920;
const HEIGHT: u32 = 480;

fn process_png_data(png_data: Vec<u8>) {
    if png_data.is_empty() {
        println!("No PNG data received");
    } else {
        let rgb = image::load_from_memory(&png_data)
            .map_err(|e| format!("PNG decode failed: {}", e))
            .unwrap();

        rgb.save("output.png").unwrap();
        // println!("Screenshot saved as output.png");
    }
}

fn main() -> wry::Result<()> {
    let event_loop = EventLoop::new();
    let size = Size::Physical(dpi::PhysicalSize {
        width: WIDTH,
        height: HEIGHT,
    });
    let window = WindowBuilder::new()
        .with_inner_size(size)
        .build(&event_loop)
        .unwrap();

    let builder = WebViewBuilder::new()
        .with_url("http://tauri.app")
        .with_drag_drop_handler(|e| {
            match e {
                wry::DragDropEvent::Enter { paths, position } => {
                    println!("DragEnter: {position:?} {paths:?} ")
                }
                wry::DragDropEvent::Over { position } => println!("DragOver: {position:?} "),
                wry::DragDropEvent::Drop { paths, position } => {
                    println!("DragDrop: {position:?} {paths:?} ")
                }
                wry::DragDropEvent::Leave => println!("DragLeave"),
                _ => {}
            }
            true
        });

    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    ))]
    let webview = builder.build(&window)?;

    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    )))]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window.default_vbox().unwrap();
        builder.build_gtk(vbox)?
    };

    // Track when we started and whether we've taken the screenshot
    let start_time = Instant::now();
    let mut screenshot_taken = false;

    println!("Starting webview, will take screenshot in 5 seconds...");

    let mut count = 1;

    // Run the event loop
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll; // Use Poll to keep checking time

        // Check if 5 seconds have passed and we haven't taken the screenshot yet
        if !screenshot_taken && start_time.elapsed() >= Duration::from_secs(5) {
            webview
                .take_snapshot(None, move |result| {
                    let png_data = match result {
                        Ok(png_data) => png_data,
                        Err(e) => {
                            eprintln!("Error taking snapshot: {}", e);
                            Vec::new()
                        }
                    };
                    process_png_data(png_data);
                })
                .unwrap();

            count += 1;
            println!("{} / {}", start_time.elapsed().as_secs(), count);
        }

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            // After screenshot is taken, we can go back to Wait mode for efficiency
            Event::MainEventsCleared if screenshot_taken => {
                *control_flow = ControlFlow::Wait;
            }
            _ => {}
        }
    });
}
