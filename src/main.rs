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

pub struct PngVideoEncoder {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    width: u32,
    height: u32,
    framerate: gst::Fraction,
}

impl PngVideoEncoder {
    pub fn new(
        output_path: &str,
        width: u32,
        height: u32,
        framerate: gst::Fraction,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        gst::init()?;

        let pipeline = gst::Pipeline::new(None);

        // Create elements
        let appsrc = gst::ElementFactory::make("appsrc")
            .build()?
            .downcast::<gst_app::AppSrc>()
            .unwrap();

        let pngdec = gst::ElementFactory::make("pngdec").build()?;
        let videoconvert = gst::ElementFactory::make("videoconvert").build()?;
        let videoscale = gst::ElementFactory::make("videoscale").build()?;
        let encoder = gst::ElementFactory::make("x264enc").build()?;
        let muxer = gst::ElementFactory::make("mp4mux").build()?;
        let filesink = gst::ElementFactory::make("filesink").build()?;

        // Configure appsrc
        let caps = gst::Caps::builder("image/png")
            .field("width", width as i32)
            .field("height", height as i32)
            .field("framerate", framerate)
            .build();

        appsrc.set_property("caps", &caps);
        appsrc.set_property("format", &gst::Format::Time);
        appsrc.set_property("is-live", &true);
        appsrc.set_property("stream-type", &gst_app::AppStreamType::Stream);

        // Configure encoder
        encoder.set_property("tune", &"zerolatency");
        encoder.set_property("speed-preset", &"fast");

        // Configure file sink
        filesink.set_property("location", &output_path);

        // Add elements to pipeline
        pipeline.add_many(&[
            &appsrc.clone().upcast::<gst::Element>(),
            &pngdec,
            &videoconvert,
            &videoscale,
            &encoder,
            &muxer,
            &filesink,
        ])?;

        // Link elements
        gst::Element::link_many(&[
            &appsrc.clone().upcast::<gst::Element>(),
            &pngdec,
            &videoconvert,
            &videoscale,
            &encoder,
            &muxer,
            &filesink,
        ])?;

        Ok(PngVideoEncoder {
            pipeline,
            appsrc,
            width,
            height,
            framerate,
        })
    }

    pub fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.pipeline.set_state(gst::State::Playing)?;
        Ok(())
    }

    pub fn push_png_buffer(&self, png_data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let buffer = gst::Buffer::from_slice(png_data);

        // Set buffer timestamp (you may want to calculate this based on frame index)
        if let Some(clock) = self.pipeline.clock() {
            let base_time = self.pipeline.base_time();
            let now = clock.time();
            if let Some(running_time) = now.checked_sub(base_time) {
                let mut buffer_ref = buffer.make_mut();
                buffer_ref.set_pts(running_time);
            }
        }

        match self.appsrc.push_buffer(buffer) {
            Ok(gst::FlowSuccess::Ok) => Ok(()),
            Err(gst::FlowError::Flushing) => Err("Pipeline is flushing".into()),
            Err(gst::FlowError::Eos) => Err("End of stream".into()),
            Err(err) => Err(format!("Failed to push buffer: {:?}", err).into()),
        }
    }

    pub fn finish(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.appsrc.end_of_stream()?;

        // Wait for EOS
        let bus = self.pipeline.bus().unwrap();
        for msg in bus.iter_timed(gst::ClockTime::NONE) {
            match msg.view() {
                gst::MessageView::Eos(..) => break,
                gst::MessageView::Error(err) => {
                    return Err(format!("Pipeline error: {}", err.error()).into());
                }
                _ => {}
            }
        }

        self.pipeline.set_state(gst::State::Null)?;
        Ok(())
    }
}

// Example usage
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let encoder = PngVideoEncoder::new(
        "output.mp4",
        1920,
        1080,
        gst::Fraction::new(30, 1), // 30 fps
    )?;

    encoder.start()?;

    // Example: Push some PNG image data
    // In practice, you'd load actual PNG files or generate PNG data
    let png_files = vec!["frame1.png", "frame2.png", "frame3.png"];

    for png_file in png_files {
        // Load PNG data (you'd implement this based on your source)
        let png_data = std::fs::read(png_file)?;
        encoder.push_png_buffer(&png_data)?;

        // Optional: add delay between frames if needed
        std::thread::sleep(std::time::Duration::from_millis(33)); // ~30fps
    }

    encoder.finish()?;
    println!("Video encoding completed!");

    Ok(())
}
