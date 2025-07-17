// Copyright 2020-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT
use gstreamer::prelude::*;
use std::{
    sync::mpsc,
    thread::{self, sleep},
    time::{Duration, Instant},
};
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::WebViewBuilder;
use wry::WebViewExtMacOS;
use wry::dpi::Size;

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const FPS: u32 = 30;
const FRAME_DURATION: Duration = Duration::from_millis(1000 / FPS as u64);

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
    let size = Size::Physical(wry::dpi::PhysicalSize {
        width: WIDTH,
        height: HEIGHT,
    });
    let window = WindowBuilder::new()
        .with_inner_size(size)
        .build(&event_loop)
        .unwrap();

    let builder = WebViewBuilder::new()
        .with_url("https://www.sagejenson.com/36points/")
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
    let mut last_frame_time = Instant::now();

    println!("Starting webview, will take screenshot in 5 seconds...");

    let mut count = 1;

    let (tx, rx) = mpsc::channel::<(Vec<u8>, u64)>();

    // Start encoder in a separate thread
    let encoder_handle = thread::spawn(move || {
        let encoder =
            PngVideoEncoder::new("output.mp4", WIDTH, HEIGHT, gst::Fraction::new(30, 1)).unwrap();
        encoder.start().unwrap();
        while let Ok((png_data, timestamp)) = rx.recv() {
            if png_data.is_empty() {
                println!("stopping");
                encoder.finish().unwrap();

                break; // Signal to stop
            }
            let static_data: &'static [u8] = Box::leak(png_data.into_boxed_slice());
            println!("whattt");
            encoder
                .push_png_buffer_with_timestamp(static_data, timestamp)
                .unwrap();
        }
    });

    // Run the event loop
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll; // Use Poll to keep checking time

        let now = Instant::now();

        // Check if 5 seconds have passed and we haven't taken the screenshot yet
        if (now.duration_since(last_frame_time) >= FRAME_DURATION)
            && start_time.elapsed() >= Duration::from_secs(7)
        {
            let tx_clone = tx.clone();
            webview
                .take_snapshot(None, move |result| {
                    let png_data = match result {
                        Ok(png_data) => png_data,
                        Err(e) => {
                            eprintln!("Error taking snapshot: {}", e);
                            Vec::new()
                        }
                    };

                    // let static_data: &'static [u8] = Box::leak(png_data.into_boxed_slice());
                    // encoder.push_png_buffer(static_data);
                    // process_png_data(png_data);
                    let timestamp_ns = count as u64 * (1_000_000_000 / FPS as u64);
                    let _ = tx_clone.send((png_data, timestamp_ns));
                })
                .unwrap();

            last_frame_time = now;

            if count == 400 {
                let _ = tx.send((Vec::new(), 0u64));
                println!("end reached");
            }

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
            _ => {}
        }
    });
}

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;

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

        let pipeline = gst::Pipeline::new();

        // Create elements
        let appsrc = gst::ElementFactory::make("appsrc")
            .build()?
            .downcast::<gst_app::AppSrc>()
            .unwrap();

        let pngdec = gst::ElementFactory::make("pngdec").build()?;
        let videoconvert = gst::ElementFactory::make("videoconvert").build()?;
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

        encoder.set_property("threads", &0u32); // auto-detect CPU cores
        encoder.set_property("sliced-threads", &true); // enable slice-based threading
        encoder.set_property("sync-lookahead", &0); // disable lookahead for speed

        // Configure file sink
        filesink.set_property("location", &output_path);

        // Add elements to pipeline
        pipeline.add_many(&[
            &appsrc.clone().upcast::<gst::Element>(),
            &pngdec,
            &videoconvert,
            &encoder,
            &muxer,
            &filesink,
        ])?;

        // Link elements
        gst::Element::link_many(&[
            &appsrc.clone().upcast::<gst::Element>(),
            &pngdec,
            &videoconvert,
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

    pub fn push_png_buffer_with_timestamp(
        &self,
        png_data: &'static [u8],
        timestamp_ns: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut buffer = gst::Buffer::from_slice(png_data);

        // Set timestamp and duration
        {
            let buffer_ref = buffer.get_mut().unwrap();
            buffer_ref.set_pts(gst::ClockTime::from_nseconds(timestamp_ns));
            buffer_ref.set_duration(gst::ClockTime::from_nseconds(
                1_000_000_000 / self.framerate.numer() as u64 * self.framerate.denom() as u64,
            ));
        }

        match self.appsrc.push_buffer(buffer) {
            Ok(gst::FlowSuccess::Ok) => Ok(()),
            Ok(gst::FlowSuccess::CustomSuccess) => Ok(()),
            Ok(gst::FlowSuccess::CustomSuccess2) => Ok(()),
            Ok(gst::FlowSuccess::CustomSuccess1) => Ok(()),
            Err(gst::FlowError::Flushing) => Err("Pipeline is flushing".into()),
            Err(gst::FlowError::Eos) => Err("End of stream".into()),
            Err(err) => Err(format!("Failed to push buffer: {:?}", err).into()),
        }
    }

    pub fn push_png_buffer(
        &self,
        png_data: &'static [u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let buffer = gst::Buffer::from_slice(png_data);

        match self.appsrc.push_buffer(buffer) {
            Ok(gst::FlowSuccess::Ok) => Ok(()),
            Ok(gst::FlowSuccess::CustomSuccess) => Ok(()),
            Ok(gst::FlowSuccess::CustomSuccess2) => Ok(()),
            Ok(gst::FlowSuccess::CustomSuccess1) => Ok(()),
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
