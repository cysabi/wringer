// Copyright 2020-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT
use clap::{Parser, Subcommand};
use gstreamer::prelude::*;
use std::{
    process::ExitStatus,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, sleep},
    time::{Duration, Instant},
};
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::WebViewExtMacOS;
use wry::dpi::Size;
use wry::{WebView, WebViewBuilder};

fn process_png_data(png_data: Vec<u8>) {
    if png_data.is_empty() {
        println!("No PNG data received");
    } else {
        let rgb = image::load_from_memory(&png_data)
            .map_err(|e| format!("PNG decode failed: {}", e))
            .unwrap();

        rgb.save("output.png").unwrap();
        std::process::exit(1);
        println!("Screenshot saved as output.png");
    }
}

#[derive(Parser)]
#[command(name = "webview-recorder")]
#[command(about = "A webview recording and capturing tool")]
#[command(version = "1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Verbosity level (0 = quiet, 1 = normal, 2 = verbose)
    #[arg(short, long, default_value = "1")]
    verbosity: u8,
}

#[derive(Subcommand)]
enum Commands {
    /// Capture a single screenshot of the webview
    Capture {
        /// Width of the webview window
        #[arg(short, long, default_value = "1920")]
        width: u32,

        /// Height of the webview window  
        #[arg(short, long, default_value = "1080")]
        height: u32,
    },
    /// Record a video of the webview
    Record {
        /// Width of the webview window
        #[arg(short, long, default_value = "1920")]
        width: u32,

        /// Height of the webview window
        #[arg(short, long, default_value = "1080")]
        height: u32,

        /// Frames per second for recording
        #[arg(short, long, default_value = "30")]
        fps: u16,
    },
}

fn main() -> wry::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Capture { width, height } => {
            if cli.verbosity > 0 {
                println!(
                    "Starting capture mode with dimensions: {}x{}",
                    width, height
                );
            }
            run_capture(width, height, cli.verbosity)
        }
        Commands::Record { width, height, fps } => {
            if cli.verbosity > 0 {
                println!(
                    "Starting record mode with dimensions: {}x{} at {} FPS",
                    width, height, fps
                );
            }
            run_record(width, height, fps, cli.verbosity)
        }
    }
}

fn build_webview(width: u32, height: u32) -> wry::Result<(WebView, EventLoop<()>)> {
    let event_loop = EventLoop::new();
    let size = Size::Physical(wry::dpi::PhysicalSize { width, height });
    let window = WindowBuilder::new()
        .with_inner_size(size)
        .build(&event_loop)
        .unwrap();

    let builder = WebViewBuilder::new()
        .with_url("https://www.apple.com")
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

    Ok((webview, event_loop))
}

fn run_capture(width: u32, height: u32, verbosity: u8) -> wry::Result<()> {
    let (webview, event_loop) = build_webview(width, height)?;
    let mut active_webview = false;

    let last_frame_time = Instant::now();

    let snapshot_taken = Arc::new(AtomicBool::new(false));

    let mut count = 0;

    // Run the event loop
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll; // Use Poll to keep checking time

        let exit_flag = snapshot_taken.clone();

        let now = Instant::now();

        // Check if 5 seconds have passed and we haven't taken the screenshot yet
        if now.duration_since(last_frame_time).as_secs() >= 5 {
            webview
                .take_snapshot(None, move |result| {
                    let png_data = match result {
                        Ok(png_data) => png_data,
                        Err(e) => {
                            eprintln!("Error taking snapshot: {}", e);
                            Vec::new()
                        }
                    };

                    exit_flag.store(true, Ordering::SeqCst);

                    process_png_data(png_data);
                })
                .unwrap();
        }

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            _ => (),
        }
    });
}

fn run_record(width: u32, height: u32, fps: u16, verbosity: u8) -> wry::Result<()> {
    let (webview, event_loop) = build_webview(width, height)?;
    // Track when we started and whether we've taken the screenshot
    let start_time = Instant::now();
    let mut last_frame_time = Instant::now();

    println!("Starting webview, will take screenshot in 5 seconds...");

    let mut count = 1;

    let (tx, rx) = mpsc::channel::<(Vec<u8>, u64)>();

    let encoder = PngVideoEncoder::new(
        "output.mkv",
        width,
        height,
        gst::Fraction::new(fps as i32, 1),
    )
    .unwrap();

    let frame_duration: Duration = Duration::from_millis(1000 / fps as u64);

    // Start encoder in a separate thread
    let encoder_handle = thread::spawn(move || {
        encoder.start().unwrap();
        while let Ok((png_data, timestamp)) = rx.recv() {
            println!("{}", timestamp);
            if png_data.is_empty() || timestamp == 0 {
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

    let mut active_webview = false;

    // Run the event loop
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll; // Use Poll to keep checking time

        let now = Instant::now();

        // Check if 5 seconds have passed and we haven't taken the screenshot yet
        if (now.duration_since(last_frame_time) >= frame_duration) && active_webview {
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
                    let timestamp_ns = count as u64 * (1_000_000_000 / fps as u64);
                    let _ = tx_clone.send((png_data, timestamp_ns));
                })
                .unwrap();

            last_frame_time = now;

            if count == 300 {
                println!("{}", start_time.elapsed().as_millis())
            }

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
            Event::RedrawRequested(_) => {
                active_webview = true;
            }
            _ => (),
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

        let h265parse = gst::ElementFactory::make("h265parse").build()?;
        let pngdec = gst::ElementFactory::make("pngdec").build()?;
        let videoconvert = gst::ElementFactory::make("videoconvert").build()?;

        let encoder = gst::ElementFactory::make("x265enc")
            .build()
            .or_else(|_| gst::ElementFactory::make("nvh265enc").build()) // NVIDIA
            .or_else(|_| gst::ElementFactory::make("vaapih265enc").build()) // Intel/AMD
            .or_else(|_| gst::ElementFactory::make("qsvh265enc").build()) // Intel QuickSync
            .or_else(|_| gst::ElementFactory::make("avenc_libx265").build()) // FFmpeg libx265
            .or_else(|_| gst::ElementFactory::make("avenc_hevc_nvenc").build()) // FFmpeg NVIDIA
            .or_else(|_| {
                gst::ElementFactory::make("x264enc")
                    .property_from_str("speed-preset", "slow")
                    .build()
            })?;

        let muxer = gst::ElementFactory::make("matroskamux").build()?;
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

        encoder.set_property("option-string", &"crf=18:threads=0");

        // Configure file sink
        filesink.set_property("location", &output_path);

        // Add elements to pipeline
        pipeline.add_many(&[
            &appsrc.clone().upcast::<gst::Element>(),
            &pngdec,
            &videoconvert,
            &h265parse,
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
            &h265parse,
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
