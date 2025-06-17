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

fn main() {
    // make a webview
    // throw it some basic html
    // call Win32:CapturePreview
}
