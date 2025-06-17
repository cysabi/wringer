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

impl State {
fn main() {
    // make a webview
    // throw it some basic html
    // call Win32:CapturePreview
}
