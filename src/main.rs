use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    path::Path,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use dpi::{PhysicalPosition, Size};
use ndarray::Array3;
use tracing_subscriber;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};
use wry::Rect;
use wry::WebViewBuilder;
use wry::WebViewExtMacOS;

use video_rs::encode::{Encoder, Settings};
use video_rs::init as video_init;
use video_rs::time::Time;

const WIDTH: u32 = 1600;
const HEIGHT: u32 = 1200;

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

type SnapshotSender = mpsc::Sender<Vec<u8>>;

struct State {
    window: Option<Window>,
    webview: Option<wry::WebView>,
    encoder: Option<Encoder>,
    next_pts: Option<Time>,
    should_record: bool,
    is_closing: bool,
    page_loaded: bool,
    snapshot_rx: Option<mpsc::Receiver<Vec<u8>>>,
    snapshot_tx: Option<SnapshotSender>,
}

impl State {
    fn new() -> Self {
        let (snapshot_tx, snapshot_rx) = mpsc::channel();

        State {
            window: None,
            webview: None,
            encoder: None,
            next_pts: None,
            should_record: false,
            is_closing: false,
            page_loaded: false,
            snapshot_rx: Some(snapshot_rx),
            snapshot_tx: Some(snapshot_tx),
        }
    }

    fn handle_snapshots(&mut self) {
        // Fix borrowing issue by taking ownership temporarily
        if let Some(rx) = self.snapshot_rx.take() {
            let mut snapshots = Vec::new();

            // Collect all available snapshots
            while let Ok(png_data) = rx.try_recv() {
                snapshots.push(png_data);
            }

            // Put the receiver back
            self.snapshot_rx = Some(rx);

            // Process collected snapshots
            for png_data in snapshots {
                if self.is_closing || !self.should_record {
                    continue;
                }

                if let Err(e) = self.encode_frame(png_data) {
                    eprintln!("Failed to encode frame: {}", e);
                }
            }
        }
    }

    fn encode_frame(&mut self, png_data: Vec<u8>) -> Result<(), String> {
        let rgb = image::load_from_memory(&png_data)
            .map_err(|e| format!("PNG decode failed: {}", e))?
            .to_rgb8();

        let frame = Array3::from_shape_fn((HEIGHT as usize, WIDTH as usize, 3), |(y, x, c)| {
            rgb.get_pixel(x as u32, y as u32).0[c]
        });

        if let (Some(encoder), Some(pts)) = (&mut self.encoder, &mut self.next_pts) {
            encoder
                .encode(&frame, *pts)
                .map_err(|e| format!("Failed to encode frame: {}", e))?;

            let frame_duration = Time::from_nth_of_a_second(60);
            *pts = pts.aligned_with(frame_duration).add();

            Ok(())
        } else {
            Err("Encoder or PTS not available".to_string())
        }
    }

    fn request_snapshot(&self) {
        if !self.should_record || self.is_closing || !self.page_loaded {
            return;
        }

        if let (Some(webview), Some(tx)) = (&self.webview, &self.snapshot_tx) {
            let tx_clone = tx.clone();

            let _ = webview.take_snapshot(None, move |result| match result {
                Ok(png_data) => {
                    let _ = tx_clone.send(png_data);
                }
                Err(e) => {
                    eprintln!("Snapshot failed: {:?}", e);
                }
            });
        }
    }
}

impl ApplicationHandler for State {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        tracing_subscriber::fmt::init();
        video_init().expect("video-rs init failed");

        let mut attr = Window::default_attributes();
        attr.decorations = false;
        let window = el.create_window(attr).unwrap();

        let settings = Settings::preset_h264_yuv420p(WIDTH as usize, HEIGHT as usize, false);
        let enc = Encoder::new(Path::new("capture.ts"), settings).unwrap();
        self.encoder = Some(enc);
        self.next_pts = Some(Time::zero());

        let webview = WebViewBuilder::new()
            .with_url("https://tauri.app")
            .with_bounds(Rect {
                position: dpi::Position::Physical(PhysicalPosition { x: 0, y: 0 }),
                size: Size::Physical(dpi::PhysicalSize {
                    width: WIDTH,
                    height: HEIGHT,
                }),
            })
            .with_on_page_load_handler(|event, url| match event {
                wry::PageLoadEvent::Started => {
                    println!("Page load started: {}", url);
                }
                wry::PageLoadEvent::Finished => {
                    println!("Page load finished: {}", url);
                }
            })
            .build_as_child(&window)
            .unwrap();

        self.window = Some(window);
        self.webview = Some(webview);

        el.set_control_flow(winit::event_loop::ControlFlow::Poll);
        self.start_recording_after_delay();
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        println!("{:?}", event);

        if let WindowEvent::CloseRequested = event {
            self.cleanup_and_exit();
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
            self.cleanup_and_exit();
            return;
        }

        self.handle_snapshots();
        self.request_snapshot(); // Add this to actually take snapshots
    }
}

impl State {
    fn start_recording_after_delay(&mut self) {
        self.page_loaded = true;
        self.should_record = true;
        println!("Recording enabled");
    }

    fn cleanup_and_exit(&mut self) {
        println!("Cleaning up and exiting...");
        self.is_closing = true;
        self.should_record = false;

        thread::sleep(Duration::from_millis(100));

        if let Some(mut encoder) = self.encoder.take() {
            println!("Finalizing encoder");
            if let Err(e) = encoder.finish() {
                eprintln!("Failed to finalize video: {}", e);
            } else {
                println!("Video finalized successfully");
            }
        }

        std::process::exit(0);
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut state = State::new();

    event_loop.run_app(&mut state).unwrap();
}
