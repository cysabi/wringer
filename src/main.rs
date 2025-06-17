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
fn main() {
    // make a webview
    // throw it some basic html
    // call Win32:CapturePreview
}
